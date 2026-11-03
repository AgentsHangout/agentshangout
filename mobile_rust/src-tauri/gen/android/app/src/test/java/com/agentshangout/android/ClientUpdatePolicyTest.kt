package com.agentshangout.android

import org.json.JSONArray
import org.junit.Assert.*
import org.junit.Test

class ClientUpdatePolicyTest {
  private fun release(tag: String, published: String = "2026-11-03T00:00:00Z") =
    """{"tag_name":"$tag","published_at":"$published","draft":false,"prerelease":false,"assets":[{"name":"android-client.json"},{"name":"android-client.zip"}]}"""

  @Test fun assetReleaseSurvivesMoreThanThirtyServerReleases() {
    val pages = mutableListOf<Int>()
    val selected = ClientUpdatePolicy.findRelease { page ->
      pages.add(page)
      if (page == 1) JSONArray((1..100).joinToString(",", "[", "]") { release("v0.1.$it") })
      else JSONArray("[${release("mobile-assets-test")}]")
    }
    assertEquals("mobile-assets-test", selected)
    assertEquals(listOf(1, 2), pages)
  }

  @Test fun rejectsDraftsIncompleteAssetsAndMalformedTags() {
    assertNull(ClientUpdatePolicy.findRelease { JSONArray("[" +
      release("mobile-assets-test").replace("\"draft\":false", "\"draft\":true") + "," +
      release("mobile-assets-other").replace("android-client.zip", "wrong.zip") + "," +
      release("mobile-assets-../evil") + "]") })
  }

  @Test fun foregroundChecksAreThrottledAndNeverOverlap() {
    val policy = ClientUpdatePolicy()
    assertTrue(policy.begin(0))
    assertFalse(policy.begin(600_000))
    policy.finish()
    assertFalse(policy.begin(10))
    assertTrue(policy.begin(300_000))
  }

  @Test fun discoveryIsBoundedAndUsesPublicationTime() {
    var requests = 0
    val selected = ClientUpdatePolicy.findRelease { page ->
      requests++
      val mobile = release("mobile-assets-$page", if (page == 2) "2026-11-03T01:00:00Z" else "2026-11-03T00:00:00Z")
      JSONArray((listOf(mobile) + (1..99).map { release("v0.1.$it") }).joinToString(",", "[", "]"))
    }
    assertEquals(3, requests)
    assertEquals("mobile-assets-2", selected)
  }
}
