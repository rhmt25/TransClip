package io.transclip.app.storage

import android.content.Context
import androidx.datastore.preferences.core.PreferenceDataStoreFactory
import androidx.test.platform.app.InstrumentationRegistry
import io.transclip.proto.GroupKey
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.runBlocking
import org.junit.Test
import java.io.File
import java.util.UUID
import kotlin.test.assertContentEquals
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertNotEquals
import kotlin.test.assertNull

/** Runs on a device or emulator because it needs the real Android Keystore. */
class SecretStoreTest {
    private val context: Context get() = InstrumentationRegistry.getInstrumentation().targetContext

    private fun store(name: String, alias: String): SecretStore {
        val file = File(context.cacheDir, "$name-${UUID.randomUUID()}.preferences_pb")
        val ds = PreferenceDataStoreFactory.create(produceFile = { file })
        return SecretStore(ds, AesGcmWrapper.keystore(alias))
    }

    @Test
    fun deviceIdIsCreatedOnceAndStaysTheSameAcrossInstances() = runBlocking {
        val file = File(context.cacheDir, "ids-${UUID.randomUUID()}.preferences_pb")
        val scope1 = CoroutineScope(SupervisorJob() + Dispatchers.IO)
        val first = SecretStore(
            PreferenceDataStoreFactory.create(scope = scope1, produceFile = { file }),
            AesGcmWrapper.keystore("tc_test_a"),
        )
        val id = first.deviceId()
        assertEquals(id, first.deviceId())
        // An app restart: the old DataStore is gone, a new one opens the same file.
        scope1.cancel()
        val scope2 = CoroutineScope(SupervisorJob() + Dispatchers.IO)
        val second = SecretStore(
            PreferenceDataStoreFactory.create(scope = scope2, produceFile = { file }),
            AesGcmWrapper.keystore("tc_test_a"),
        )
        assertEquals(id, second.deviceId())
        scope2.cancel()
    }

    @Test
    fun groupKeyRoundTripsAndIsNotStoredInPlain() = runBlocking {
        val file = File(context.cacheDir, "gk-${UUID.randomUUID()}.preferences_pb")
        val ds = PreferenceDataStoreFactory.create(produceFile = { file })
        val s = SecretStore(ds, AesGcmWrapper.keystore("tc_test_b"))
        assertNull(s.groupKey())
        val key = GroupKey(ByteArray(32) { (it * 7 + 1).toByte() })
        s.setGroupKey(key)
        assertContentEquals(key.bytes, s.groupKey()?.bytes)
        // The key bytes must not appear in the file.
        val raw = file.readBytes()
        val needle = key.bytes
        val found = (0..raw.size - needle.size).any { i -> needle.indices.all { raw[i + it] == needle[it] } }
        assertEquals(false, found)
        s.setGroupKey(null)
        assertNull(s.groupKey())
    }

    @Test
    fun wrappingTwiceGivesDifferentCiphertextAndTamperingIsDetected() {
        val w = AesGcmWrapper.keystore("tc_test_c")
        val plain = ByteArray(32) { it.toByte() }
        val a = w.wrap(plain)
        val b = w.wrap(plain)
        assertNotEquals(a.toList(), b.toList())
        assertContentEquals(plain, w.unwrap(a))
        a[a.size - 1] = (a[a.size - 1].toInt() xor 1).toByte()
        assertFailsWith<Exception> { w.unwrap(a) }
    }

    @Test
    fun unreadableSecretIsTreatedAsNotPaired() = runBlocking {
        val s = store("bad", "tc_test_d")
        s.setGroupKey(GroupKey(ByteArray(32) { 5 }))
        // A different Keystore key cannot decrypt it.
        val file = File(context.cacheDir, "bad-x.preferences_pb")
        assertNull(SecretStore(PreferenceDataStoreFactory.create(produceFile = { file }), AesGcmWrapper.keystore("tc_test_e")).groupKey())
    }
}
