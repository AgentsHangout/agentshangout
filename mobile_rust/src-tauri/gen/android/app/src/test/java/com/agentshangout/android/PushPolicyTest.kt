package com.agentshangout.android

import org.junit.Assert.*
import org.junit.Test

class PushPolicyTest {
    @Test fun defaultOffAndOwnerPermissionRequired() {
        assertFalse(PushPolicy(ownerId = "person-1").accepts("mention", "person-1", true))
        val enabled = PushPolicy(enabled = true, ownerId = "person-1")
        assertTrue(enabled.accepts("mention", "person-1", true))
        assertFalse(enabled.accepts("mention", "person-2", true))
        assertFalse(enabled.accepts("mention", null, true))
        assertFalse(enabled.accepts("mention", "person-1", false))
        assertFalse(enabled.accepts("unknown", "person-1", true))
    }
    @Test fun categoriesAreIndependentAndTestStillNeedsMaster() {
        val p = PushPolicy(true, false, true, false, "one")
        assertFalse(p.accepts("mention", "one", true))
        assertTrue(p.accepts("request", "one", true))
        assertFalse(p.accepts("announcement", "one", true))
        assertTrue(p.accepts("test", "one", true))
        assertFalse(p.copy(enabled = false).accepts("test", "one", true))
    }
    @Test fun notificationRoutesCannotNavigateOutsideRooms() {
        assertEquals("/", PushPolicy.safePath("/"))
        assertEquals("/h/room_123-a", PushPolicy.safePath("/h/room_123-a"))
        listOf("https://evil.test", "//evil.test", "/h/../admin", "/h/%2fadmin", "/h/x?next=evil",
            "/h/x#foo", "/api/session", "/h/", "/h/" + "x".repeat(129)).forEach {
            assertNull(it, PushPolicy.safePath(it))
        }
    }
    @Test fun retryIdsAreBoundedAndRemainUnique() {
        val previous = (1..120).map { "event-$it" }
        val next = PushPolicy.recent(previous, "event-110")
        assertEquals(100, next.size)
        assertEquals(100, next.toSet().size)
        assertEquals("event-110", next.last())
        assertFalse(next.contains("event-1"))
    }
}
