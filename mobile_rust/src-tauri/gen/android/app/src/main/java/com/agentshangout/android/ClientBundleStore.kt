package com.agentshangout.android

import java.io.File
import java.security.MessageDigest
import java.util.UUID
import java.util.zip.ZipInputStream
import org.json.JSONObject

internal const val CLIENT_API_CONTRACT = 1
internal const val CLIENT_NATIVE_CONTRACT = 1
internal const val MAX_BUNDLE_BYTES = 32L * 1024 * 1024
internal const val MAX_ARCHIVE_BYTES = 16L * 1024 * 1024
private const val MAX_FILE_BYTES = 8L * 1024 * 1024

internal fun sha256(bytes: ByteArray): String = MessageDigest.getInstance("SHA-256")
  .digest(bytes).joinToString("") { "%02x".format(it) }

internal data class ClientBundleFile(val sha256: String, val size: Long)

// Content addressed independently of the release name or ZIP encoding.
internal fun bundleVersion(files: Map<String, ClientBundleFile>): String = sha256(
  files.toSortedMap().entries.joinToString("") { (path, file) -> "$path\u0000${file.sha256}\u0000${file.size}\n" }
    .toByteArray(Charsets.UTF_8)
)

internal data class ClientBundleManifest(
  val version: String,
  val apiContract: Int,
  val minNative: Int,
  val files: Map<String, ClientBundleFile>,
  val release: String = "",
  val archiveSha256: String = "",
) {
  fun validate() {
    require(apiContract == CLIENT_API_CONTRACT && minNative in 1..CLIENT_NATIVE_CONTRACT) {
      "This client bundle needs a different native app or server API contract"
    }
    require(files.size in 3..256 && files.keys.containsAll(listOf("index.html", "app.js", "style.css"))) { "Incomplete client bundle" }
    require(files.keys.all { path ->
      path.matches(Regex("[a-z][a-z0-9-]*\\.(js|css|svg|png|jpg|webp)")) || path == "index.html"
    }) { "Unsafe client asset path" }
    require(files.values.all { it.size in 1..MAX_FILE_BYTES && it.sha256.matches(Regex("[a-f0-9]{64}")) }) {
      "Invalid client asset digest or size"
    }
    require(files.values.sumOf { it.size } <= MAX_BUNDLE_BYTES) { "Client bundle is too large" }
    require(version == bundleVersion(files)) { "Client bundle version does not match its contents" }
  }

  fun json(): String = JSONObject().put("format", 1).put("version", version)
    .put("api_contract", apiContract).put("min_native", minNative).put("release", release)
    .put("archive_sha256", archiveSha256).put("files", JSONObject().apply {
      files.toSortedMap().forEach { (path, file) ->
        put(path, JSONObject().put("sha256", file.sha256).put("size", file.size))
      }
    }).toString()

  companion object {
    fun parse(text: String): ClientBundleManifest {
      require(text.length <= 128 * 1024) { "Client manifest is too large" }
      val json = JSONObject(text)
      require(json.getInt("format") == 1) { "Unsupported client manifest format" }
      val files = json.getJSONObject("files")
      return ClientBundleManifest(json.getString("version"), json.getInt("api_contract"),
        json.getInt("min_native"), files.keys().asSequence().associateWith { path ->
          files.getJSONObject(path).let { ClientBundleFile(it.getString("sha256"), it.getLong("size")) }
        }, json.optString("release"), json.optString("archive_sha256")).also { it.validate() }
    }
  }
}

internal data class BundleState(
  var active: String? = null,
  var previous: String? = null,
  var pending: String? = null,
  var trial: String? = null,
  var rejected: String? = null,
)

internal interface BundleStateStorage {
  fun read(): BundleState
  fun write(state: BundleState)
}

/** A selected bundle stays pinned for this process. Downloading never changes a running document. */
internal class ClientBundleStore(private val root: File, private val storage: BundleStateStorage) {
  private var selected = false
  private var selection: ClientBundleManifest? = null

