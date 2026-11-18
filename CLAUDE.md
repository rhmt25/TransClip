# CLAUDE.md — Aturan Kerja Proyek TransClip

TransClip = sinkronisasi clipboard (teks & gambar) dan transfer file/folder P2P antara **Windows 11** (Rust, satu `.exe`) dan **Android** (Kotlin, satu `.apk`), lewat LAN dan Bluetooth, terenkripsi end-to-end.

## Urutan baca sebelum mulai
1. `TODO.md` — cari task pertama yang belum `[x]`. Itu tugasmu.
2. `docs/PLAN.md` — bagian yang dirujuk task.
3. `docs/PROTOCOL.md` — wajib dibaca utuh sebelum menyentuh kode jaringan/kripto.
4. `docs/DECISIONS.md` — keputusan yang sudah final; jangan dibuka ulang tanpa alasan baru.
5. `blueprint_aplikasi_multi_device_clipboard_file_sync.md` — visi produk (konteks saja).

## Cara mengerjakan task
1. Ubah status task di `TODO.md` menjadi `[~]`.
2. Kerjakan hanya cakupan task itu. Temuan di luar cakupan → tambahkan sebagai task baru di fase yang tepat (ID berikutnya, mis. `P2-10`), jangan dikerjakan diam-diam.
3. Tulis test lebih dulu atau bersamaan. Semua kriteria **Selesai jika** harus terpenuhi.
4. Jalankan perintah verifikasi (bagian bawah). Semua harus hijau.
5. Ubah status menjadi `[x]`, perbarui baris **Progres** di `TODO.md` jika satu fase selesai.
6. Commit satu task = satu commit (atau beberapa commit kecil): `P2-04: clipboard sync & offline queue`.
7. Lanjut ke task berikutnya. Berhenti dan lapor ke pengguna bila menemui task **[MANUAL]**, task `[!]` terblokir, atau keputusan desain yang tidak tercakup dokumen.

## Task [MANUAL]
Agent tidak punya perangkat fisik. Untuk task [MANUAL]:
* Siapkan semuanya (build, instruksi langkah demi langkah dalam Bahasa Indonesia, apa yang harus diamati).
* Minta pengguna menjalankan, lalu catat hasilnya di `docs/manual-tests.md` / `docs/spikes/`.
* Jangan menandai `[x]` sebelum pengguna mengonfirmasi.
* Pengguna bisa menjalankan perintah lewat prompt dengan prefix `! <perintah>`.

## Aturan keras
* **Protokol:** setiap perubahan perilaku kabel → perbarui `docs/PROTOCOL.md`, kedua implementasi, `cargo run -p tvgen`, dan `DECISIONS.md` dalam commit yang sama. Kedua sisi harus lulus test vector.
* **Kripto:** hanya primitif dari library yang ditetapkan di PLAN §2. Jangan menulis primitif sendiri. Perbandingan MAC constant-time. Kunci di-zeroize.
* **Privasi:** jangan pernah me-log isi clipboard, nama file pengguna pada level di atas `debug`, kunci, atau payload pairing. Log cukup id, ukuran, mime, hash pendek (8 karakter).
* **Input jaringan tidak dipercaya:** tidak ada `unwrap()`/`expect()`/`!!` pada data dari jaringan; semua batas di PROTOCOL §10 diterapkan.
* **Clipdeck/** adalah referensi milik orang lain berlisensi GPL-2.0-or-later: boleh dibaca untuk ide/pola, **tidak boleh** menyalin kodenya. Jangan mengubah isi folder itu.
* Jangan menambah dependency besar di luar PLAN §2 tanpa entri `DECISIONS.md`.
* Jangan menghapus/menonaktifkan test untuk membuat build hijau.
* Antarmuka mengikuti `pedoman_desain/Pedoman.md` dan `docs/design-system.md` (token warna, tanpa kartu bersarang, tombol tema).
* Bahasa: kode, identifier, komentar kode, dan pesan commit dalam **Bahasa Inggris**; dokumen di `docs/`, teks UI, dan komunikasi dengan pengguna dalam **Bahasa Indonesia** (UI siapkan resource string agar bisa ditambah Bahasa Inggris nanti).

## Konvensi kode
### Rust (`windows/`)
* `cargo fmt`; `cargo clippy --workspace --all-targets -- -D warnings`.
* Error: `thiserror` di crate library, `anyhow` hanya di `app`.
* `unsafe` hanya di modul Win32 (`app/src/{clipboard,radial,tray,bluetooth,ipc,paths,secrets,single_instance,theme_sys,autostart}`), setiap blok diberi komentar `// SAFETY:`.
* Crate `proto` dan `engine` tidak boleh bergantung pada crate `windows`.
* Logging lewat `tracing` dengan field terstruktur.

### Kotlin (`android/`)
* Kotlin style resmi; coroutine dengan scope terstruktur, tanpa `GlobalScope`.
* Modul `:proto` tidak boleh bergantung pada Android SDK.
* Teks UI di `res/values/strings.xml`.
* Semua akses clipboard di main thread; I/O di `Dispatchers.IO`.

## Perintah verifikasi
```powershell
# Windows (dari folder windows/)
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p tvgen        # lalu pastikan `git diff docs/test-vectors` kosong

# Android (dari folder android/)
./gradlew :proto:test :app:testDebugUnitTest :app:lintDebug :app:assembleDebug

# Interop live (setelah P1-11 / P5-02)
cargo build -p proto --example echo_peer
$env:TRANSCLIP_ECHO_PEER = "$PWD\target\debug\examples\echo_peer.exe"
cargo build -p engine --example interop_peer
$env:TRANSCLIP_INTEROP_PEER = "$PWD\target\debug\examples\interop_peer.exe"
# lalu jalankan ./gradlew :proto:test :app:testDebugUnitTest dari android/

# Instal ke HP
./gradlew :app:installDebug
adb logcat -s TransClip
```

## Lingkungan
* Host pengembangan: Windows 11, PowerShell. Rust & Android SDK dipasang di P0-02/P0-03.
* Bukan Play Store: `.apk` didistribusikan langsung.

## Catatan lingkungan (Windows PowerShell 5.1)
* Jangan membaca/menulis file berisi karakter non-ASCII dengan Get-Content/Set-Content tanpa -Encoding UTF8; hasilnya rusak (mojibake). Pakai Edit/Write, atau Python dengan encoding utf-8.
* Jalankan Gradle dari android/ dengan .\gradlew.bat; JDK 23 dipakai, bytecode target 17.

