package io.transclip.app

import io.transclip.engine.PeerStatus
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update

/** One line in the activity list. Never contains clipboard content. */
data class HistoryEntry(val time: Long, val text: String)

/** A file transfer in progress, for the Transfers list. */
data class ActiveTransfer(
    val xferId: String,
    val batchId: String,
    val name: String,
    val done: Long,
    val total: Long,
    val incoming: Boolean,
)

/** Shared, observable state of the app (PLAN section 6.1). */
object AppState {
    private val _peers = MutableStateFlow<List<PeerStatus>>(emptyList())
    private val _paired = MutableStateFlow(false)
    private val _running = MutableStateFlow(false)
    private val _paused = MutableStateFlow(false)
    private val _history = MutableStateFlow<List<HistoryEntry>>(emptyList())

    private val _transfers = MutableStateFlow<List<ActiveTransfer>>(emptyList())
    val transfers: StateFlow<List<ActiveTransfer>> = _transfers.asStateFlow()

    fun transferStarted(t: ActiveTransfer) {
        _transfers.update { it.filterNot { x -> x.xferId == t.xferId } + t }
    }

    fun transferProgress(xferId: String, done: Long, total: Long) {
        _transfers.update { list -> list.map { if (it.xferId == xferId) it.copy(done = done, total = total) else it } }
    }

    fun transferEnded(xferId: String) {
        _transfers.update { it.filterNot { x -> x.xferId == xferId } }
    }

    fun clearHistory() {
        _history.value = emptyList()
    }

    val peers: StateFlow<List<PeerStatus>> = _peers.asStateFlow()
    val paired: StateFlow<Boolean> = _paired.asStateFlow()
    val running: StateFlow<Boolean> = _running.asStateFlow()
    val paused: StateFlow<Boolean> = _paused.asStateFlow()
    val history: StateFlow<List<HistoryEntry>> = _history.asStateFlow()

    fun setPeers(list: List<PeerStatus>) {
        _peers.value = list
    }

    fun setPaired(value: Boolean) {
        _paired.value = value
    }

    fun setRunning(value: Boolean) {
        _running.value = value
    }

    fun setPaused(value: Boolean) {
        _paused.value = value
    }

    /** Keeps the newest 100 entries. */
    fun addHistory(text: String) {
        _history.update { (listOf(HistoryEntry(System.currentTimeMillis(), text)) + it).take(100) }
    }
}
