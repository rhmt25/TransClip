# TransClip Wire Protocol — v1

Status: **Normatif**. Implementasi Windows (Rust) dan Android (Kotlin) WAJIB mengikuti dokumen ini persis.
Setiap perubahan pada dokumen ini wajib disertai: (1) perubahan di kedua implementasi, (2) pembaruan test vector di `docs/test-vectors/`, (3) entri di `docs/DECISIONS.md`.

Kata **WAJIB / TIDAK BOLEH / SEBAIKNYA** dipakai dalam arti RFC 2119 (MUST / MUST NOT / SHOULD).

---

## 1. Konvensi

| Hal | Aturan |
|---|---|
| Endianness | Semua integer multi-byte **big-endian** |
| String | UTF-8, tanpa BOM |
| JSON | Objek UTF-8, tanpa komentar. Penerima WAJIB mengabaikan field yang tidak dikenal |
| Base64 | `base64url` **tanpa padding** (RFC 4648 §5) untuk semua nilai biner di JSON |
| `device_id` | 16 byte acak, dibuat sekali saat instalasi, dikodekan sebagai 32 karakter hex huruf kecil |
| `item_id`, `xfer_id`, `batch_id` | UUIDv4 string huruf kecil, format `8-4-4-4-12` |
| Waktu (`ts`) | Unix epoch dalam **milidetik** (integer) |
| Hash konten | SHA-256, hex huruf kecil, 64 karakter |

---

## 2. Transport

| Transport | Parameter |
|---|---|
| TCP (LAN) | Port default **47820** (dapat diubah di setting). Satu port melayani kanal kontrol dan kanal file. `TCP_NODELAY` aktif |
| UDP beacon | Port **47821**, broadcast ke `255.255.255.255` dan alamat broadcast tiap interface |
| mDNS | Service type `_transclip._tcp.local.` |
| Bluetooth RFCOMM | Service UUID **`7b3c2a10-6f1e-4b8e-9d1a-5c0e2f7a4b61`**, nama SDP `TransClip` |

Semua transport stream (TCP, RFCOMM) memakai lapisan yang sama: **Handshake → Record Layer terenkripsi → Frame**.

---

## 3. Grup & Kunci

* **Group Key (`GK`)**: 32 byte acak (CSPRNG). Dibuat oleh perangkat pertama yang membuat grup.
* **Group ID (`GID`)**: `SHA-256("TransClip-v1 group-id" || GK)[0..8]` → 8 byte, ditampilkan sebagai 16 hex. Dipakai di discovery agar perangkat tahu grup peer tanpa membocorkan `GK`.
* String domain di atas ditulis sebagai byte ASCII apa adanya, tanpa terminator.

### 3.1 Payload pairing (QR / kode teks)

```
transclip://pair?v=1&k=<b64url(GK)>&g=<urlencoded group name>&d=<inviter device_id>&a=<ip:port>[,<ip:port>...]&x=<expiry unix seconds>
```

* `a` opsional: alamat IP inviter saat QR dibuat (bantuan discovery).
* `x`: kedaluwarsa tampilan. Pemindai SEBAIKNYA menolak payload kedaluwarsa dengan pesan yang jelas.
* QR dikodekan dengan error correction level **M**.
* "Kode teks" = string URI yang sama persis (untuk copy-paste manual).

---

## 4. Discovery

### 4.1 mDNS
* Instance name: nama perangkat (dipotong 63 byte).
* TXT record: `v=1`, `id=<device_id>`, `gid=<GID hex>`, `pl=<windows|android>`, `nm=<nama perangkat>`.
* Hanya peer dengan `gid` sama yang dihubungi.

### 4.2 UDP beacon (fallback)
Dikirim setiap **10 detik** dan segera saat jaringan berubah. Format biner, total 44 + N byte:

| Offset | Ukuran | Isi |
|---|---|---|
| 0 | 4 | Magic ASCII `TCLB` |
| 4 | 1 | Versi = `0x01` |
| 5 | 8 | `GID` |
| 13 | 16 | `device_id` (biner) |
| 29 | 2 | Port TCP (u16) |
| 31 | 1 | Platform: `0x01` windows, `0x02` android |
| 32 | 12 | Reserved, isi `0x00` |
| 44 | N | Nama perangkat UTF-8, N ≤ 64 |

Paket dengan magic/versi salah, `GID` berbeda, atau `device_id` milik sendiri WAJIB diabaikan. Beacon tidak diautentikasi; keamanan dijamin oleh handshake.

### 4.3 Cache alamat
Setiap perangkat menyimpan `ip:port` terakhir yang berhasil untuk tiap `device_id` dan mencobanya lebih dulu saat start.

---