  @Synchronized fun select(): ClientBundleManifest? {
    if (selected) return selection
    root.mkdirs()
    val state = storage.read()
    if (state.trial != null) {
      // The previous process did not reach a usable client screen. Return to the last good bundle.
      state.rejected = state.trial
      state.active = state.previous
      state.trial = null
      state.pending = null
    }
    state.pending?.let { pending ->
      if (pending != state.active && pending != state.rejected && verified(pending) != null) {
        state.previous = state.active
        state.active = pending
        state.trial = pending
      }
      state.pending = null
    }
    selection = state.active?.let(::verified)
    if (state.active != null && selection == null) {
      state.rejected = state.active
      state.active = state.previous
      state.trial = null
      selection = state.active?.let(::verified)
      if (selection == null) state.active = null // The APK is always a usable final fallback.
    }
    storage.write(state)
    selected = true
    return selection
  }

  @Synchronized fun markHealthy() {
    val state = storage.read()
    if (selection != null && state.trial == selection?.version) {
      state.trial = null
      storage.write(state)
    }
  }

  @Synchronized fun shouldDownload(version: String): Boolean {
    val state = storage.read()
    return version != state.active && version != state.pending && version != state.rejected
  }

  fun asset(manifest: ClientBundleManifest, path: String): File = File(File(root, manifest.version), path)

  @Synchronized fun stage(manifest: ClientBundleManifest, archive: File) {
    manifest.validate()
    require(manifest.archiveSha256.matches(Regex("[a-f0-9]{64}"))) { "Missing archive checksum" }
    require(archive.length() in 1..MAX_ARCHIVE_BYTES) { "Client archive is too large" }
    require(sha256(archive.readBytes()) == manifest.archiveSha256) { "Client archive checksum mismatch" }
    root.mkdirs()
    val staging = File(root, "staging-${UUID.randomUUID()}")
    require(staging.mkdir()) { "Could not create client staging directory" }
    try {
      val seen = mutableSetOf<String>()
      var total = 0L
      ZipInputStream(archive.inputStream().buffered()).use { zip ->
        while (true) {
          val entry = zip.nextEntry ?: break
          val expected = manifest.files[entry.name]
          require(!entry.isDirectory && expected != null && seen.add(entry.name)) { "Unexpected client archive entry" }
          val target = File(staging, entry.name)
          var size = 0L
          val digest = MessageDigest.getInstance("SHA-256")
          target.outputStream().buffered().use { output ->
            val buffer = ByteArray(16 * 1024)
            while (true) {
              val count = zip.read(buffer)
              if (count < 0) break
              size += count
              total += count
              require(size <= expected.size && total <= MAX_BUNDLE_BYTES) { "Client archive exceeds declared size" }
              digest.update(buffer, 0, count)
              output.write(buffer, 0, count)
            }
          }
          require(size == expected.size && digest.digest().joinToString("") { "%02x".format(it) } == expected.sha256) {
            "Client asset checksum mismatch"
          }
          zip.closeEntry()
        }
      }
      require(seen == manifest.files.keys) { "Client archive has missing assets" }
      File(staging, "manifest.json").writeText(manifest.json())
      val destination = File(root, manifest.version)
      if (!destination.exists()) require(staging.renameTo(destination)) { "Could not activate verified download" }
      require(verified(manifest.version) != null) { "Stored client bundle failed verification" }
      val state = storage.read()
      state.pending = manifest.version
      storage.write(state) // Only a complete, verified directory can become pending.
      // Keep the running process, last good version and next-launch candidate. Nothing else in
      // this private cache is needed; staging failures never become an active bundle.
      val keep = setOfNotNull(state.active, state.previous, state.pending, selection?.version)
      root.listFiles()?.filter { it.isDirectory && it.name.matches(Regex("[a-f0-9]{64}")) && it.name !in keep }
        ?.forEach { it.deleteRecursively() }
    } finally {
      staging.deleteRecursively()
    }
  }

  private fun verified(version: String): ClientBundleManifest? = try {
    require(version.matches(Regex("[a-f0-9]{64}")))
    val directory = File(root, version)
    val manifest = ClientBundleManifest.parse(File(directory, "manifest.json").readText())
    require(manifest.version == version)
    require(manifest.files.all { (path, expected) ->
      val file = File(directory, path)
      file.length() == expected.size && sha256(file.readBytes()) == expected.sha256
    })
    manifest
  } catch (_: Exception) { null }
}
