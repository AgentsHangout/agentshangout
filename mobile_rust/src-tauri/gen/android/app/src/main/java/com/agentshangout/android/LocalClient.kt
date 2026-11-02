package com.agentshangout.android

import android.content.Context
import android.graphics.Bitmap
import android.net.Uri
import android.net.http.SslError
import android.os.Build
import android.os.Message
import android.util.AtomicFile
import android.util.Log
import android.view.KeyEvent
import android.webkit.*
import androidx.webkit.WebViewCompat
import java.io.File
import java.io.InputStream
import java.net.HttpURLConnection
import java.net.URL
import java.util.Properties
import javax.net.ssl.HttpsURLConnection
import org.json.JSONArray

/** Public assets only. Cookies, authentication, API calls and event streams stay in the WebView. */
internal class LocalClient private constructor(private val context: Context) {
  private val directory = File(context.filesDir, "client-bundles")
  private val stateFile = AtomicFile(File(directory, "state.properties"))
  private val store = ClientBundleStore(directory, object : BundleStateStorage {
    override fun read(): BundleState = try {
      val values = Properties().apply { stateFile.openRead().use { load(it) } }
      fun value(key: String) = values.getProperty(key)?.takeIf { it.matches(Regex("[a-f0-9]{64}")) }
      BundleState(value("active"), value("previous"), value("pending"), value("trial"), value("rejected"))
    } catch (_: Exception) { BundleState() }

    override fun write(state: BundleState) {
      directory.mkdirs()
      val stream = stateFile.startWrite()
      try {
        Properties().apply {
          state.active?.let { setProperty("active", it) }
          state.previous?.let { setProperty("previous", it) }
          state.pending?.let { setProperty("pending", it) }
          state.trial?.let { setProperty("trial", it) }
          state.rejected?.let { setProperty("rejected", it) }
        }.store(stream, null)
        stateFile.finishWrite(stream)
      } catch (error: Exception) {
        stateFile.failWrite(stream)
        throw error
      }
    }
  })
  private val downloaded = store.select()
  private val manifest = downloaded ?: context.assets.open("client/manifest.json").bufferedReader().use {
    ClientBundleManifest.parse(it.readText())
  }

  init {
    Log.i("AgentsHangoutClient", "Local client ${manifest.version.take(12)} (${if (downloaded == null) "APK" else "download"})")
    Thread({
      try { checkForUpdate() }
      catch (error: Exception) {
        // No URL, invite, session, or response body is logged. Offline is normal.
        Log.i("AgentsHangoutClient", "Keeping installed client (${error.javaClass.simpleName})")
      }
    }, "AgentsHangout-assets").start()
  }

  fun intercept(request: WebResourceRequest): WebResourceResponse? {
    if (request.method != "GET" || !isProduction(request.url)) return null
    val path = request.url.path ?: return null
    val asset = when {
      path == "/" || SPA_ROUTE.matches(path) -> "index.html"
      path.removePrefix("/") in manifest.files -> path.removePrefix("/")
      path.matches(Regex("/[^/]+\\.(js|css|svg|png|jpg|webp)")) -> return missing()
      else -> return null // In particular: /api, /docs, /privacy, and OAuth start URLs.
    }
    val body = try {
      if (downloaded == null) context.assets.open("client/$asset") else store.asset(manifest, asset).inputStream()
    } catch (_: Exception) {
      // Never silently mix a server file into a pinned client version.
      return missing()
    }
    val mime = when (asset.substringAfterLast('.')) {
      "html" -> "text/html"
      "js" -> "text/javascript"
      "css" -> "text/css"
      "svg" -> "image/svg+xml"
      "jpg" -> "image/jpeg"
      "webp" -> "image/webp"
      else -> "image/png"
    }
    return WebResourceResponse(mime, "UTF-8", 200, "OK", mapOf(
      "Cache-Control" to "no-store",
      "Content-Security-Policy" to "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'",
      "X-Content-Type-Options" to "nosniff",
      "Referrer-Policy" to "no-referrer",
      "X-AgentsHangout-Client" to manifest.version,
    ), body)
  }