## 5. Aturan Koneksi

* **Siapa yang dial:** antara dua peer, perangkat dengan `device_id` (dibandingkan sebagai 16 byte, unsigned, lexicographic) **lebih kecil** yang membuka koneksi kontrol.
* **Fallback:** jika peer yang dikenal (terlihat di discovery) belum terhubung dalam **30 detik**, perangkat yang lebih besar BOLEH ikut dial.
* **Duplikat:** jika ada dua koneksi kontrol aktif ke peer yang sama, pertahankan koneksi yang **diinisiasi oleh `device_id` lebih kecil**, tutup yang lain dengan `error {code:"duplicate"}`.
* Satu peer = maksimal satu koneksi kontrol per transport (LAN, BT) + nol atau lebih koneksi file (LAN saja).

---

## 6. Handshake

Pesan handshake dikirim **sebelum** enkripsi aktif, memakai frame plaintext (format §8) tanpa payload. Timeout keseluruhan handshake: **10 detik**.

```
Initiator (I)                                  Responder (R)
  hs1 {v, gid, eph_i, n_i}          ───►
                                    ◄───        hs2 {v, eph_r, n_r, mac_r}
  hs3 {mac_i}                       ───►
  ═══════════ record layer terenkripsi aktif dua arah ═══════════
  hello {...}                       ◄──►        hello {...}
```

| Pesan | Field |
|---|---|
| `hs1` | `v`=1, `gid` (16 hex), `eph_i` (b64url, 32 byte X25519 public), `n_i` (b64url, 16 byte acak) |
| `hs2` | `v`=1, `eph_r`, `n_r`, `mac_r` (b64url, 32 byte) |
| `hs3` | `mac_i` (b64url, 32 byte) |

### 6.1 Derivasi kunci

```
DH   = X25519(eph_priv_self, eph_pub_peer)                       // 32 byte
TH   = SHA-256("TransClip-v1 transcript" || GID || eph_i || n_i || eph_r || n_r)
PRK  = HKDF-Extract(salt = TH, IKM = DH || GK)                   // HKDF-SHA256
OKM  = HKDF-Expand(PRK, info = "TransClip-v1 keys", L = 128)
K_i2r   = OKM[0..32]     // kunci AEAD arah I → R
K_r2i   = OKM[32..64]    // kunci AEAD arah R → I
K_mac_r = OKM[64..96]
K_mac_i = OKM[96..128]
mac_r = HMAC-SHA256(K_mac_r, "R" || TH)
mac_i = HMAC-SHA256(K_mac_i, "I" || TH)
```

* Semua nilai biner dimasukkan sebagai byte mentah (bukan base64).
* Perbandingan MAC WAJIB constant-time.
* R yang menerima `gid` berbeda WAJIB mengirim `error {code:"wrong_group"}` plaintext lalu menutup koneksi.
* MAC salah → tutup koneksi tanpa pesan (jangan bocorkan informasi). Catat di log sebagai `auth_failed`.
* DH bernilai semua-nol (public key low-order) → tolak.
* Kunci ephemeral WAJIB baru per koneksi dan dihapus (zeroize) setelah derivasi.

---

## 7. Record Layer (terenkripsi)

Setelah `hs3` diterima/dikirim, semua byte stream dibungkus record:

| Ukuran | Isi |
|---|---|
| 4 | `L` = panjang ciphertext (u32), `17 ≤ L ≤ 65 552` |
| L | Ciphertext ChaCha20-Poly1305 (plaintext ≤ 65 536 byte + tag 16 byte) |

* Nonce 12 byte = `0x00000000 || counter (u64 big-endian)`. Counter per arah, mulai **0**, naik 1 per record.
* Associated data: kosong.
* Counter mencapai `2^32` → tutup koneksi (praktis tidak tercapai; reconnect membuat kunci baru).
* Gagal dekripsi atau `L` di luar batas → tutup koneksi.
* Plaintext record membentuk **satu aliran byte kontinu**; frame (§8) boleh terpotong di antara record. Pengirim SEBAIKNYA mengisi record sampai 65 536 byte bila ada data, dan WAJIB flush di akhir setiap frame.

---

## 8. Frame

Diadopsi dari protokol Clipdeck, diperluas.

| Ukuran | Isi |
|---|---|
| 4 | `N` = panjang header JSON (u32), `1 ≤ N ≤ 65 536` |
| N | Header JSON. WAJIB memiliki `"t"` (string) dan `"size"` (integer ≥ 0) |
| size | Payload biner (boleh kosong) |

Batas `size` per tipe (lihat §9). Pelanggaran batas → `error {code:"limit"}` lalu tutup koneksi.

