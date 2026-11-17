# Uji Manual

Isi kolom Hasil dan Tanggal setelah dijalankan di perangkat asli.

| ID | Skenario | Hasil | Tanggal |
| --- | --- | --- | --- |
| M01 | Teks PC → semua HP (< 1 s), muncul di Gboard |  |  |
| M02 | Screenshot Win+Shift+S → HP, kualitas identik (bandingkan hash PNG) |  |  |
| M03 | Teks HP (tile) → PC, muncul di Win+V |  |  |
| M04 | Foto galeri HP (share) → PC sebagai gambar clipboard |  |  |
| M05 | Salin dari password manager → tidak terkirim |  |  |
| M06 | Tidak ada loop/duplikat dengan 1 PC + 2 HP aktif |  |  |
| M07 | Radial: 1 file, 20 file, folder bertingkat → 1 peer dan "Semua" |  |  |
| M08 | Share 10 foto dari HP → PC, struktur & hash cocok |  |  |
| M09 | File 2 GB, cabut Wi-Fi di 50%, sambung lagi → resume, hash cocok |  |  |
| M10 | Wi-Fi mati → BT mengambil alih teks; Wi-Fi kembali → LAN |  |  |
| M11 | HP layar mati 1 jam → masih menerima teks |  |  |
| M12 | Reboot PC & HP → otomatis tersambung lagi |  |  |
| M13 | Pairing via QR dan via kode teks; QR kedaluwarsa ditolak |  |  |
| M14 | Rotasi kunci: perangkat dicabut tidak bisa terhubung |  |  |
| M15 | Path jahat (dikirim via `interop_peer`) ditolak |  |  |
| M16 | Drag dari app Admin → didokumentasikan tidak didukung, tidak crash |  |  |
| M17 | Folder tujuan: default di Download (kedua platform) → ubah ke folder lain → file tiba di folder baru → folder dicabut/dihapus → fallback ke Download + notifikasi → Reset ke default |  |  |
| T01 | Tray Windows (P2-06): jalankan `transclip.exe` → ikon biru muncul di tray; klik kanan → menu (status, Kirim file, Jeda, Pengaturan, Pairing, Buka folder unduhan, Keluar); Jeda → ikon abu-abu dan tanda centang; Keluar → ikon hilang, proses berhenti; restart Explorer → ikon kembali |  |  |
| T02 | Clipboard Windows (P2-07): dua perangkat dalam satu grup → salin teks dan screenshot (Win+Shift+S) di satu sisi, muncul di `Win + V` sisi lain; salin dari KeePass/Bitwarden → tidak terkirim; tidak ada loop salin-ulang |  |  |
| T03 | Autostart (P2-09): aktifkan `autostart` → keluar dari sesi Windows dan masuk lagi → daemon berjalan otomatis; nonaktifkan → tidak berjalan lagi |  |  |

## Catatan uji otomatis lewat adb (2026-11-18, HP 22021211RG / Android 14 ↔ PC Windows 11, satu Wi-Fi)

Dijalankan oleh agen, bukan pengganti uji pengguna; tanda `[x]` di TODO tetap menunggu konfirmasi pengguna.

* Pairing lewat tautan `transclip://pair?...` (membuat grup di PC lewat IPC, mengisi kode di HP, menekan *Gabung*): kedua sisi terhubung, `list_peers` di PC menampilkan HP `online`.
* M01 (sebagian): teks PC → HP diterapkan (`Applied`) dalam < 1 s. Isi clipboard HP belum dibaca balik.
* M03 (sebagian): teks di HP → tombol *Kirim clipboard* → clipboard PC berisi teks yang sama (`Delivered`).
* M02 (sebagian): gambar PC (clipboard Windows) → HP: file `Pictures/TransClip/TransClip-xxxx.png`, 320×200, piksel identik dengan sumber.
* Belum diuji: tile Quick Settings, layar mati lama, reboot, gambar HP → PC, KeePass/Bitwarden, tray (T01), autostart (T03).
