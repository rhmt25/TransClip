package io.transclip.engine

import kotlin.test.Test
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class RateLimitTest {
    @Test
    fun twentyPassThenBlockedUntilTheWindowSlides() {
        val r = PeerRate()
        repeat(PeerRate.MAX_ITEMS) { assertTrue(r.allow(deviceId(1), 0)) }
        assertFalse(r.allow(deviceId(1), 5_000))
        assertTrue(r.allow(deviceId(1), PeerRate.WINDOW_MS))
    }

    @Test
    fun peersAreIndependent() {
        val r = PeerRate()
        repeat(PeerRate.MAX_ITEMS) { r.allow(deviceId(1), 0) }
        assertTrue(r.allow(deviceId(2), 0))
    }
}
