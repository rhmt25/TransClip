# Arsitektur

TransClip terdiri dari dua aplikasi yang berbicara protokol kabel yang sama (`docs/PROTOCOL.md`): daemon Windows (Rust) dan aplikasi Android (Kotlin). Tidak ada server; perangkat saling menemukan lewat mDNS dan beacon UDP, lalu terhubung langsung (TCP, atau Bluetooth RFCOMM sebagai cadangan).

## Struktur repositori
```
assets/branding/     ikon sumber (diolah menjadi ikon Windows dan Android)
docs/                dokumentasi; test-vectors/ dihasilkan oleh tvgen
scripts/             build-release.ps1 (test + build + checksum)
windows/             Cargo workspace
  crates/proto/      framing, record layer, handshake, pesan, validasi path (murni, tanpa Win32)
  crates/engine/     peer manager, discovery, transfer, antrian offline (tokio)
  crates/tvgen/      generator test vector
  app/               transclip.exe: tray, clipboard, radial, bluetooth, ipc, settings (egui)
android/             Gradle project
  proto/             modul JVM murni (framing, kripto, pesan), diuji dengan JUnit
  engine/            peer, discovery, transfer, sinkronisasi (JVM murni)
  app/               aplikasi: service, Compose UI, clipboard, penyimpanan
```
Memisahkan `proto` dan `engine` dari `app` membuat logika inti dapat diuji dengan `cargo test` dan JVM test tanpa perangkat.

## Lapisan logika (kedua platform)
```
Platform layer: ClipboardWatcher / ClipboardWriter / UI / FileSource / Notifier
        |
Engine:         ClipboardSync, TransferManager, PeerManager, OfflineQueue, Discovery
        |
Link:           Handshake -> RecordLayer -> FrameCodec  (TcpTransport, RfcommTransport)
```

| Komponen | Tanggung jawab |
|---|---|
| `Discovery` | mDNS register/browse dan beacon UDP; menghasilkan event peer terlihat |
| `PeerManager` | satu peer per `device_id`: status, backoff, aturan dial, memilih link terbaik |
| `Link` | koneksi terautentikasi: loop baca, antrian tulis berprioritas, ping/pong |
| `ClipboardSync` | filter item sensitif, hash, dedupe, kirim ke peer online atau antrian offline |
| `TransferManager` | batch file/folder, penawaran, resume, verifikasi hash, kuota |

## Pemilihan transport
LAN dipakai bila tersedia. Bila LAN ke peer mati lebih dari 5 detik dan peer ditandai berprioritas Bluetooth serta sudah paired di OS, link Bluetooth dibuka; link ditutup setelah LAN stabil 60 detik.

## Windows
Daemon (`transclip.exe`) menjalankan thread Win32 untuk tray/clipboard/radial dan runtime tokio untuk jaringan. Jendela Settings adalah proses terpisah (`transclip.exe --settings`, egui) yang berbicara dengan daemon lewat named pipe `\\.\pipe\transclip-<SID>` (JSON per baris), sehingga RAM daemon tetap kecil.

## Android
Foreground service memegang engine; UI Compose membaca state lewat `StateFlow`. Penyimpanan memakai MediaStore dan SAF (minSdk 29). Sinkron clipboard otomatis memakai layanan Aksesibilitas opsional; tanpanya tersedia tile Quick Settings dan Share Sheet.

## Teknologi
| Area | Pilihan |
|---|---|
| Kripto Rust | RustCrypto: `x25519-dalek`, `chacha20poly1305`, `hkdf`, `hmac`, `sha2`, `subtle`, `zeroize` |
| Kripto Android | BouncyCastle (X25519, ChaCha20-Poly1305, HKDF) dan JCA |
| JSON | `serde_json` / `kotlinx.serialization` |
| mDNS | `mdns-sd` / `NsdManager` |
| QR | `qrcode` (render), ZXing (scan dan render Android) |
| UI | egui (Windows), Jetpack Compose (Android) |