  fun probeReady(webView: WebView, url: String, remaining: Int = 30) {
    if (downloaded == null || remaining <= 0 || !isProduction(Uri.parse(url))) return
    webView.postDelayed({
      // A successful route sets this flag; a network/error screen deliberately does not.
      webView.evaluateJavascript("location.origin === 'https://agentshangout.com' && document.documentElement.dataset.clientReady === 'true'") { ready ->
        if (ready == "true") store.markHealthy()
        else if (webView.url == url) probeReady(webView, url, remaining - 1)
      }
    }, 500)
  }

  private fun checkForUpdate() {
    val releases = JSONArray(download(RELEASES_URL, 1024L * 1024).toString(Charsets.UTF_8))
    val release = (0 until releases.length()).map { releases.getJSONObject(it) }
      .filter { candidate ->
        val tag = candidate.optString("tag_name")
        val assets = candidate.optJSONArray("assets")
        val names = if (assets == null) emptySet() else
          (0 until assets.length()).map { assets.getJSONObject(it).optString("name") }.toSet()
        !candidate.optBoolean("draft", true) && !candidate.optBoolean("prerelease", true) &&
          tag.matches(Regex("mobile-assets-[A-Za-z0-9._-]{1,80}")) &&
          names.containsAll(listOf("android-client.json", "android-client.zip"))
      }.maxByOrNull { it.optString("published_at") }?.getString("tag_name") ?: return
    val text = download("https://github.com/AgentsHangout/agentshangout/releases/download/$release/android-client.json", 128L * 1024)
      .toString(Charsets.UTF_8)
    val next = ClientBundleManifest.parse(text)
    require(next.release == release) { "Client manifest belongs to a different release" }
    if (next.version == manifest.version || !store.shouldDownload(next.version)) return
    val archive = File.createTempFile("client-download-", ".zip", context.cacheDir)
    try {
      archive.writeBytes(download("https://github.com/AgentsHangout/agentshangout/releases/download/${next.release}/android-client.zip", MAX_ARCHIVE_BYTES))
      store.stage(next, archive)
      Log.i("AgentsHangoutClient", "Verified client ${next.version.take(12)} ready for next launch")
    } finally { archive.delete() }
  }

  companion object {
    private const val RELEASES_URL = "https://api.github.com/repos/AgentsHangout/agentshangout/releases?per_page=30"
    // These are precisely the server's index routes. OAuth callback routes must run the same client.
    private val SPA_ROUTE = Regex("/(?:h|join|c|world|house|s|join-city|join-house|p)/[^/]+|/worlds|/discord/linked|/sign-in/back")
    @Volatile private var instance: LocalClient? = null

    fun install(context: Context, webView: WebView) {
      // A custom development server keeps the external loading behavior.
      if (!isProduction(Uri.parse(BuildConfig.AGENTSHANGOUT_URL))) {
        webView.loadUrl(BuildConfig.AGENTSHANGOUT_URL)
        return
      }
      val client = instance ?: synchronized(this) {
        instance ?: LocalClient(context.applicationContext).also { instance = it }
      }
      val delegate = WebViewCompat.getWebViewClient(webView) ?: WebViewClient()
      webView.webViewClient = LocalClientWebViewClient(delegate, client)
      webView.loadUrl(BuildConfig.AGENTSHANGOUT_URL)
    }

    private fun isProduction(url: Uri): Boolean = url.scheme == "https" &&
      url.host == "agentshangout.com" && url.port in listOf(-1, 443) && url.userInfo == null

    private fun missing() = WebResourceResponse("text/plain", "UTF-8", 404, "Not Found",
      mapOf("Cache-Control" to "no-store", "X-Content-Type-Options" to "nosniff"), "Client asset unavailable".byteInputStream())

    private fun download(address: String, limit: Long): ByteArray {
      var url = URL(address)
      repeat(6) {
        require(url.protocol == "https" && url.host in setOf("api.github.com", "github.com", "release-assets.githubusercontent.com", "objects.githubusercontent.com")) {
          "Untrusted client download location"
        }
        val connection = url.openConnection() as HttpsURLConnection
        try {
          connection.instanceFollowRedirects = false
          connection.connectTimeout = 10_000
          connection.readTimeout = 15_000
          connection.setRequestProperty("User-Agent", "AgentsHangout-Android-client/1")
          connection.setRequestProperty("Accept-Encoding", "identity")
          val status = connection.responseCode
          if (status in listOf(301, 302, 303, 307, 308)) {
            url = URL(url, connection.getHeaderField("Location") ?: error("Missing redirect"))
          } else {
            require(status == HttpURLConnection.HTTP_OK) { "Client update unavailable" }
            require(connection.contentLengthLong <= limit) { "Client update is too large" }
            return connection.inputStream.use { stream -> readLimited(stream, limit) }
          }
        } finally { connection.disconnect() }
      }
      error("Too many client download redirects")
    }

    private fun readLimited(input: InputStream, limit: Long): ByteArray {
      val output = java.io.ByteArrayOutputStream()
      val buffer = ByteArray(16 * 1024)
      var size = 0L
      while (true) {
        val count = input.read(buffer)
        if (count < 0) break
        size += count
        require(size <= limit) { "Client update exceeds size limit" }
        output.write(buffer, 0, count)
      }
      return output.toByteArray()
    }
  }
}

