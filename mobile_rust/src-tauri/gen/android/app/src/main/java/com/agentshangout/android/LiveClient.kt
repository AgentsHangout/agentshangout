package com.agentshangout.android

import android.graphics.Bitmap
import android.net.http.SslError
import android.os.Build
import android.os.Message
import android.view.KeyEvent
import android.webkit.*
import androidx.webkit.WebViewCompat

/** Keep Wry's navigation, IPC, certificate and security behavior intact. */
internal object LiveClient {
  fun install(activity: MainActivity, webView: WebView) {
    val delegate = WebViewCompat.getWebViewClient(webView) ?: WebViewClient()
    webView.webViewClient = LiveWebViewClient(delegate, activity)
    webView.loadUrl(BuildConfig.AGENTSHANGOUT_URL)
  }
}

@Suppress("DEPRECATION")
private class LiveWebViewClient(private val delegate: WebViewClient, private val activity: MainActivity) : WebViewClient() {
  override fun shouldInterceptRequest(view: WebView, request: WebResourceRequest): WebResourceResponse? {
    return delegate.shouldInterceptRequest(view, request)
  }
  override fun shouldInterceptRequest(view: WebView, url: String) = delegate.shouldInterceptRequest(view, url)
  override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest) = delegate.shouldOverrideUrlLoading(view, request)
  override fun shouldOverrideUrlLoading(view: WebView, url: String) = delegate.shouldOverrideUrlLoading(view, url)
  override fun onPageStarted(view: WebView, url: String, icon: Bitmap?) {
    activity.pageStarted(url)
    delegate.onPageStarted(view, url, icon)
  }
  override fun onPageFinished(view: WebView, url: String) {
    delegate.onPageFinished(view, url)
  }
  override fun onLoadResource(view: WebView, url: String) = delegate.onLoadResource(view, url)
  override fun onPageCommitVisible(view: WebView, url: String) = delegate.onPageCommitVisible(view, url)
  override fun onReceivedError(view: WebView, request: WebResourceRequest, error: WebResourceError) {
    delegate.onReceivedError(view, request, error)
    activity.networkError(view, request.url.toString(), request.isForMainFrame, error.errorCode)
  }
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
