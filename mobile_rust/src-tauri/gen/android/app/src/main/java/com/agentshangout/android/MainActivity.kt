package com.agentshangout.android

import android.content.res.Configuration
import android.os.Bundle
import androidx.activity.enableEdgeToEdge
import android.webkit.WebView

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
  }

  override fun onWebViewCreate(webView: WebView) {
    super.onWebViewCreate(webView)
    // Wry attaches its own client after this callback. Wait for that, then retain it as a delegate.
    // Rust opens about:blank first, so no remote document races the local client installation.
    webView.post { LocalClient.install(this, webView) }
  }

  // The manifest keeps this activity across a turn, so ask the WebView to take the new size.
  // The page then moves the tab bar to the side.
  override fun onConfigurationChanged(newConfig: Configuration) {
    super.onConfigurationChanged(newConfig)
    window.decorView.requestLayout()
  }
}
