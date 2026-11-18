package io.transclip.engine

import io.transclip.proto.DeviceId

/** Per-peer sliding window: at most [MAX_ITEMS] clipboard items per [WINDOW_MS] (PLAN section 7). */
class PeerRate {
    private val hits = HashMap<DeviceId, ArrayDeque<Long>>()

    @Synchronized
    fun allow(peer: DeviceId, nowMs: Long): Boolean {
        val q = hits.getOrPut(peer) { ArrayDeque() }
        while (q.isNotEmpty() && nowMs - q.first() >= WINDOW_MS) q.removeFirst()
        if (q.size >= MAX_ITEMS) return false
        q.addLast(nowMs)
        return true
    }

    companion object {
        const val MAX_ITEMS = 20
        const val WINDOW_MS = 10_000L
    }
}