Penerima WAJIB **mengabaikan** frame dengan `t` tidak dikenal (setelah membaca dan membuang payload-nya).

---

## 9. Pesan (setelah handshake)

### 9.1 Sesi

| `t` | Arah | Field | Payload | Keterangan |
|---|---|---|---|---|
| `hello` | ↔ | `v`, `id` (device_id), `name`, `pl` (`windows`/`android`), `app` (versi semver), `caps` (array string), `role` (`control`/`file`) | – | Pesan pertama setelah handshake. Peer belum dianggap "connected" sebelum `hello` diterima |
| `ping` | ↔ | – | – | Dikirim jika tidak ada pengiriman selama **30 detik** |
| `pong` | ↔ | – | – | Jawaban `ping` |
| `error` | ↔ | `code`, `msg` (opsional), `ref` (id terkait, opsional) | – | Lihat §11 |
| `bye` | ↔ | `reason` | – | Penutupan sopan (app keluar, pause) |

Koneksi tanpa byte masuk selama **90 detik** dianggap mati.

`caps` v1: `text`, `image`, `file`, `announce`, `rekey`.

### 9.2 Clipboard

| `t` | Field | Payload | Batas payload |
|---|---|---|---|
| `text` | `id` (item_id), `origin` (device_id), `ts`, `hash` | UTF-8 | 8 MiB |
| `image` | `id`, `origin`, `ts`, `hash`, `mime`, `w`, `h` | Byte gambar asli | 50 MiB |
| `announce` | `id`, `origin`, `ts`, `hash`, `kind` (`image`), `mime`, `w`, `h`, `bytes` (ukuran) | – | – |
| `pull` | `id` | – | – |
| `ack` | `id`, `ok` (bool), `reason` (opsional) | – | – |

Aturan:
* `hash` = SHA-256 dari payload (untuk `text`: byte UTF-8).
* `mime` yang didukung: `image/png` (utama), `image/jpeg`, `image/webp`, `image/gif`, `image/bmp`. Pengirim **TIDAK BOLEH** mengonversi ulang format lossy (JPEG/WebP) — kirim byte asli. Konversi hanya boleh dari format mentah (DIB) ke PNG.
* Penerima WAJIB membalas `ack` setelah item ditulis ke clipboard (atau gagal, `ok:false`).
* Penerima WAJIB mengabaikan (tetapi tetap `ack ok:true`) item yang `id`-nya sudah pernah dilihat (cache LRU 1024 id / 10 menit), dan item yang `hash`-nya sama dengan isi clipboard saat ini.
* **Konflik:** item diterapkan hanya jika `ts` > `ts` item clipboard terakhir yang diterapkan; jika `ts` sama, menang `origin` yang lebih kecil.
* `announce` dipakai bila item terlalu besar untuk transport saat ini (lihat PLAN §Transport). Penerima mengirim `pull {id}` untuk mendapatkan frame `image` lengkap. Pengirim menyimpan item yang di-announce minimal **10 menit**; `pull` untuk id yang sudah hilang dijawab `error {code:"gone", ref:id}`.
* Tidak ada relay: penerima **TIDAK BOLEH** meneruskan item ke peer lain.

### 9.3 Transfer file

Alur LAN:

```
Pengirim (kanal kontrol)            Penerima
  file_offer {...}            ───►
                              ◄───   file_accept {xfer_id, offset}
  [buka koneksi TCP baru, handshake, hello role=file]
  file_stream {xfer_id}       ───►   (di koneksi file)
  file_chunk ... file_chunk   ───►
  file_end {xfer_id}          ───►
                              ◄───   file_result {xfer_id, ok, reason}   (di kanal kontrol)
```

Alur Bluetooth: sama, tetapi `file_stream` / `file_chunk` / `file_end` dikirim di kanal kontrol RFCOMM yang sama, diselingi frame kontrol (frame kontrol didahulukan di antara chunk).

| `t` | Field | Payload | Keterangan |
|---|---|---|---|
| `file_offer` | `xfer_id`, `batch_id`, `batch_name`, `batch_count`, `batch_bytes`, `kind` (`file`/`dir`), `relative_path`, `bytes` (ukuran file; BUKAN `size`), `sha256`, `mtime` | – | Satu per entri. `relative_path` memakai `/` sebagai pemisah, relatif terhadap akar batch |
| `file_accept` | `xfer_id`, `offset` | – | `offset` > 0 berarti lanjut (resume) |
| `file_reject` | `xfer_id`, `reason` (`quota`, `path`, `disk_full`, `disabled`) | – | |
| `file_stream` | `xfer_id`, `offset` | – | Pembuka aliran |
| `file_chunk` | `xfer_id`, `offset` | ≤ 64 KiB (LAN), ≤ 8 KiB (BT) | |
| `file_end` | `xfer_id` | – | |
| `file_result` | `xfer_id`, `ok`, `reason` (`hash`, `io`, `cancelled`) | – | |
| `file_cancel` | `xfer_id`, `reason` | – | Dari sisi mana pun |
| `batch_done` | `batch_id`, `ok_count`, `fail_count` | – | Dari penerima setelah seluruh batch |

