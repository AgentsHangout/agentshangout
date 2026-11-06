package com.agentshangout.android

import android.content.res.Configuration
import android.os.Bundle
import androidx.activity.enableEdgeToEdge
import android.webkit.WebView
import android.Manifest
import android.content.Intent
import android.graphics.Color
import android.view.Gravity
import android.view.ViewGroup
import android.widget.Button
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.TextView
import androidx.activity.result.contract.ActivityResultContracts

class MainActivity : TauriActivity() {
  private var phoneWebView: WebView? = null
  private var offlineView: FrameLayout? = null
  private var offlineWebView: WebView? = null
  private var savedAccessibility = android.view.View.IMPORTANT_FOR_ACCESSIBILITY_AUTO
  private var savedFocusable = false
  private var currentPage: String? = null
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
    // Rust opens about:blank first, so the live page cannot race installation.
    webView.post {
      phoneWebView = webView
      pushSettings.attach(webView)
      LiveClient.install(this, webView)
      openNotification(intent)
    }
  }

  override fun onResume() { super.onResume(); pushSettings.resume() }
  override fun onDestroy() { hideOffline(); pushSettings.destroy(); phoneWebView = null; super.onDestroy() }
  internal fun pageStarted(url: String) {
    currentPage = url
    if (OfflinePolicy.retryUrl(BuildConfig.AGENTSHANGOUT_URL, url, true, -2) != null) hideOffline()
  }

  internal fun networkError(view: WebView, url: String, mainFrame: Boolean, code: Int) {
    val retry = OfflinePolicy.retryUrl(BuildConfig.AGENTSHANGOUT_URL, url, mainFrame, code) ?: return
    if (currentPage != retry || isFinishing || isDestroyed) return
    hideOffline()
    savedAccessibility = view.importantForAccessibility
    savedFocusable = view.isFocusable
    offlineWebView = view
    view.importantForAccessibility = android.view.View.IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS
    view.isFocusable = false
    view.clearFocus()
    val density = resources.displayMetrics.density
    val overlay = FrameLayout(this).apply {
      setBackgroundColor(Color.WHITE)
      isClickable = true
      isFocusable = true
      importantForAccessibility = android.view.View.IMPORTANT_FOR_ACCESSIBILITY_YES
    }
    val content = LinearLayout(this).apply {
      orientation = LinearLayout.VERTICAL
      gravity = Gravity.CENTER
      val padding = (24 * density).toInt()
      setPadding(padding, padding, padding, padding)
      addView(TextView(this@MainActivity).apply {
        text = "Can't connect to Agents Hangout. Check your connection and try again."
        textSize = 18f
        setTextColor(Color.BLACK)
        gravity = Gravity.CENTER
      })
      addView(Button(this@MainActivity).apply {
        text = "Retry"
        minHeight = (48 * density).toInt()
        setOnClickListener {
          hideOffline()
          view.loadUrl(retry)
        }
      })
    }
    overlay.addView(content, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
    addContentView(overlay, ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
    offlineView = overlay
    overlay.requestFocus()
    overlay.announceForAccessibility("Can't connect to Agents Hangout. Retry.")
  }

  private fun hideOffline() {
    offlineView?.let { (it.parent as? ViewGroup)?.removeView(it) }
    offlineView = null
    offlineWebView?.let {
      it.importantForAccessibility = savedAccessibility
      it.isFocusable = savedFocusable
    }
    offlineWebView = null
  }

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