/** Preserve Wry's navigation, IPC/lifecycle and security behavior while intercepting public assets. */
@Suppress("DEPRECATION")
private class LocalClientWebViewClient(private val delegate: WebViewClient, private val client: LocalClient) : WebViewClient() {
  override fun shouldInterceptRequest(view: WebView, request: WebResourceRequest): WebResourceResponse? {
    // Wry records external navigation here. Its older-WebView document-start fallback relies on
    // that record even when our static response supplies the bytes.
    val response = delegate.shouldInterceptRequest(view, request)
    return client.intercept(request) ?: response
  }
  override fun shouldInterceptRequest(view: WebView, url: String) = delegate.shouldInterceptRequest(view, url)
  override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest) = delegate.shouldOverrideUrlLoading(view, request)
  override fun shouldOverrideUrlLoading(view: WebView, url: String) = delegate.shouldOverrideUrlLoading(view, url)
  override fun onPageStarted(view: WebView, url: String, icon: Bitmap?) = delegate.onPageStarted(view, url, icon)
  override fun onPageFinished(view: WebView, url: String) {
    delegate.onPageFinished(view, url)
    client.probeReady(view, url)
  }
  override fun onLoadResource(view: WebView, url: String) = delegate.onLoadResource(view, url)
  override fun onPageCommitVisible(view: WebView, url: String) = delegate.onPageCommitVisible(view, url)
  override fun onReceivedError(view: WebView, request: WebResourceRequest, error: WebResourceError) = delegate.onReceivedError(view, request, error)
  override fun onReceivedError(view: WebView, code: Int, description: String, url: String) = delegate.onReceivedError(view, code, description, url)
  override fun onReceivedHttpError(view: WebView, request: WebResourceRequest, response: WebResourceResponse) = delegate.onReceivedHttpError(view, request, response)
  override fun onReceivedSslError(view: WebView, handler: SslErrorHandler, error: SslError) = delegate.onReceivedSslError(view, handler, error)
  override fun onReceivedClientCertRequest(view: WebView, request: ClientCertRequest) = delegate.onReceivedClientCertRequest(view, request)
  override fun onReceivedHttpAuthRequest(view: WebView, handler: HttpAuthHandler, host: String, realm: String) = delegate.onReceivedHttpAuthRequest(view, handler, host, realm)
  override fun onFormResubmission(view: WebView, dontResend: Message, resend: Message) = delegate.onFormResubmission(view, dontResend, resend)
  override fun doUpdateVisitedHistory(view: WebView, url: String, reload: Boolean) = delegate.doUpdateVisitedHistory(view, url, reload)
  override fun shouldOverrideKeyEvent(view: WebView, event: KeyEvent) = delegate.shouldOverrideKeyEvent(view, event)
  override fun onUnhandledKeyEvent(view: WebView, event: KeyEvent) = delegate.onUnhandledKeyEvent(view, event)
  override fun onScaleChanged(view: WebView, old: Float, new: Float) = delegate.onScaleChanged(view, old, new)
  override fun onReceivedLoginRequest(view: WebView, realm: String, account: String?, args: String) = delegate.onReceivedLoginRequest(view, realm, account, args)
  override fun onRenderProcessGone(view: WebView, detail: RenderProcessGoneDetail): Boolean =
    if (Build.VERSION.SDK_INT >= 26) delegate.onRenderProcessGone(view, detail) else false
  override fun onSafeBrowsingHit(view: WebView, request: WebResourceRequest, threat: Int, response: SafeBrowsingResponse) {
    if (Build.VERSION.SDK_INT >= 27) delegate.onSafeBrowsingHit(view, request, threat, response)
  }
}