Aturan:
* Entri `dir` tidak punya stream; penerima langsung membuat foldernya lalu mengirim `file_result ok:true`.
* Penerima menulis ke `<nama>.transclip-part`, me-rename setelah hash cocok. Bila nama tujuan sudah ada → tambah ` (1)`, ` (2)`, ….
* Resume: bila koneksi putus, pengirim mengulang `file_offer` dengan `xfer_id` sama; penerima menjawab `file_accept` dengan `offset` = ukuran file `.transclip-part` yang ada.
* Hash diverifikasi atas **seluruh** file. Hash salah → hapus file part, `file_result ok:false reason:"hash"`, pengirim mencoba ulang **sekali** dari offset 0.
* Validasi `relative_path` (WAJIB, di penerima): tolak jika: kosong; diawali `/` (absolut); ada segmen kosong (`a//b`); ada segmen `.` atau `..`; mengandung `\`, `:`, `<`, `>`, `"`, `|`, `?`, `*` atau karakter kontrol (U+0000–U+001F, U+007F); ada segmen yang diakhiri titik atau spasi; ada segmen dengan nama reserved Windows (`CON`, `PRN`, `AUX`, `NUL`, `COM1`–`COM9`, `LPT1`–`LPT9`; case-insensitive; bagian sebelum titik pertama, dengan atau tanpa ekstensi); panjang segmen > 255 byte UTF-8; atau panjang total > 4096 byte UTF-8. Penolakan → `file_reject reason:"path"`.

### 9.4 Rotasi kunci grup

| `t` | Field | Payload |
|---|---|---|
| `rekey` | `new_gk` (b64url 32 byte), `name`, `by` (device_id), `ts` | – |
| `rekey_ack` | `ts` | – |

* Dikirim hanya lewat kanal terenkripsi ke peer yang **tidak** sedang dicabut.
* Penerima menyimpan `new_gk`, membalas `rekey_ack`, lalu menutup semua koneksi dan reconnect dengan kunci baru.
* Peer yang offline saat rekey harus di-pair ulang.

---

## 10. Batas & Timeout (ringkasan)

| Item | Nilai |
|---|---|
| Header JSON | 1 B – 64 KiB |
| Record plaintext | ≤ 64 KiB |
| Teks clipboard | ≤ 8 MiB |
| Gambar clipboard | ≤ 50 MiB |
| Chunk file LAN / BT | 64 KiB / 8 KiB |
| Handshake timeout | 10 s |
| Ping interval / dead timeout | 30 s / 90 s |
| Ack timeout | 15 s, retry 1×, lalu tandai gagal |
| Cache item id | 1024 entri / 10 menit |
| Antrian offline per peer | 5 item clipboard (bukan file, bukan sensitif) |
| Backoff reconnect | 2 s × 2, maks 30 s, reset saat sukses |
| Beacon interval | 10 s |

---

## 11. Kode error

| `code` | Arti | Aksi penerima |
|---|---|---|
| `wrong_group` | GID tidak cocok | Jangan retry ke alamat ini sampai discovery berubah |
| `version` | Versi tidak didukung | Tampilkan "perbarui aplikasi" |
| `limit` | Batas ukuran dilanggar | Tutup koneksi |
| `duplicate` | Koneksi ganda | Tutup koneksi ini, pakai yang lain |
| `gone` | Item `pull` sudah tidak ada | Hapus announce dari UI |
| `busy` | Terlalu banyak transfer / rate limit | Retry setelah 5 s |
| `internal` | Kesalahan lain | Log |

---

## 12. Test vector

Folder `docs/test-vectors/` WAJIB berisi file JSON yang dihasilkan oleh `cargo run -p tvgen` (Windows workspace):

* `frames.json` — daftar `{name, header, payload_hex, encoded_hex}`.
* `handshake.json` — input tetap (GK, eph private keys, nonces) dan output (`GID`, `TH`, `K_i2r`, `K_r2i`, `mac_r`, `mac_i`).
* `records.json` — kunci, counter, plaintext, ciphertext.
* `beacon.json` — field dan byte hasil.
* `paths.json` — daftar `relative_path` beserta hasil `valid: true/false`.

Kedua implementasi WAJIB punya unit test yang membaca file ini dan mencocokkan hasil byte-per-byte.
