# TransClip

Sinkronisasi clipboard (teks dan gambar) dan transfer file/folder antara **Windows 11** dan **Android** lewat jaringan lokal, terenkripsi end-to-end. Satu `.exe` untuk Windows, satu `.apk` untuk Android. Tanpa server, tanpa akun.

## Fitur
- Salin di satu perangkat, tempel di perangkat lain. Gambar dikirim lossless (PNG).
- Kirim file dan folder dua arah, dengan resume dan verifikasi hash.
- Banyak perangkat dalam satu grup (maksimal 10), semuanya saling terhubung.
- Tidak ada yang keluar dari jaringan Anda. Isi clipboard dan nama file tidak pernah dicatat di log.

## Pasang
1. **Windows:** jalankan `TransClip-<versi>-windows-x64.exe`. Ikon muncul di tray (cek panah **^**). Saat dialog firewall muncul, pilih **Private network**.
2. **Android:** pasang `TransClip-<versi>.apk` (izinkan instal dari sumber ini), buka aplikasinya, dan izinkan notifikasi.

## Pairing
1. Di PC: klik kanan ikon tray, *Pengaturan*, tab *Pairing*, **Buat grup baru** lalu **Tampilkan kode pairing**.
2. Di HP: **Scan QR** dan arahkan ke layar PC (atau tempel kodenya, lalu **Gabung**). Kode berlaku 2 menit.
3. Perangkat saling terhubung dalam beberapa detik. Untuk menambah perangkat lain, ulangi dari perangkat mana pun yang sudah tergabung.

## Memakai
- **Clipboard:** salin seperti biasa. Di Android, buka aplikasi atau pakai tile *Kirim clipboard* di Quick Settings (Android membatasi akses clipboard di latar belakang).
- **Menu radial (PC):** tahan klik kiri pada file, seret, tekan Shift (ubah di Settings → Radial), lepas di perangkat tujuan; lepas di pusat untuk batal.
- **Kirim file dari PC:** klik kanan file, *Kirim ke*, *TransClip* (aktifkan di Settings, tab Umum), atau menu tray *Kirim file…*, atau `transclip.exe --send <berkas>`.
- **Kirim file dari HP:** tombol *Kirim file* atau Share Sheet.
- **Folder tujuan:** default `Downloads\TransClip` (PC) dan `Download/TransClip` (HP). Bisa diubah di Settings; bila folder tidak bisa dipakai, TransClip kembali ke default dan memberi tahu.

## Izin Android
| Izin | Alasan |
|---|---|
| Jaringan, Wi-Fi multicast | Menemukan dan menghubungi perangkat lain di LAN |
| Notifikasi | Layanan latar belakang dan hasil transfer |
| Kamera | Hanya untuk scan QR pairing |
| Mulai saat boot | Tersambung otomatis setelah HP dinyalakan (bisa dimatikan) |

## Keterbatasan
- Windows tidak membagikan clipboard dari aplikasi yang dijalankan sebagai Administrator ke proses biasa (UIPI).
- Android 10+ hanya mengizinkan membaca clipboard saat aplikasi di depan; salin otomatis dari latar belakang tidak tersedia.
- Folder kosong tidak dibuat di penyimpanan default Android (MediaStore tidak mengenal folder kosong).
- Bluetooth hanya dipakai bila PC dan HP sudah di-pair di pengaturan Bluetooth masing-masing dan opsi *Bluetooth bila Wi-Fi putus* dicentang.
- Sinkron otomatis di Android memerlukan layanan Aksesibilitas TransClip (hanya mendengarkan perubahan clipboard, tidak membaca layar); layar transparan berkedip sebentar saat menyalin. Setelah memasang ulang APK, matikan lalu nyalakan layanannya.

Masalah koneksi: lihat `docs/troubleshooting.md`. Spesifikasi: `docs/PLAN.md` dan `docs/PROTOCOL.md`.
