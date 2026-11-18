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
* Transfer file (adb, 2026-11-18): folder `Kiriman Uji` (sub-folder bertingkat, folder kosong, file 3 MB) + file lepas PC → HP: struktur tiba di `Download/TransClip/`, SHA-256 `data.bin` sama dengan sumber. File 2,5 MB HP → PC (lewat Share): SHA-256 sama. Folder kosong tidak dibuat di HP (MediaStore tidak punya folder kosong; dicatat sebagai batasan).
* Belum diuji: M08 (10 foto), M09 (2 GB + cabut Wi-Fi), radial menu, pemilih perangkat, folder tujuan kustom.

## T04 — Folder tujuan yang dapat diubah (P5-09)
**Android:** buka TransClip → *Folder penerimaan* → **Ubah…** → pilih folder (mis. Documents/Kiriman) → kirim file/folder dari PC. Harapan: file tiba di folder itu, subfolder utuh, nama bentrok menjadi `nama (1)`. **Reset ke default** → kembali ke `Download/TransClip`. Hapus folder terpilih lewat Files, restart layanan → notifikasi "Folder tujuan tidak dapat diakses" dan file masuk ke default.
**Windows:** `ipc.ps1 -Method set_download_dir -ParamsJson '{"dir":"D:\Kiriman"}'` (UI Settings menyusul di P4) → kirim dari HP → tiba di folder itu; `{"dir":""}` untuk reset; path relatif ditolak. Menu tray *Buka folder unduhan* membuka folder efektif. Folder tidak bisa ditulis → fallback ke `Downloads\TransClip` + notifikasi.
Sisa P5-09: tombol Ubah/Reset di jendela Settings Windows (menunggu P4-04, memakai method IPC di atas).

## T05 — Jendela Settings Windows (P4-01, P4-02)
Jalankan `transclip.exe`, klik kanan tray → *Pengaturan* (atau `transclip.exe --settings`). Harapan: jendela dengan tab Perangkat / Pairing / Umum / Lanjutan; menutup jendela mengakhiri prosesnya (Task Manager) dan RAM daemon tidak naik. Tab Pairing: *Tampilkan kode pairing* → QR + hitung mundur 2 menit + *Salin kode*; QR harus terbaca aplikasi scanner umum. Tab Umum: setiap checkbox langsung berlaku, *Ubah…* memilih folder tujuan. Tanpa daemon: jendela menawarkan *Jalankan TransClip*.

## T06 — Pairing QR di Android (P4-03)
HP: *Scan QR* (izinkan kamera) → arahkan ke QR di jendela Settings PC (tab Pairing → Tampilkan kode pairing). Harapan: terhubung < 5 detik; bila HP sudah punya grup, muncul dialog konfirmasi *Ganti grup?*. Sebaliknya: *Buat grup* di HP menampilkan QR; Android B memindainya.

## T07 — Ringkasan uji akhir (P4-04, P4-05, P5-05, P5-08, P7-05)
* **Pengaturan Android:** ganti nama perangkat, matikan *Sinkronkan teks* lalu salin teks dari PC → tidak muncul di HP.
* **Rotasi kunci (M14):** PC → Settings → Pairing → *Ganti Group Key…*, centang satu HP sebagai dicabut → HP lain tetap tersambung, HP yang dicabut berstatus offline dan perlu pairing ulang.
* **Kirim ke (M07 sebagian):** Settings → Umum → centang *Tampilkan di menu Kirim ke*; klik kanan file → Kirim ke → TransClip → pilih perangkat. Tray → *Kirim file…* mengirim ke semua perangkat online.
* **Layar transfer Android:** kirim file besar dari PC; progres bergerak, *Batal* menghentikan transfer.
* **Sleep/resume (P7-05):** tidurkan PC 1 menit, bangunkan → HP kembali online < 15 detik.
* **Rate limit:** kirim > 20 salinan dalam 10 detik dari satu perangkat → sisanya ditolak (`busy`), tidak ada crash.

## T08 — Sinkron otomatis Android (P3-06)
* **Teks:** aktifkan layanan Aksesibilitas TransClip → salin teks di aplikasi mana pun → muncul di PC tanpa menekan tombol (diuji 2026-11-18 di Redmi 22021211RG, lulus).
* **Screenshot:** Pengaturan → *Kirim screenshot otomatis* (izin foto) → ambil screenshot → tertempel sebagai gambar di PC (lulus 2026-11-18).
* **Catatan:** banyak ROM tidak menaruh screenshot atau gambar "salin" di clipboard sistem; saran gambar di Gboard bukan isi clipboard sistem. Gambar yang benar-benar disalin ke clipboard (mis. Chrome *Salin gambar*) ditangkap lewat layanan Aksesibilitas bila aplikasinya mengisi clipboard sistem.

## T09 — Menu radial (P5-04, M07)
1. Pastikan minimal satu HP online. Di Explorer, **tahan klik kiri pada sebuah file, geser, lalu tekan Right Ctrl** (ganti lewat Settings → Radial → *Rekam tombol*, tekan tombol apa saja). Lingkaran perangkat muncul di kursor.
2. Lepas file di segmen perangkat → tiba di HP. Segmen "Semua" → ke semua perangkat online. Lepas di tengah lingkaran atau tekan **Esc** → batal.
3. Ulangi dengan banyak file (20) dan satu folder bertingkat. Uji juga di monitor kedua dengan skala DPI berbeda.
4. Drag dari aplikasi yang dijalankan sebagai Administrator tidak didukung (UIPI); pastikan tidak crash.

## T10 — Bluetooth cadangan (P6-01..03, M10)
Prasyarat: **pair PC dan HP di pengaturan Bluetooth masing-masing** (Windows: Settings → Bluetooth & devices → Add device; Android: Bluetooth → Pasangkan). Daemon PC mencatat `bluetooth service advertised` bila layanan RFCOMM terdaftar.
1. PC: Settings → tab *Perangkat* → centang **Bluetooth bila Wi-Fi putus** untuk HP. HP: layar utama → saklar **Bluetooth bila Wi-Fi putus** pada PC (izinkan Bluetooth bila diminta).
2. Pastikan tersambung lewat Wi-Fi (HP menampilkan "· Wi-Fi"). Matikan Wi-Fi HP.
3. Dalam ≈ 15 detik status berubah menjadi "· Bluetooth"; salin teks di PC → muncul di HP.
4. Nyalakan Wi-Fi lagi → kembali ke "· Wi-Fi"; link Bluetooth ditutup setelah 60 detik tanpa teks hilang.
Gambar > 2 MiB lewat Bluetooth tidak dikirim langsung: HP menampilkan notifikasi "Gambar tersedia — ketuk untuk ambil" (PC mengambilnya otomatis). Uji dengan screenshot besar saat Wi-Fi mati.
