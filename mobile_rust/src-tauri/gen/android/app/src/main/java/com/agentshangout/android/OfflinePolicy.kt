package com.agentshangout.android

import java.net.URI

/** Only retry a failed top-level network navigation on the configured server origin. */
internal object OfflinePolicy {
  private val networkErrors = setOf(-1, -2, -6, -7, -8, -9)

  fun retryUrl(base: String, failed: String, mainFrame: Boolean, code: Int): String? {
    if (!mainFrame || code !in networkErrors || failed.any { it.isISOControl() }) return null
    return try {
      val origin = URI(base)
      val url = URI(failed)
      fun port(uri: URI): Int = if (uri.port >= 0) uri.port else if (uri.scheme.equals("https", true)) 443 else 80
      if (origin.scheme?.lowercase() !in setOf("https", "http") || url.scheme?.equals(origin.scheme, true) != true ||
          origin.host == null || url.host?.equals(origin.host, true) != true ||
          origin.userInfo != null || url.userInfo != null || port(origin) != port(url)) null else failed
    } catch (_: IllegalArgumentException) { null } catch (_: java.net.URISyntaxException) { null }
  }
}
