# Berkontribusi

Terima kasih sudah mau membantu. Dengan mengirim kontribusi, Anda setuju kode tersebut dilisensikan di bawah `GPL-3.0-or-later` (lihat `LICENSE`).

Baca dulu `docs/ARCHITECTURE.md`; baca `docs/PROTOCOL.md` utuh sebelum menyentuh kode jaringan atau kripto, dan `docs/DECISIONS.md` agar tidak membuka ulang keputusan yang sudah final.

## Verifikasi sebelum membuka PR
```powershell
# Windows (dari folder windows/)
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p tvgen        # lalu pastikan `git diff docs/test-vectors` kosong

# Android (dari folder android/)
.\gradlew.bat :proto:test :app:testDebugUnitTest :app:lintDebug :app:assembleDebug
```

## Aturan
* **Protokol:** perubahan perilaku kabel memperbarui `docs/PROTOCOL.md`, kedua implementasi (Rust dan Kotlin), test vector (`cargo run -p tvgen`), dan `docs/DECISIONS.md` dalam satu commit. Kedua sisi harus lulus test vector yang sama.
* **Kripto:** hanya primitif dari library yang sudah dipakai (RustCrypto, BouncyCastle); jangan menulis primitif sendiri. Perbandingan MAC constant-time, kunci di-zeroize.
* **Privasi:** jangan log isi clipboard, nama file pengguna di atas level `debug`, kunci, atau payload pairing. Cukup id, ukuran, mime, dan hash pendek.
* **Input jaringan tidak dipercaya:** tanpa `unwrap()`/`expect()`/`!!` pada data dari jaringan; terapkan batas di PROTOCOL §10.
* Jangan menambah dependency besar tanpa entri di `docs/DECISIONS.md`. Jangan menghapus atau menonaktifkan test agar build hijau.
* Jangan menyalin kode dari proyek yang lisensinya tidak kompatibel dengan GPL-3.0-or-later.
* UI mengikuti `docs/design-system.md` dan `docs/design/guidelines.md`.

## Konvensi kode
* **Rust (`windows/`):** `cargo fmt`, clippy tanpa peringatan; `thiserror` di crate library, `anyhow` hanya di `app`; `unsafe` hanya di modul Win32 dengan komentar `// SAFETY:`; crate `proto` dan `engine` tidak bergantung pada crate `windows`; logging lewat `tracing` dengan field terstruktur.
* **Kotlin (`android/`):** gaya resmi Kotlin; coroutine terstruktur (tanpa `GlobalScope`); modul `:proto` tidak bergantung pada Android SDK; teks UI di `res/values/strings.xml`; akses clipboard di main thread, I/O di `Dispatchers.IO`.
* **Bahasa:** kode, identifier, komentar, dan pesan commit dalam Bahasa Inggris; dokumen dan teks UI dalam Bahasa Indonesia.
