# Log Keputusan

Format: `D-<nomor> · <tanggal> · <judul>` → Keputusan, Alasan, Alternatif yang ditolak. Entri baru ditambahkan di bawah; entri lama tidak diubah (buat entri baru yang menggantikan).

---

### D-001 · 2026-11-18 · Platform v1
* **Keputusan:** hanya Windows 11 dan Android. Linux dikeluarkan dari cakupan.
* **Alasan:** fokus; clipboard latar belakang di Linux (terutama GNOME Wayland) butuh komponen per-desktop.

### D-002 · 2026-11-18 · Bahasa aplikasi Windows: Rust
* **Keputusan:** Rust + `windows-rs`, satu `.exe` dengan CRT statis.
* **Alasan:** ukuran & RAM kecil (target < 30 MB), tanpa runtime.
* **Ditolak:** C# .NET 8 (Native AOT tidak mendukung WinForms/WPF; single-file ±60–80 MB), Python + Nuitka.

### D-003 · 2026-11-18 · Minimum Android: API 29 (Android 10)
* **Alasan:** `MediaStore` (Pictures & Downloads) tanpa izin storage; mencakup hampir semua HP aktif.

### D-004 · 2026-11-18 · Framing: header JSON berprefiks panjang
* **Keputusan:** format frame Clipdeck (u32 panjang + JSON + payload), diperluas dengan batas per tipe dan pesan baru.
* **Alasan:** mudah diperluas (tipe tak dikenal diabaikan), mudah di-debug, terbukti di Clipdeck.
* **Ditolak:** header biner tetap `CLIP + tipe + UUID + panjang` dari blueprint awal (tanpa versi, sulit diperluas).

### D-005 · 2026-11-18 · Kriptografi
* **Keputusan:** handshake X25519 ephemeral + Group Key (PSK) → HKDF-SHA256 → ChaCha20-Poly1305 per record (PROTOCOL §6–7). RustCrypto di Windows, BouncyCastle di Android.
* **Alasan:** kerahasiaan + forward secrecy di LAN; HMAC saja (blueprint awal) tidak mengenkripsi.
* **Ditolak:** Noise via library (library Java tidak terawat); TLS (butuh manajemen sertifikat).

### D-006 · 2026-11-18 · Settings Windows sebagai proses terpisah (egui)
* **Alasan:** RAM daemon tetap kecil; UI dimuat hanya saat dibuka; satu `.exe` dengan argumen `--settings`.

### D-007 · 2026-11-18 · Penyimpanan file diterima di Android
* **Keputusan:** default `MediaStore.Downloads` (`Download/TransClip/…`); SAF opsional untuk folder kustom.
* **Alasan:** tanpa izin storage di API 29+, folder bertingkat didukung lewat `RELATIVE_PATH`.

### D-008 · 2026-11-18 · Tipe foreground service Android: `connectedDevice`
* **Alasan:** cocok untuk LAN dan Bluetooth; prasyarat Android 14+ terpenuhi lewat `CHANGE_WIFI_MULTICAST_STATE` / `BLUETOOTH_CONNECT`. Tidak dibatasi waktu seperti `dataSync`.
* **Perlu diverifikasi** di P3-03 pada Android 14+.

### D-009 · 2026-11-18 · Folder tujuan file dapat diubah di kedua platform
* **Keputusan:** default `Downloads\TransClip` (Windows, via `FOLDERID_Downloads`) dan `Download/TransClip` (Android, via `MediaStore`). Pengguna dapat memilih folder lain: Windows lewat dialog folder native (crate `rfd`), Android lewat SAF dengan izin persisten. Bila folder kustom tidak tersedia saat transfer, fallback ke default + notifikasi.
* **Alasan:** permintaan pengguna. Fallback mencegah file hilang saat drive/izin berubah.
* **Detail:** PLAN §4.5, TODO P5-09.

### D-010 · 2026-11-18 · `file_offer.bytes` menggantikan `file_offer.size`
* **Keputusan:** ukuran file di `file_offer` bernama `bytes`. Field `size` di header frame hanya berarti panjang payload.
* **Alasan:** `size` ganda (ukuran file vs panjang payload) bentrok pada satu objek JSON dan membuat decoder salah membaca payload.

### D-011 · 2026-11-18 · Record layer sebagai `SecureReader`/`SecureWriter`, bukan `AsyncRead`/`AsyncWrite`
* **Keputusan:** lapisan record menyediakan `SecureReader::read_frame` dan `SecureWriter::write_frame` (async) di atas setengah-koneksi hasil `tokio::io::split`, tanpa implementasi trait `AsyncRead`/`AsyncWrite` manual.
* **Alasan:** semua pembaca/penulis di engine hanya berbicara dalam frame; poll-state-machine manual rawan bug dan tidak memberi manfaat. Byte stream plaintext tetap kontinu lintas record seperti di PROTOCOL §7.

