# Blueprint Proyek: Standalone Multi-Device Lossless Clipboard & File Sync

Target platform: **Windows 11** dan **Android** (beberapa perangkat). Linux tidak termasuk cakupan.

Beberapa bagian terinspirasi dari repo referensi **Clipdeck** (clipboard sync GNOME ↔ Android via Bluetooth); bagian tersebut ditandai *[Clipdeck]*.

---

## 1. Ringkasan & Tujuan

Ekosistem aplikasi desktop dan mobile yang **100% mandiri** (tanpa aplikasi pihak ketiga), ringan, dan berjalan otomatis (*zero-interaction*) untuk menyinkronkan clipboard (teks & gambar/screenshot) serta mengirim file/folder secara instan antar banyak perangkat Windows 11 dan Android.

### Prinsip Desain
1. **True Standalone:** 1 file `.exe` di Windows dan 1 file `.apk` di Android. Tanpa Shizuku, Termux, ADB, atau runtime terpisah.
2. **Full P2P Mesh:** setiap perangkat simetris (server + client). Jika PC mati, antar-Android tetap terhubung.
3. **Lossless:** gambar dan file identik secara piksel/byte, tanpa penurunan kualitas.
   * File/folder dikirim byte-per-byte, tanpa zip.
   * Screenshot dikirim sebagai **PNG** (lossless). `CF_DIB` mentah 4K sekitar 33 MB, sedangkan PNG lossless sekitar 2–4 MB. Di Windows, tulis format `PNG` dan `CF_DIBV5` sekaligus agar kompatibel dengan aplikasi lama.
4. **Seamless UX:** clipboard otomatis masuk ke riwayat `Win + V` dan Gboard; transfer file di Windows lewat **Radial Drop Menu**, tanpa klik *Accept*.
5. **Privasi by default:** data sensitif (password manager) tidak pernah disinkronkan.

---

## 2. Arsitektur Jaringan

### 2.1 Dual-Transport

| Jalur | Teknologi | Peran |
| :--- | :--- | :--- |
| **Wi-Fi LAN (utama)** | TCP + mDNS | Screenshot, file/folder besar, broadcast clipboard multi-device |
| **Bluetooth Classic (cadangan)** | RFCOMM (SPP) | Teks instan dan file kecil saat Wi-Fi putus atau ada AP Isolation (mode *Priority/Lazy*) |

### 2.2 Discovery & Koneksi Mesh
* **Discovery berlapis:** (1) mDNS, (2) UDP broadcast beacon, (3) cache IP terakhir, (4) daftar IP di dalam QR pairing, (5) input IP manual. `NsdManager` Android kurang stabil dan butuh `WifiManager.MulticastLock`.
* **Aturan dial deterministik:** node dengan `device_id` lebih kecil yang melakukan dial, yang besar hanya listen. Mencegah koneksi ganda saat dua node saling dial. Berlaku untuk LAN dan Bluetooth.
* **Tanpa relay:** pengirim asal mengirim langsung ke tiap peer (full mesh ≤10 node). `Message_UUID` cache hanya sebagai pengaman, bukan mekanisme utama.
* **Reconnect [Clipdeck]:** backoff eksponensial 2 s → 30 s, reset setelah sukses, berhenti hanya jika pengguna menekan Disconnect. UI menampilkan alasan kegagalan (putus oleh peer, Bluetooth mati, izin kurang).
* **Antrian offline [Clipdeck]:** hingga 5 item teks/gambar kecil per peer disimpan saat peer belum terhubung dan dikirim setelah `hello`. Item sensitif dan file besar tidak diantrikan.

### 2.3 Framing Protokol [Clipdeck, dimodifikasi]

```text
[4 byte, big-endian] panjang header N
[N byte, UTF-8 JSON] header: {"t":"...", "size":<payload>, ...}
[size byte]          payload (opsional)
```

