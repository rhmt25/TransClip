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
