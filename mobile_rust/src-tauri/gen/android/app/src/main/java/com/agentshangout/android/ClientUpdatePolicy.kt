package com.agentshangout.android

import org.json.JSONArray
import org.json.JSONObject

/** Discovery and foreground scheduling, independent of Android and of download activation. */
internal class ClientUpdatePolicy {
  private var checking = false
  private var lastCheck: Long? = null

  @Synchronized fun begin(now: Long): Boolean {
    if (checking || lastCheck?.let { now - it < 300_000 } == true) return false
    checking = true
    lastCheck = now
    return true
  }

  @Synchronized fun finish() { checking = false }

  companion object {
    fun findRelease(fetchPage: (Int) -> JSONArray): String? {
      var newest: JSONObject? = null
      for (page in 1..3) {
        val releases = fetchPage(page)
        for (index in 0 until releases.length()) {
          val candidate = releases.getJSONObject(index)
          val assets = candidate.optJSONArray("assets") ?: continue
          val names = (0 until assets.length()).map { assets.getJSONObject(it).optString("name") }.toSet()
          if (candidate.optBoolean("draft", true) || candidate.optBoolean("prerelease", true) ||
              !candidate.optString("tag_name").matches(Regex("mobile-assets-[A-Za-z0-9._-]{1,80}")) ||
              !names.containsAll(listOf("android-client.json", "android-client.zip"))) continue
          if (newest == null || candidate.optString("published_at") > newest.optString("published_at")) newest = candidate
        }
        if (releases.length() < 100) break
      }
      return newest?.getString("tag_name")
    }
  }
}
