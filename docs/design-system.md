# Sistem desain TransClip

Sumber aturan: `pedoman_desain/Pedoman.md`. Dokumen ini mencatat penerapannya di Windows dan Android.

## Prinsip
- Latar lembut, tidak pernah hitam atau putih pekat (putih hanya untuk permukaan kartu tema terang dan latar QR, yang butuh zona sunyi terang).
- Satu warna aksen biru langit; tanpa gradasi, kaca buram, atau bayangan tebal.
- Struktur dibentuk garis 1 px, spasi kelipatan 4/8 dan tipografi, bukan kartu bersarang.
- Sudut 6 px untuk kontrol, 8 px untuk wadah. Id perangkat, alamat IP, dan kode pairing memakai font monospace.
- Tombol tema (Otomatis / Terang / Gelap) di pojok kanan atas jendela Settings dan di top app bar Android.

## Token
| Token | Terang | Gelap |
|---|---|---|
| canvas | `#F7F9FC` | `#12151A` |
| surface | `#FFFFFF` | `#1A1E26` |
| subtle | `#EDF2F7` | `#232934` |
| text / text2 | `#1A202C` / `#4A5568` | `#F1F5F9` / `#94A3B8` |
| border | `#E2E8F0` | `#2A313D` |
| accent / on-accent | `#0EA5E9` / `#FFFFFF` | `#38BDF8` / `#0B111A` |
| ok / warn / error | `#10B981` `#F59E0B` `#EF4444` | `#34D399` `#FBBF24` `#F87171` |

Kontras teks di atas aksen: 7:1 di tema gelap; di tema terang 2,8:1 (putih di `#0EA5E9`, sesuai pedoman) sehingga hanya dipakai untuk label tombol yang tebal dan besar.

## Penerapan
- **Windows (egui):** `windows/app/src/settings/theme.rs` (token, font Segoe UI dan Consolas, komponen toggle, tombol, badge, navigasi). Kolom navigasi kiri, judul halaman, baris pengaturan dengan garis pemisah. Tema disimpan di `config.toml` (`theme`), mengikuti Windows bila `auto`.
- **Overlay radial:** `windows/app/src/radial/render.rs`. Segmen terpisah dengan sudut membulat dan celah, pusat sebagai zona batal ("N item / Lepas di sini untuk batal"), latar arang transparan `rgba(18,21,26,0.88)`, tepi biru muda, sorotan `#38BDF8`, pusat memerah saat sorotan batal. Bentuk mengikuti `pedoman_desain/bentuk radial desain.jpg`.
- **Android (Compose):** `android/app/src/main/kotlin/io/transclip/app/ui/` (`Theme.kt`, `Components.kt`, `Screens.kt`). Tiga tujuan di navigasi bawah: Beranda, Pairing, Pengaturan. Tema disimpan di DataStore (`themeMode`).
- **Ikon:** `Icon.png` di root diolah menjadi `windows/app/assets/*.ico` (aplikasi, jeda, galat), `icon-256.png` (jendela), dan ikon adaptif Android (`mipmap-anydpi-v26`, latar `#12151A`, ikon monokrom untuk tema Android 13+).
- **Windows Search:** daemon menjaga pintasan `%APPDATA%\Microsoft\Windows\Start Menu\Programs\TransClip.lnk` (bisa dimatikan di Settings → Umum → Windows). Windows mengindeks menu Start, jadi "TransClip" muncul di pencarian dan membuka jendela Settings.

## Revisi 2 (umpan balik pengguna)
- Menu dikurangi: Windows hanya **Perangkat** dan **Pengaturan**; Android hanya **Beranda** dan **Pengaturan** (Pairing menjadi lembar bawah dari tombol Tambah perangkat).
- Tata letak tidak lagi berupa deretan baris: Windows memakai kolom (tile perangkat + panel QR, panel pengaturan dua kolom); Android memakai hero dengan angka besar, tiga kartu aksi, dan karusel kartu perangkat.
- Teks dipangkas; penjelasan panjang dipindah ke tooltip atau dihapus.
- Kontrol sesuai fungsi: *switch* untuk pengaturan aktif/nonaktif, *checkbox* untuk memilih beberapa item (mis. perangkat yang dicabut).
- Ikon aplikasi (`.exe`, tray, jendela, launcher Android) memakai `Icon_bg_putih.png` dengan latar putih (launcher: latar adaptif `#FEFEFE` agar tidak hitam); logo di dalam aplikasi memakai `Icon.png` yang transparan.