* **Batas keras:** header 1 B – 64 KiB; payload clipboard maksimal 50 MiB. Di luar batas → koneksi ditutup.
* **Tipe pesan tidak dikenal diabaikan**, sehingga versi baru dapat menambah pesan tanpa merusak versi lama.
* `hello` dikirim pertama oleh kedua sisi: `v` (versi), `name`, `device_id`, `caps`.
* `ack {id}` untuk tiap item yang diterima; `ping`/`pong` setiap 30 detik sebagai keep-alive.

| `t` | Field | Payload | Keterangan |
| :--- | :--- | :--- | :--- |
| `hello` | `v`, `name`, `device_id`, `caps` | – | Pertama kali, dua arah |
| `text` | `id`, `ts`, `hash` | UTF-8 | Clipboard teks |
| `image` | `id`, `ts`, `mime`, `hash`, `w`, `h` | PNG | Clipboard gambar |
| `announce` / `pull` | `id`, `kind`, `size`, `hash` | – | Item besar ditarik sesuai kebutuhan |
| `file_offer` | `xfer_id`, `name`, `relative_path`, `size`, `sha256`, `mtime` | – | Awal transfer file |
| `file_chunk` | `xfer_id`, `offset` | ≤64 KB | Data file |
| `file_done` / `file_cancel` | `xfer_id` | – | Akhir / batal |
| `ack`, `ping`, `pong` | – | – | Kontrol |

**Interop test [Clipdeck]:** buat *golden test vectors* (frame hex) di dokumentasi dan wajibkan setiap implementasi (Kotlin dan Windows) lolos vektor yang sama.

### 2.4 Kanal & Transfer File
* **LAN:** 1 koneksi TCP kontrol permanen + koneksi TCP baru per transfer file (diawali `file_offer`). Lebih sederhana dari multiplexing manual dan tidak ada head-of-line blocking.
* **Bluetooth:** hanya 1 link, sehingga multiplexing dengan prioritas: frame kontrol menyela di antara chunk file. Chunk 64 KB di LAN, 8 KB di Bluetooth.
* **Resume & back-pressure:** `file_chunk` membawa `offset`; transfer dapat dilanjutkan setelah putus. SHA-256 dihitung streaming (tanpa buffer seluruh file di RAM).
* **Announce lalu pull:** gambar besar (terutama lewat Bluetooth, ~100–200 KB/s, 3 MB ≈ 15–30 detik) diumumkan dulu dengan ukuran + hash, lalu ditarik saat dibutuhkan. Di LAN ditarik otomatis; di Bluetooth bersifat *lazy*. Di Windows ini dapat dipetakan ke *delayed rendering* (`WM_RENDERFORMAT`); uji dulu apakah `Win + V` tetap menampilkan itemnya.

---

## 3. Keamanan

* **Master Group Key (pairing O(1)):** satu kunci grup 256-bit dibuat sekali, diekspor sebagai **QR Code**. Perangkat baru memindai sekali untuk bergabung. QR hanya ditampilkan saat diminta, berlaku singkat (mis. 2 menit), dan tidak disimpan sebagai gambar.
* **Handshake dua arah + enkripsi per sesi:** HMAC saja hanya membuktikan kepemilikan kunci; data LAN setelahnya tetap plaintext dan rawan replay. Gunakan nonce dari kedua sisi, turunkan session key (`HKDF(group_key, nonce_A || nonce_B)`) atau pola **Noise** dengan PSK (mis. `Noise_NNpsk0`), lalu AEAD (ChaCha20-Poly1305 / AES-256-GCM) per frame. Pakai library mapan (libsodium / Noise), jangan merakit kripto sendiri.
* **Bluetooth [Clipdeck]:** hanya perangkat yang sudah *paired* (bonded) yang boleh terhubung; link dienkripsi Bluetooth. Handshake Group Key yang sama tetap dijalankan di atas RFCOMM agar model keamanan seragam.
* **Zero-prompt tetap**, dengan batas aman: kuota ukuran, rate-limit per peer, dan notifikasi kecil saat file diterima (bukan dialog Accept).
* **Sanitasi `relative_path` oleh penerima:** tolak `..`, path absolut, drive letter, nama terlarang Windows (`CON`, `NUL`, `:` dst.), dan benturan nama beda huruf besar-kecil. Tulis hanya di dalam folder tujuan. Pengirim hanya mengirim file yang dipilih pengguna secara eksplisit.
* **Revoke (v1.1):** keypair per perangkat dan daftar perangkat tepercaya; minimal tombol **Ganti Group Key** di Settings.

