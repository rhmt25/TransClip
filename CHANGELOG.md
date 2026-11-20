# Changelog

## 1.1.0

Perbaikan sinkronisasi otomatis di latar belakang, terutama untuk HyperOS/MIUI.

- Android: salinan teks sekarang terdeteksi otomatis dari ketukan tombol *Salin/Copy* dan toast *Disalin*. Android 10+ tidak lagi memberi tahu aplikasi latar belakang saat clipboard berubah, sehingga cara lama tidak andal. Hanya label tombol/toast yang diperiksa, bukan isi layar.
- Android: layanan Aksesibilitas yang dilepas sistem ("Layanan ini tidak berfungsi") dipasang ulang otomatis. Perlu izin `WRITE_SECURE_SETTINGS`, diberikan sekali lewat `adb shell pm grant io.transclip.app android.permission.WRITE_SECURE_SETTINGS`.
- Android: perbaikan crash saat boot (`EADDRINUSE` pada port 47820) karena engine dimulai dua kali bersamaan.
- Android: pembacaan clipboard dicoba ulang bila ditolak sesaat setelah fokus.
- Bluetooth: server dan dialer menunggu Bluetooth menyala, tidak lagi harus aktif saat layanan mulai. Hanya komputer/ponsel yang dipasangkan yang dicoba (headset dan controller dilewati), perangkat yang terakhir berhasil dicoba lebih dulu.
- Bluetooth: kini otomatis untuk semua perangkat (tidak perlu mengaktifkan per perangkat) dan perangkat yang pernah terhubung diingat, sehingga bisa tersambung lewat Bluetooth setelah restart tanpa terlihat di LAN.
- Koneksi ulang segera setelah jaringan tersedia atau tervalidasi, tanpa menunggu backoff hingga 30 detik.
- Versi aplikasi: 1.1.0 (Windows dan Android).

## 1.0.0

Rilis pertama.

- Sinkronisasi clipboard teks dan gambar (PNG lossless) antara Windows 11 dan Android lewat LAN.
- Transfer file dan folder dua arah: resume, verifikasi SHA-256, nama bentrok ` (n)`, validasi path.
- Enkripsi end-to-end (X25519 + group key, HKDF, ChaCha20-Poly1305).
- Pairing lewat QR atau kode teks; rotasi Group Key dengan pencabutan perangkat.
- Windows: ikon tray, jendela Settings (egui, proses terpisah), menu *Kirim ke*, folder tujuan yang bisa diubah.
- Android: layanan foreground, Quick Settings tile, Share Sheet, pemilih folder tujuan (SAF), layar transfer dan riwayat.
- Batas: 20 item clipboard / 10 detik per perangkat, 10 perangkat per grup, 3 batch paralel per perangkat.
- Menu radial: tahan klik kiri, seret file, tekan tombol pemicu (default Shift, bisa direkam ulang di Settings), lepas di perangkat tujuan; zona **Batal** di tengah.
- Bluetooth cadangan: bila Wi-Fi putus, perangkat berprioritas tersambung lewat Bluetooth dalam ±13 detik dan kembali ke LAN otomatis; gambar besar diumumkan dan diambil sesuai permintaan.
- Android: sinkron otomatis lewat layanan Aksesibilitas (opsional), pemantau screenshot (opsional), pemilih perangkat di Share Sheet, layar transfer dengan tombol Batal.
