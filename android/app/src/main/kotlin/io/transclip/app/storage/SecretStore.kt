package io.transclip.app.storage

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import androidx.datastore.core.DataStore
import androidx.datastore.preferences.core.Preferences
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.stringPreferencesKey
import androidx.datastore.preferences.preferencesDataStore
import io.transclip.proto.DeviceId
import io.transclip.proto.GroupKey
import io.transclip.proto.hexToBytes
import io.transclip.proto.toHex
import kotlinx.coroutines.flow.first
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

private val Context.secretsStore: DataStore<Preferences> by preferencesDataStore(name = "secrets")

/** Wraps small secrets with a key that never leaves the Android Keystore. */
interface KeyWrapper {
    fun wrap(plain: ByteArray): ByteArray

    /** Throws when the data was tampered with or the key is gone. */
    fun unwrap(wrapped: ByteArray): ByteArray
}

/** AES-256-GCM with a Keystore-resident key; the wire format is `iv(12) || ciphertext+tag`. */
class AesGcmWrapper(private val keyProvider: () -> SecretKey) : KeyWrapper {
    override fun wrap(plain: ByteArray): ByteArray {
        val cipher = Cipher.getInstance(TRANSFORM)
        cipher.init(Cipher.ENCRYPT_MODE, keyProvider())
        return cipher.iv + cipher.doFinal(plain)
    }

    override fun unwrap(wrapped: ByteArray): ByteArray {
        require(wrapped.size > IV_LEN + 16) { "wrapped secret too short" }
        val cipher = Cipher.getInstance(TRANSFORM)
        cipher.init(Cipher.DECRYPT_MODE, keyProvider(), GCMParameterSpec(128, wrapped, 0, IV_LEN))
        return cipher.doFinal(wrapped, IV_LEN, wrapped.size - IV_LEN)
    }

    companion object {
        private const val TRANSFORM = "AES/GCM/NoPadding"
        private const val IV_LEN = 12

        /** The production wrapper: key alias `transclip_gk_wrap` in the Android Keystore. */
        fun keystore(alias: String = "transclip_gk_wrap"): AesGcmWrapper = AesGcmWrapper {
            val ks = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
            (ks.getKey(alias, null) as? SecretKey) ?: KeyGenerator
                .getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
                .apply {
                    init(
                        KeyGenParameterSpec.Builder(
                            alias,
                            KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
                        )
                            .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                            .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                            .setKeySize(256)
                            .build(),
                    )
                }
                .generateKey()
        }
    }
}

/**
 * Device id (created once) and group key (wrapped by the Keystore key). The ciphertext lives in
 * a DataStore file; the plain group key exists only in memory.
 */
class SecretStore(
    private val store: DataStore<Preferences>,
    private val wrapper: KeyWrapper,
) {
    constructor(context: Context) : this(context.applicationContext.secretsStore, AesGcmWrapper.keystore())

    /** The stable id of this device; generated on first use. */
    suspend fun deviceId(): DeviceId {
        store.data.first()[DEVICE_ID]?.let { return DeviceId.fromHex(it) }
        val fresh = DeviceId.random()
        var result = fresh
        store.edit { prefs ->
            // Another caller may have created one in the meantime: keep the first.
            val existing = prefs[DEVICE_ID]
            if (existing != null) result = DeviceId.fromHex(existing) else prefs[DEVICE_ID] = fresh.toHex()
        }
        return result
    }

    /** The group key, or null when this device is not paired (or the secret cannot be read). */
    suspend fun groupKey(): GroupKey? {
        val b64 = store.data.first()[GROUP_KEY] ?: return null
        return try {
            GroupKey(wrapper.unwrap(Base64.decode(b64, Base64.NO_WRAP)))
        } catch (e: Exception) {
            null
        }
    }

    suspend fun setGroupKey(key: GroupKey?) {
        store.edit { prefs ->
            if (key == null) {
                prefs.remove(GROUP_KEY)
            } else {
                prefs[GROUP_KEY] = Base64.encodeToString(wrapper.wrap(key.bytes), Base64.NO_WRAP)
            }
        }
    }

    private companion object {
        val DEVICE_ID = stringPreferencesKey("device_id")
        val GROUP_KEY = stringPreferencesKey("group_key_wrapped")
    }
}

/** Convenience used by tests and the pairing flow. */
fun GroupKey.toHexString(): String = bytes.toHex()

fun groupKeyFromHex(hex: String): GroupKey = GroupKey(hexToBytes(hex, 32))
