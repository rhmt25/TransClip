# Hasil spike (dari implementasi nyata, 2026-11-18)

Perangkat uji: HP Redmi 22021211RG / POCO F4 (MIUI, Android), PC Windows 11. Dicatat dari uji langsung selama pengembangan, bukan dari program spike terpisah.

## S1 — Gambar ke clipboard Android dan Gboard
- PC ke HP: gambar PNG diterima, disimpan ke `Pictures/TransClip/` lewat MediaStore, dan ditaruh di clipboard sebagai URI (`ClipData.newUri`). Engine mencatat `Applied`.
- Tempel hanya terlihat di aplikasi yang mendukung tempel gambar (WhatsApp, Gboard, Google Pesan); kolom teks biasa tidak menampilkannya.
- **Temuan:** saran screenshot di Gboard bukan isi clipboard sistem. Di HP uji, screenshot dan "salin" tertentu tidak mengubah clipboard sistem (log: `mimes=[text/plain]`, tanpa notifikasi perubahan). Solusi: pemantau screenshot lewat MediaStore (opt-in) dan jalur Share Sheet.

## S2 — Auto-capture clipboard Android
- `OnPrimaryClipChangedListener` terpanggil di latar belakang untuk teks.
- Membaca isi clipboard di latar belakang diblokir Android 10+. Layanan Aksesibilitas (tanpa membaca layar) boleh membuka layar transparan `ClipCaptureActivity` dari latar belakang; layar itu mendapat fokus sehingga clipboard terbaca. Teks tersinkron otomatis, kedipan layar singkat.
- Gambar hanya tertangkap bila aplikasi sumber benar-benar menaruhnya di clipboard sistem.

## S3 — Radial overlay menerima drop dari Explorer
Berhasil (uji otomatis 2026-11-18): overlay `WS_EX_LAYERED|NOACTIVATE|TOPMOST` menerima drop `CF_HDROP` lewat `IDropTarget` dari sumber drag OLE (`DoDragDrop`). Pemicu final: Shift (polling, D-014). Belum diuji dari Explorer, aplikasi elevated, dan monitor dengan DPI berbeda.

## S4 — RFCOMM Windows dan Android
- Windows: WinRT `RfcommServiceProvider` + `StreamSocketListener` (server) dan `BluetoothDevice.GetRfcommServicesForIdAsync` + `StreamSocket` (klien) berfungsi untuk layanan UUID PROTOCOL §2; API final: **WinRT**, bukan Winsock `AF_BTH`.
- Android: `listenUsingRfcommWithServiceRecord` / `createRfcommSocketToServiceRecord`.
- Pengguna melaporkan koneksi Bluetooth berjalan ("sudah bisa"); log PC mencatat `peer online via=Bluetooth`. Throughput belum diukur.
