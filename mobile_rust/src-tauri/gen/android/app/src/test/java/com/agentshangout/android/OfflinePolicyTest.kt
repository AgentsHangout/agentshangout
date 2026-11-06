package com.agentshangout.android

import org.junit.Assert.*
import org.junit.Test

class OfflinePolicyTest {
  @Test fun onlyMainFrameNetworkFailureShowsRetryForTrustedOrigin() {
    val base = "https://agentshangout.com"
    val room = "https://agentshangout.com/h/abc?tab=chat"
    assertEquals(room, OfflinePolicy.retryUrl(base, room, true, -2))
    assertNull(OfflinePolicy.retryUrl(base, room, false, -2))
    assertNull(OfflinePolicy.retryUrl(base, room, true, -11))
    assertNull(OfflinePolicy.retryUrl(base, "https://evil.test/h/abc", true, -2))
    assertNull(OfflinePolicy.retryUrl(base, "https://agentshangout.com@evil.test/h/abc", true, -2))
    assertNull(OfflinePolicy.retryUrl(base, "http://agentshangout.com/h/abc", true, -2))
    assertNull(OfflinePolicy.retryUrl(base, "https://agentshangout.com:444/h/abc", true, -2))
    assertNull(OfflinePolicy.retryUrl(base, "javascript:alert(1)", true, -2))
  }
}
