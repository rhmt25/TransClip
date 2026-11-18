# Audit keamanan mandiri (P7-03)

Tanggal: 2026-11-18. Dikerjakan oleh pengembang (bukan audit pihak ketiga). Status: OK / CATATAN.

| # | Item | Status | Bukti |
|---|---|---|---|
| 1 | Tidak ada log isi clipboard, kunci, payload pairing | OK | Log hanya id, ukuran, hash pendek; `grep` pada `tracing::` dan `Log.` tidak menemukan isi. Nama file hanya di level debug. |
| 2 | Kunci di-zeroize | OK | `GroupKey` memakai `ZeroizeOnDrop`; secrets Windows lewat DPAPI, Android lewat Keystore AES-GCM |
| 3 | DACL named pipe hanya pemilik | OK | `D:P(A;;GA;;;OW)` di `ipc/server.rs`; ada test |
| 4 | Semua input jaringan divalidasi | OK | Batas PROTOCOL §10; fuzz `proto/tests/fuzz.rs` (7 properti x 10 000) dan `FuzzTest.kt` (10 000) tanpa panic |
| 5 | Tidak ada `unwrap`/`expect`/`!!` pada data jaringan | OK | lint clippy `unwrap_used`/`expect_used` aktif; Kotlin ditinjau manual |
| 6 | QR/kode pairing tidak disimpan | OK | Hanya di memori; berlaku 2 menit; deep link hanya mengisi kolom, pengguna harus menekan Gabung |
| 7 | File diterima tidak dieksekusi atau dibuka otomatis | OK | Tidak ada pemanggilan `ShellExecute` atau `spawn` pada file masuk; hanya folder tujuan dan folder log yang dibuka di Explorer |
| 8 | Path traversal | OK | `validate_relative_path` (Rust dan Kotlin) diuji vektor bersama + fuzz. **Fuzz menemukan panic pada nama `COM`+karakter multibyte; sudah diperbaiki** |
| 9 | Rate limit | OK | 20 item clipboard / 10 detik per perangkat; 3 batch paralel; `max_incoming_file_bytes`; cek ruang disk |
| 10 | Rotasi kunci mencabut perangkat | OK | Test engine: perangkat yang dicabut gagal handshake dengan kunci baru |
| 11 | Dependency audit | CATATAN | `cargo audit` dan dependency-check belum terpasang di mesin pengembang; jalankan sebelum rilis (`cargo install cargo-audit`; `cargo audit`) |
| 12 | Anggota grup tepercaya penuh | CATATAN (diterima) | Satu Group Key bersama: setiap anggota dapat memulai rotasi kunci atau mengirim clipboard. Sesuai D-keputusan model ancaman (PLAN §7) |
| 13 | Lalu lintas LAN terenkripsi tetapi beacon UDP tidak | CATATAN (diterima) | Beacon hanya memuat id grup (hash), id perangkat, port, nama; tidak ada rahasia |
| 14 | Windows tidak ditandatangani | CATATAN | SmartScreen akan memperingatkan; cocokkan SHA-256 |