---

## 4. Privasi & Konsistensi Clipboard

Clipdeck tidak menyimpan entri yang ditandai password manager. Terapkan di kedua platform:

| Platform | Sinyal "ini rahasia" yang dihormati |
| :--- | :--- |
| Windows | Format `ExcludeClipboardContentFromMonitorProcessing`, `CanIncludeInClipboardHistory` = 0, `CanUploadToCloudClipboard` = 0 |
| Android | `ClipDescription.EXTRA_IS_SENSITIVE` (API 33+) |

* Item sensitif tidak dikirim dan tidak masuk antrian offline.
* Saat menulis clipboard dari peer di Windows, pertimbangkan `CanUploadToCloudClipboard` = 0 agar tidak bocor ke cloud clipboard Microsoft.
* **Cegah echo/loop:** menulis clipboard dari peer memicu `WM_CLIPBOARDUPDATE` / `OnPrimaryClipChanged` lagi. Tandai penulisan internal (flag + hash terakhir) dan abaikan event itu.
* **Deduplikasi** berdasarkan SHA-256 isi [Clipdeck: item sama dipindah ke atas, tidak digandakan], selain `Message_UUID`.
* **Konflik:** *last-writer-wins* memakai `ts`, dengan `device_id` sebagai tie-breaker.
* **Pengaturan:** *Pause sync*, mode hanya-teks, daftar aplikasi dikecualikan (opsional), ukuran riwayat, pembersihan cache gambar otomatis.

---

## 5. Implementasi Platform

### A. Windows 11 (Native Standalone `.exe`)

* **Teknologi:** .NET Native AOT **tidak mendukung WinForms/WPF**, jadi overlay dan Settings tidak bisa AOT dengan framework itu. Pilihan:
  1. **Rust** (`windows-rs`, layered window Win32) → `.exe` kecil, RAM ±15–25 MB. *Direkomendasikan.*
  2. **C# .NET 8 single-file self-contained (non-AOT, trimmed)** → disk ±60–80 MB, RAM ±40–60 MB, produktivitas tinggi.
  3. Python + Nuitka → cepat dibuat, tetapi lebih besar dan lebih berat.
  Target RAM 15–25 MB hanya realistis untuk pilihan 1. Berjalan *resident* di System Tray.
* **Clipboard engine:** `AddClipboardFormatListener` + `WM_CLIPBOARDUPDATE`. Coba ulang `OpenClipboard` beberapa kali (aplikasi lain sering menahannya), pakai `GetClipboardSequenceNumber` untuk menghindari event ganda, dan hormati format rahasia (bagian 4). Teks (`CF_UNICODETEXT`) dan gambar dibaca/ditulis di memori (zero-disk I/O). Untuk `Win + V`, pengguna harus mengaktifkan *Clipboard history* di Windows.
* **Bluetooth:** RFCOMM lewat WinRT (`RfcommServiceProvider` / `StreamSocket`) dengan UUID layanan kustom.
* **Dynamic Radial Drop Menu** (terinspirasi *KnifeLemon/Filee*):
  * Hook global (`SetWindowsHookEx`) saja tidak cukup untuk mengetahui file sedang di-drag dari Explorer. Pendekatan yang lebih andal: saat tombol pemicu ditekan dan tombol mouse tertahan, tampilkan overlay *layered* transparan di kursor yang terdaftar sebagai **OLE `IDropTarget`**, sehingga tiap segmen menerima `HDROP` secara native.
  * Sudut segmen 360°/N sesuai perangkat aktif, ditambah segmen **"Kirim ke Semua"**. File terkirim saat drop dilepas di segmen tujuan.
  * **Batasan UIPI:** drag dari aplikasi yang berjalan sebagai Administrator tidak bisa diterima overlay biasa. Dokumentasikan sebagai keterbatasan.
  * **Cadangan:** menu konteks Explorer / folder `SendTo` dan hotkey kirim file terpilih.
  * Maksimal 6–8 segmen; di atasnya dikelompokkan per kategori.
