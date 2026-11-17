package io.transclip.proto

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.int
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.long
import java.io.File
import kotlin.test.Test
import kotlin.test.assertContentEquals
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertIs
import kotlin.test.assertTrue

/** Reads the shared JSON files in `docs/test-vectors` produced by the Rust `tvgen` tool. */
class VectorsTest {
    private fun load(name: String): JsonObject {
        val file = File("../../docs/test-vectors/$name")
        check(file.exists()) { "missing $name; run `cargo run -p tvgen` in windows/" }
        return Json.parseToJsonElement(file.readText()).jsonObject
    }

    private fun JsonElement.str() = jsonPrimitive.content
    private fun JsonElement.bytes() = hexToBytes(str())
    private fun JsonObject.arr(key: String): JsonArray = getValue(key).jsonArray

    @Test
    fun frameVectors() {
        val v = load("frames.json")
        for (case in v.arr("valid")) {
            val c = case.jsonObject
            val name = c.getValue("name").str()
            val encoded = c.getValue("encoded_hex").bytes()
            val frames = FrameDecoder().push(encoded)
            assertEquals(1, frames.size, name)
            assertContentEquals(c.getValue("payload_hex").bytes(), frames[0].payload, name)
            if (c.containsKey("unknown")) {
                assertIs<Message.Unknown>(frames[0].message, name)
            } else {
                val expected = wireJson.decodeFromJsonElement(Message.serializer(), c.getValue("header"))
                assertEquals(expected, frames[0].message, name)
                frames[0].message.validate()
                // Re-encoding must decode to the same message (byte order of JSON keys may differ).
                val again = FrameDecoder().push(encodeFrame(frames[0].message, frames[0].payload))
                assertEquals(frames[0].message, again[0].message, name)
                assertContentEquals(frames[0].payload, again[0].payload, name)
            }
        }
        for (case in v.arr("invalid")) {
            val c = case.jsonObject
            val name = c.getValue("name").str()
            val bytes = c.getValue("encoded_hex").bytes()
            when (c.getValue("error").str()) {
                "limit" -> assertFailsWith<ProtoException.Limit>(name) { FrameDecoder().push(bytes) }
                "bad_header" -> assertFailsWith<ProtoException.BadHeader>(name) { FrameDecoder().push(bytes) }
                else -> error("unknown error kind in $name")
            }
        }
    }

    @Test
    fun handshakeVector() {
        val v = load("handshake.json")
        val gk = GroupKey(v.getValue("gk_hex").bytes())
        assertEquals(v.getValue("gid_hex").str(), gk.groupId().toHex())
        val secretI = v.getValue("eph_i_secret_hex").bytes()
        val secretR = v.getValue("eph_r_secret_hex").bytes()
        val ephI = x25519Public(secretI)
        val ephR = x25519Public(secretR)
        assertContentEquals(v.getValue("eph_i_public_hex").bytes(), ephI)
        assertContentEquals(v.getValue("eph_r_public_hex").bytes(), ephR)
        val dhI = x25519Agree(secretI, ephR)!!
        val dhR = x25519Agree(secretR, ephI)!!
        assertContentEquals(dhI, dhR)
        assertContentEquals(v.getValue("dh_hex").bytes(), dhI)
        val keys = deriveKeys(gk, gk.groupId(), ephI, v.getValue("n_i_hex").bytes(), ephR, v.getValue("n_r_hex").bytes(), dhI)
        assertContentEquals(v.getValue("th_hex").bytes(), keys.th)
        assertContentEquals(v.getValue("k_i2r_hex").bytes(), keys.kI2r)
        assertContentEquals(v.getValue("k_r2i_hex").bytes(), keys.kR2i)
        assertContentEquals(v.getValue("mac_r_hex").bytes(), keys.macR)
        assertContentEquals(v.getValue("mac_i_hex").bytes(), keys.macI)
        for (key in listOf("hs1_frame_hex", "hs2_frame_hex", "hs3_frame_hex")) {
            assertEquals(1, FrameDecoder().push(v.getValue(key).bytes()).size, key)
        }
        val hs1 = FrameDecoder().push(v.getValue("hs1_frame_hex").bytes())[0].message
        assertIs<Message.Hs1>(hs1)
        assertEquals(b64urlEncode(ephI), hs1.ephI)
    }

    @Test
    fun recordVectors() {
        val v = load("records.json")
        val key = v.getValue("key_hex").bytes()
        for (case in v.arr("cases")) {
            val c = case.jsonObject
            val counter = c.getValue("counter").jsonPrimitive.long
            val plain = c.getValue("plaintext_hex").bytes()
            val cipher = c.getValue("ciphertext_hex").bytes()
            assertContentEquals(plain, Record.open(key, counter, cipher), "open $counter")
            assertContentEquals(cipher, Record.seal(key, counter, plain), "seal $counter")
            if (counter + 1 < Record.COUNTER_LIMIT) {
                assertFailsWith<ProtoException.Auth> { Record.open(key, counter + 1, cipher) }
            } else {
                assertFailsWith<ProtoException.Limit> { Record.open(key, counter + 1, cipher) }
            }
        }
    }

    @Test
    fun beaconVectors() {
        val v = load("beacon.json")
        for (case in v.arr("valid")) {
            val c = case.jsonObject
            val bytes = c.getValue("encoded_hex").bytes()
            val b = Beacon.decode(bytes)
            assertEquals(c.getValue("gid_hex").str(), b.gid.toHex())
            assertEquals(c.getValue("device_id_hex").str(), b.deviceId.toHex())
            assertEquals(c.getValue("port").jsonPrimitive.int, b.port)
            assertEquals(c.getValue("name").str(), b.name)
            assertEquals(if (c.getValue("platform").str() == "windows") Platform.WINDOWS else Platform.ANDROID, b.platform)
            assertContentEquals(bytes, b.encode())
        }
        for (case in v.arr("invalid")) {
            val c = case.jsonObject
            assertFailsWith<ProtoException>(c.getValue("name").str()) { Beacon.decode(c.getValue("encoded_hex").bytes()) }
        }
    }

    @Test
    fun pairingVectors() {
        val v = load("pairing.json")
        for (case in v.arr("valid")) {
            val c = case.jsonObject
            val uri = c.getValue("uri").str()
            val p = PairingInfo.parse(uri)
            assertContentEquals(c.getValue("gk_hex").bytes(), p.gk.bytes)
            assertEquals(c.getValue("group_name").str(), p.groupName)
            assertEquals(c.getValue("inviter_hex").str(), p.inviter.toHex())
            assertEquals(c.getValue("addrs").jsonArray.map { it.str() }, p.addrs)
            assertEquals(c.getValue("expires").jsonPrimitive.long, p.expires)
            assertEquals(uri, p.toUri())
        }
        for (uri in v.arr("invalid")) {
            assertFailsWith<ProtoException>(uri.str()) { PairingInfo.parse(uri.str()) }
        }
    }

    @Test
    fun pathVectors() {
        val v = load("paths.json")
        for (p in v.arr("valid")) assertTrue(PathRules.isValid(p.str()), "should accept ${p.str()}")
        for (p in v.arr("invalid")) assertFalse(PathRules.isValid(p.str()), "should reject ${p.str()}")
        for (c in v.arr("conflict_names")) {
            val o = c.jsonObject
            assertEquals(o.getValue("result").str(), PathRules.withCounter(o.getValue("name").str(), o.getValue("n").jsonPrimitive.int))
        }
    }
}
