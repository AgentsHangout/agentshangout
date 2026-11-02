package com.agentshangout.android

import java.io.File
import java.nio.file.Files
import java.util.zip.ZipEntry
import java.util.zip.ZipOutputStream
import org.junit.Assert.*
import org.junit.Test

class ClientBundleStoreTest {
  private class MemoryState : BundleStateStorage {
    private var value = BundleState()
    override fun read() = value.copy()
    override fun write(state: BundleState) { value = state.copy() }
  }

  private fun archive(root: File, files: Map<String, ByteArray>): File {
    val zip = File(root, "download.zip")
    ZipOutputStream(zip.outputStream()).use { stream ->
      files.forEach { (name, body) ->
        stream.putNextEntry(ZipEntry(name))
        stream.write(body)
        stream.closeEntry()
      }
    }
    return zip
  }

  private fun manifest(files: Map<String, ByteArray>, zip: File): ClientBundleManifest {
    val entries = files.mapValues { (_, body) -> ClientBundleFile(sha256(body), body.size.toLong()) }
    return ClientBundleManifest(bundleVersion(entries), 1, 1, entries, "mobile-assets-test", sha256(zip.readBytes()))
  }

  @Test fun downloadIsPinnedUntilNextLaunchAndFailedTrialRollsBack() {
    val root = Files.createTempDirectory("client-bundle-test").toFile()
    try {
      val state = MemoryState()
      val store = ClientBundleStore(File(root, "cache"), state)
      assertNull(store.select()) // APK seed
      val files = mapOf("style.css" to "body {}".toByteArray(), "index.html" to "<div id=app></div>".toByteArray(), "app.js" to "ready()".toByteArray())
      val zip = archive(root, files)
      val update = manifest(files, zip)
      store.stage(update, zip)
      assertNull(store.select()) // Never mix a download into a running document.
      val trial = ClientBundleStore(File(root, "cache"), state)
      assertEquals(update.version, trial.select()?.version)
      assertNull(ClientBundleStore(File(root, "cache"), state).select()) // No ready signal -> APK seed.
      assertEquals(update.version, state.read().rejected)
    } finally { root.deleteRecursively() }
  }

  @Test fun healthyTrialPersistsAndCorruptUpdateCannotReplaceIt() {
    val root = Files.createTempDirectory("client-bundle-test").toFile()
    try {
      val state = MemoryState()
      val files = mapOf("style.css" to "body {}".toByteArray(), "index.html" to "seed".toByteArray(), "app.js" to "first".toByteArray())
      val zip = archive(root, files)
      val first = manifest(files, zip)
      ClientBundleStore(File(root, "cache"), state).stage(first, zip)
      val trial = ClientBundleStore(File(root, "cache"), state)
      assertEquals(first.version, trial.select()?.version)
      trial.markHealthy()
      val running = ClientBundleStore(File(root, "cache"), state)
      assertEquals(first.version, running.select()?.version)
      val changed = files + ("app.js" to "second".toByteArray())
      val nextZip = archive(root, changed)
      val bad = manifest(changed, nextZip).copy(files = manifest(changed, nextZip).files +
        ("app.js" to ClientBundleFile("0".repeat(64), 6)))
      try { running.stage(bad.copy(version = bundleVersion(bad.files)), nextZip); fail("Corrupt update was accepted") }
      catch (_: IllegalArgumentException) {}
      assertEquals(first.version, ClientBundleStore(File(root, "cache"), state).select()?.version)
    } finally { root.deleteRecursively() }
  }

  @Test fun interruptedAndWrongArchiveLeaveLastGoodBundleAvailableOffline() {
    val root = Files.createTempDirectory("client-bundle-test").toFile()
    try {
      val state = MemoryState()
      val files = mapOf("style.css" to "body {}".toByteArray(), "index.html" to "ok".toByteArray(), "app.js" to "one".toByteArray())
      val zip = archive(root, files)
      val first = manifest(files, zip)
      ClientBundleStore(File(root, "cache"), state).stage(first, zip)
      val trial = ClientBundleStore(File(root, "cache"), state)
      trial.select()
      trial.markHealthy()
      val running = ClientBundleStore(File(root, "cache"), state)
      val selected = running.select()
      val nextFiles = files + ("app.js" to "two".toByteArray())
      val nextZip = archive(root, nextFiles)
      val next = manifest(nextFiles, nextZip)
      nextZip.writeBytes(nextZip.readBytes().take(20).toByteArray()) // An interrupted download.
      try { running.stage(next, nextZip); fail("Truncated archive was accepted") } catch (_: IllegalArgumentException) {}
      assertEquals(selected, running.select())
      assertNull(state.read().pending)
      assertEquals(first.version, ClientBundleStore(File(root, "cache"), state).select()?.version)
      assertFalse(File(root, "cache").listFiles()!!.any { it.name.startsWith("staging-") })
    } finally { root.deleteRecursively() }
  }

  @Test fun unhealthyNewVersionReturnsToPreviousHealthyDownload() {
    val root = Files.createTempDirectory("client-bundle-test").toFile()
    try {
      val state = MemoryState()
      val firstFiles = mapOf("style.css" to "body {}".toByteArray(), "index.html" to "ok".toByteArray(), "app.js" to "one".toByteArray())
      val firstZip = archive(root, firstFiles)
      val first = manifest(firstFiles, firstZip)
      ClientBundleStore(File(root, "cache"), state).stage(first, firstZip)
      val firstLaunch = ClientBundleStore(File(root, "cache"), state)
      firstLaunch.select()
      firstLaunch.markHealthy()
      val nextFiles = firstFiles + ("app.js" to "two".toByteArray())
      val nextZip = archive(root, nextFiles)
      val next = manifest(nextFiles, nextZip)
      firstLaunch.stage(next, nextZip)
      assertEquals(next.version, ClientBundleStore(File(root, "cache"), state).select()?.version)
      val fallback = ClientBundleStore(File(root, "cache"), state)
      assertEquals(first.version, fallback.select()?.version)
      assertFalse(fallback.shouldDownload(next.version))
    } finally { root.deleteRecursively() }
  }

  @Test fun traversalUnsupportedContractAndMissingFilesAreRejected() {
    val root = Files.createTempDirectory("client-bundle-test").toFile()
    try {
      val store = ClientBundleStore(File(root, "cache"), MemoryState())
      val files = mapOf("style.css" to "body {}".toByteArray(), "index.html" to "ok".toByteArray(), "app.js" to "ok".toByteArray())
      val zip = archive(root, files)
      val valid = manifest(files, zip)
      for (invalid in listOf(valid.copy(apiContract = 2), valid.copy(minNative = 2),
          valid.copy(files = valid.files + ("../escape.js" to ClientBundleFile("0".repeat(64), 1))))) {
        try { store.stage(invalid, zip); fail("Incompatible manifest was accepted") } catch (_: IllegalArgumentException) {}
      }
      val missing = valid.copy(files = valid.files + ("missing.js" to ClientBundleFile(sha256("ok".toByteArray()), 2)))
      try { store.stage(missing.copy(version = bundleVersion(missing.files)), zip); fail("Incomplete archive was accepted") }
      catch (_: IllegalArgumentException) {}
      assertFalse(File(root, "escape.js").exists())
      assertNull(store.select())
    } finally { root.deleteRecursively() }
  }
}
