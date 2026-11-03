package com.agentshangout.android

import android.content.res.Configuration
import android.os.Bundle
import androidx.activity.enableEdgeToEdge
import android.webkit.WebView
import android.Manifest
import android.content.Intent
import androidx.activity.result.contract.ActivityResultContracts

class MainActivity : TauriActivity() {
  private var phoneWebView: WebView? = null
  private val pushSettings: PushSettingsController by lazy { PushSettingsController(this) { notificationPermission.launch(Manifest.permission.POST_NOTIFICATIONS) } }
  private val notificationPermission = registerForActivityResult(ActivityResultContracts.RequestPermission()) {
    pushSettings.permissionResult(it)
  }
  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
  }

  override fun onWebViewCreate(webView: WebView) {
    super.onWebViewCreate(webView)
    // Wry attaches its own client after this callback. Wait for that, then retain it as a delegate.
    // Rust opens about:blank first, so no remote document races the local client installation.
    webView.post {
      phoneWebView = webView
      pushSettings.attach(webView)
      LocalClient.install(this, webView)
      openNotification(intent)
    }
  }

  override fun onResume() { super.onResume(); pushSettings.resume(); LocalClient.resume() }
  override fun onDestroy() { pushSettings.destroy(); phoneWebView = null; super.onDestroy() }
  override fun onNewIntent(intent: Intent) { super.onNewIntent(intent); setIntent(intent); openNotification(intent) }
  private fun openNotification(intent: Intent) {
    val path = PushPolicy.safePath(intent.getStringExtra(PushNotifications.PATH_EXTRA)) ?: return
    if (BuildConfig.AGENTSHANGOUT_URL.trimEnd('/') == PushNotifications.ORIGIN && phoneWebView != null) {
      phoneWebView?.loadUrl(PushNotifications.ORIGIN + path)
      intent.removeExtra(PushNotifications.PATH_EXTRA)
    }
  }

  // The manifest keeps this activity across a turn, so ask the WebView to take the new size.
  // The page then moves the tab bar to the side.
  override fun onConfigurationChanged(newConfig: Configuration) {
    super.onConfigurationChanged(newConfig)
    window.decorView.requestLayout()
  }
}