### D-012 · 2026-11-18 · Engine Kotlin sebagai modul JVM `:engine`
* **Konteks:** PLAN §6.1 menaruh paket `engine` di dalam `:app`. Logika `Link`, `PeerManager`, `ClipboardSync` tidak butuh Android SDK, dan di `:app` uji JVM-nya lebih lambat serta ikut terikat plugin Android.
* **Keputusan:** buat modul JVM murni `:engine` (paket `io.transclip.engine`) di antara `:proto` dan `:app`, mencerminkan `proto`/`engine` di sisi Rust. Kode khusus Android (NSD, Keystore, service, UI) tetap di `:app`.
* **Dampak:** uji engine berjalan dengan `./gradlew :engine:test` tanpa emulator; `:app` bergantung pada `:engine`.

### D-013 · 2026-11-18 · Transfer file lewat koneksi kontrol, tanpa koneksi TCP kedua
* **Konteks:** PROTOCOL §9.3 semula membuka koneksi TCP baru (hello `role=file`) untuk setiap aliran file di LAN.
* **Keputusan:** `file_stream`/`file_chunk`/`file_end` memakai koneksi kontrol yang sama lewat antrean data `Link`, yang sudah memberi prioritas pada frame kontrol di antara chunk.
* **Alasan:** satu koneksi, satu handshake, satu aturan dial/duplikat; clipboard tidak terblokir karena prioritas dilakukan per frame (≤ 64 KiB). Menghapus kebutuhan routing koneksi berperan `file` dan batas koneksi paralel.
* **Dampak:** PROTOCOL §9.3 diperbarui; kedua engine memakai jalur yang sama untuk LAN dan BT. Throughput satu koneksi TCP sudah memadai untuk LAN rumah.

## D-014 — Pemicu menu radial lewat polling, bukan hook global (2026-11-18)
PLAN §5.4 menyebut `WH_MOUSE_LL`/`WH_KEYBOARD_LL`. Diganti dengan timer 40 ms di thread UI yang membaca `GetAsyncKeyState` (tombol kiri mouse, tombol pemicu, Esc) dan posisi kursor. Efek aktivasi sama (tahan klik kiri + geser > `SM_CXDRAG` + tombol pemicu), tetapi tidak ada hook yang bisa memperlambat input seluruh desktop atau dilepas Windows bila lambat. Overlay, `IDropTarget`, dan geometri sesuai PLAN. Sub-ring "Lainnya…" tidak dibuat: grup maksimal 10 perangkat, jadi paling banyak 9 peer + "Semua".

## D-015 — Failover Bluetooth lebih cepat (2026-11-18)
Uji nyata (PC + POCO F4, Wi-Fi HP dimatikan) memakan 97 detik sebelum Bluetooth aktif. Perbaikan: TCP keep-alive 4 s + 1 s pada semua socket LAN (peer yang hilang terdeteksi ±15 s, bukan menunggu time-out link 90 s); Bluetooth dibuka setelah LAN mati > **5 s** (sebelumnya 10 s); percobaan ulang setelah 3 s; perangkat Bluetooth yang berhasil terakhir dicoba lebih dulu. Hasil: 13 detik, dengan teks terkirim lewat Bluetooth, dan kembali ke LAN dalam 10 detik setelah Wi-Fi menyala.

## D-016 — Pemindai QR dengan kamera di PC dan di dalam lembar Android (2026-11-18)
- **Windows:** dependensi baru `nokhwa` (Media Foundation, hanya fitur `input-msmf`) untuk membaca webcam dan `rqrr` untuk membaca QR. Dipilih karena murni Rust tanpa DLL tambahan. Hanya kode pairing yang berawalan `transclip://pair` yang diterima; kamera berjalan hanya saat mode *Pindai QR* aktif di panel Tambah Perangkat dan berhenti saat mode diganti atau jendela ditutup.
- **Android:** pemindai tidak lagi memakai `CaptureActivity` layar penuh (yang memaksa landscape) tetapi `DecoratedBarcodeView` (zxing-android-embedded, sudah menjadi dependensi) yang disematkan di lembar Tambah Perangkat pada slot persegi 1:1 yang sama dengan QR, sehingga mengikuti rotasi layar. Kamera hanya aktif saat layar *resumed*.
