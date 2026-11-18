# TransClip

[![License: GPL v3+](https://img.shields.io/badge/license-GPL--3.0--or--later-blue.svg)](LICENSE)

*English: free and open-source clipboard (text and images) and file/folder sync between Windows 11 and Android over LAN with Bluetooth fallback, end-to-end encrypted, no server and no account.*

Sinkronisasi clipboard (teks dan gambar) dan transfer file/folder antara **Windows 11** dan **Android** lewat jaringan lokal, terenkripsi end-to-end. Satu `.exe` untuk Windows, satu `.apk` untuk Android. Tanpa server, tanpa akun.

## Fitur
- Pairing dengan QR: tampilkan atau pindai dengan kamera (HP dan laptop).
- Antarmuka terang dan gelap (ikuti sistem atau pilih manual), TransClip muncul di pencarian Windows.
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
- **Clipboard:** salin seperti biasa. Di Android, aktifkan layanan Aksesibilitas TransClip agar tersinkron otomatis; tanpa itu, buka aplikasi atau pakai tile *Kirim clipboard* di Quick Settings (Android membatasi akses clipboard di latar belakang).
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
- Android 10+ hanya mengizinkan membaca clipboard saat aplikasi di depan; salin otomatis memakai layanan Aksesibilitas, dan sebagian ROM tidak menaruh screenshot di clipboard (gunakan opsi *Kirim screenshot otomatis*).
- Folder kosong tidak dibuat di penyimpanan default Android (MediaStore tidak mengenal folder kosong).
- Bluetooth hanya dipakai bila PC dan HP sudah di-pair di pengaturan Bluetooth masing-masing dan opsi *Bluetooth bila Wi-Fi putus* dicentang.
- Sinkron otomatis di Android memerlukan layanan Aksesibilitas TransClip (hanya mendengarkan perubahan clipboard, tidak membaca layar); layar transparan berkedip sebentar saat menyalin. Setelah memasang ulang APK, matikan lalu nyalakan layanannya.

Masalah koneksi: lihat `docs/troubleshooting.md`.

## Membangun dari sumber
Prasyarat: Rust stable (target `x86_64-pc-windows-msvc`) dengan Visual Studio Build Tools, serta JDK 17+ dan Android SDK (platform 35) untuk Android.

```powershell
# Windows (dari folder windows/)
cargo build --release -p transclip

# Android (dari folder android/)
.\gradlew.bat :app:assembleDebug

# Keduanya + test, hasil di dist/
.\scriptsuild-release.ps1
```

Rilis bertanda tangan: lihat `docs/release.md`.

## Dokumentasi
| Dokumen | Isi |
|---|---|
| `docs/ARCHITECTURE.md` | Struktur kode dan komponen |
| `docs/PROTOCOL.md` | Spesifikasi protokol kabel dan kripto (normatif) |
| `docs/DECISIONS.md` | Log keputusan desain |
| `docs/design-system.md`, `docs/design/` | Pedoman dan token desain UI |
| `docs/security-review.md` | Tinjauan keamanan mandiri |
| `docs/manual-tests.md` | Skenario uji di perangkat asli |
| `CONTRIBUTING.md` | Cara berkontribusi |
| `SECURITY.md` | Melaporkan celah keamanan |

## Lisensi
TransClip adalah perangkat lunak bebas di bawah **GNU General Public License v3.0 atau yang lebih baru** (`GPL-3.0-or-later`); lihat [LICENSE](LICENSE). Program ini dibagikan dengan harapan berguna, tanpa jaminan apa pun.