* **Settings UI:** dimuat hanya saat dibuka dari Tray. Isi: Master Group Key (QR), status koneksi *live* per perangkat (Connected / Waiting / Error) [Clipdeck], daftar perangkat dan tombol hapus, prioritas Bluetooth, tombol pemicu radial menu, folder tujuan default, *Pause sync*, ganti Group Key.

### B. Android (Standalone `.apk`, tanpa Shizuku/Root)

* **Teknologi:** Kotlin dengan Foreground Service. UI Jetpack Compose, state tunggal via `StateFlow` [Clipdeck].
* **Foreground Service [Clipdeck]:**
  * `foregroundServiceType="connectedDevice"` untuk LAN dan Bluetooth (prasyarat Android 14+ terpenuhi lewat `CHANGE_WIFI_MULTICAST_STATE` / `BLUETOOTH_CONNECT`; `dataSync` tidak dipakai karena dibatasi waktunya di Android 15). Uji Doze dan optimasi baterai.
  * Notifikasi ongoing berprioritas rendah dengan teks status dan aksi **Disconnect**.
  * Izin Bluetooth: `BLUETOOTH_CONNECT` (API 31+) dan `BLUETOOTH` (≤30). **Jangan memanggil `cancelDiscovery()`** bila tidak scan: di Android 12+ itu butuh `BLUETOOTH_SCAN` dan membuat setiap koneksi gagal dengan `SecurityException`.
* **Menulis clipboard (menerima dari peer) [Clipdeck]:**
  * Teks: `setPrimaryClip(ClipData.newPlainText(...))` dari service.
  * Gambar: simpan ke `Pictures/<NamaApp>` via **`MediaStore`** (`IS_PENDING=1` → tulis → `IS_PENDING=0`), lalu `ClipData.newUri(...)`. Di Android 10+ tidak butuh izin storage; hapus entri jika penulisan gagal.
  * Cara ini sudah cukup tanpa overlay/Accessibility. Verifikasi di perangkat target bahwa Gboard menampilkan gambar dari `ClipData.newUri`.
* **Membaca clipboard (mengirim ke peer):** Android 10+ memblokir aplikasi latar belakang membaca clipboard. Trik **Accessibility Service + overlay 1×1 piksel** (`TYPE_APPLICATION_OVERLAY`, `FLAG_NOT_TOUCHABLE`, `< 50 ms`) hanya dibutuhkan untuk arah ini. Risikonya: kebijakan Google Play soal Accessibility dan OEM yang mematikan overlay. Jadikan auto-capture **opsional**, dengan jalur tanpa trik: Share Sheet, Quick Settings Tile "Kirim clipboard", aksi di notifikasi, widget.
* **Transfer file & folder:**
  * **Share Sheet** (`ACTION_SEND` / `ACTION_SEND_MULTIPLE`) dengan pilihan satu atau banyak perangkat, plus *Direct Share* untuk perangkat yang sering dipakai.
  * Rekonstruksi hierarki folder dari `relative_path` tanpa ekstraksi arsip, memakai Storage Access Framework (`ACTION_OPEN_DOCUMENT_TREE`) dan `DocumentFile`, dengan sanitasi path (bagian 3).

---

## 6. Rekomendasi Batas Perangkat

