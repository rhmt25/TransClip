# Changelog

## 1.0.0

Rilis pertama.

- Sinkronisasi clipboard teks dan gambar (PNG lossless) antara Windows 11 dan Android lewat LAN.
- Transfer file dan folder dua arah: resume, verifikasi SHA-256, nama bentrok ` (n)`, validasi path.
- Enkripsi end-to-end (X25519 + group key, HKDF, ChaCha20-Poly1305).
- Pairing lewat QR atau kode teks; rotasi Group Key dengan pencabutan perangkat.
- Windows: ikon tray, jendela Settings (egui, proses terpisah), menu *Kirim ke*, folder tujuan yang bisa diubah.
- Android: layanan foreground, Quick Settings tile, Share Sheet, pemilih folder tujuan (SAF), layar transfer dan riwayat.
- Batas: 20 item clipboard / 10 detik per perangkat, 10 perangkat per grup, 3 batch paralel per perangkat.

Belum ada di rilis ini: Bluetooth cadangan (P6) dan menu radial drag-and-drop (P5-04).