| Komponen | Batas teknis | **Rekomendasi** | Alasan |
| :--- | :--- | :--- | :--- |
| Wi-Fi LAN Mesh | 50+ | **8–10 perangkat** | Broadcast screenshot 2–4 MB ke >10 perangkat membebani airtime router |
| Bluetooth RFCOMM | 7 (piconet) | **2–3 perangkat aktif** | Bandwidth ~200 KB/s dibagi rata; mode Priority/Lazy |
| Radial Drop Menu | tak terbatas | **6–8 segmen** | Di bawah 45° rawan salah drop; lebih dari 8 dikelompokkan |

**Konfigurasi ideal:** 5–6 perangkat (1 PC Windows 11 + 4–5 Android), 2–3 di antaranya prioritas Bluetooth. Untuk gambar >1 MB, kirim bergiliran (bukan paralel penuh) agar airtime stabil.

---

## 7. Roadmap

0. **Fase 0 — Spike risiko (1–2 minggu)**
   * Android: tulis gambar via `MediaStore` + `ClipData.newUri`, cek tampil di Gboard.
   * Android: uji baca clipboard latar belakang (overlay/Accessibility) di 2–3 perangkat/OEM.
   * Windows: prototipe overlay `IDropTarget` yang menerima drag dari Explorer.
   * Putuskan tumpukan teknologi Windows (Rust vs C# single-file).
1. **Fase 1 — Protokol & Core Networking**
   * Tulis spesifikasi protokol + golden test vectors + interop test.
   * Handshake terenkripsi, dedupe hash, ack/ping/pong, backoff.
2. **Fase 2 — MVP LAN: teks + gambar (Windows ↔ Android)**
   * Listener dan penulis clipboard, filter data sensitif, mDNS + fallback discovery, pairing QR.
3. **Fase 3 — Transfer file/folder**
   * Chunk streaming, SHA-256, resume, sanitasi path, Share Sheet Android, Radial Drop Menu Windows, jendela Settings.
4. **Fase 4 — Bluetooth fallback**
   * RFCOMM di Windows (WinRT) dan Android, mode Priority/Lazy, announce + pull.
5. **Fase 5 — Mesh, ketahanan, & optimasi**
   * Aturan dial deterministik, failover Wi-Fi → Bluetooth, folder besar, broadcast ke 6–8 perangkat, uji baterai/Doze, packaging satu `.exe` dan satu `.apk`.

### Checklist Uji
* [ ] Golden vector protokol lolos di Kotlin dan Windows.
* [ ] Fuzz parser frame (header 0 B, >64 KiB, `size` negatif/terlalu besar, JSON rusak): koneksi ditutup, tidak crash.
* [ ] Path traversal (`../`, `C:\`, `CON`, nama beda huruf besar-kecil) ditolak.
* [ ] Tidak ada loop clipboard saat 3+ perangkat terhubung.
* [ ] Item password manager tidak pernah terkirim.
* [ ] Transfer file 5 GB: RAM stabil, putus bisa resume, hash cocok.
* [ ] Wi-Fi putus → Bluetooth mengambil alih → Wi-Fi kembali → tidak ada item ganda.
* [ ] Android: layar mati ≥1 jam, mode hemat baterai, dan setelah reboot service berjalan lagi.

---

## 8. Rujukan Clipdeck

| Bagian yang diambil | File di repo Clipdeck |
| :--- | :--- |
| Framing, batas, pesan | `docs/PROTOCOL.md`, `android/.../Protocol.kt` |
| Foreground service, backoff, MediaStore, notifikasi | `android/.../ClipdeckService.kt` |
| State koneksi & riwayat (`StateFlow`) | `android/.../ClipStore.kt` |
| Antrian offline, pola server RFCOMM | `bridge/bridge.js` |
| Filter password manager, hash gambar, dedupe | `extension/extension.js` |
| Interop test lintas bahasa | `android/app/src/test/.../BridgeInteropTest.kt` |

Clipdeck hanya satu arah (komputer → ponsel) dan hanya Bluetooth, jadi yang diambil adalah pola dan pelajaran implementasinya, bukan arsitekturnya. Clipdeck berlisensi **GPL-2.0-or-later**: jika kode disalin (bukan hanya ide), lisensi proyek ini harus kompatibel.
