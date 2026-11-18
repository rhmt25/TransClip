# TransClip — Rencana Implementasi Siap Produksi

Dokumen ini menjelaskan **bagaimana** membangun TransClip. Visi & alasan desain ada di `blueprint_aplikasi_multi_device_clipboard_file_sync.md`; format kabel ada di `docs/PROTOCOL.md`; urutan pekerjaan ada di `TODO.md`.

---

## 1. Ruang Lingkup v1.0

### 1.1 Termasuk
* Sinkronisasi clipboard **teks** dan **gambar** dua arah antar Windows 11 dan Android (≤10 perangkat per grup).
* Transfer **file dan folder** (lossless, resume, verifikasi SHA-256).
* LAN (utama) + Bluetooth Classic RFCOMM (cadangan).
* Pairing grup lewat QR / kode teks, enkripsi end-to-end per koneksi, rotasi kunci.
* Windows: tray app, Radial Drop Menu, jendela Settings, autostart.
* Android: foreground service, Share Sheet, Quick Settings Tile, aksi notifikasi, auto-capture opsional (Accessibility).

### 1.2 Tidak termasuk (non-goals v1)
* Linux, macOS, iOS.
* Sinkronisasi via internet / relay / NAT traversal.
* Rich text / HTML clipboard (hanya teks polos).
* Clipboard berisi file (`CF_HDROP` lewat Ctrl+C di Explorer) — file dikirim lewat Radial Menu / Share Sheet.
* UI riwayat clipboard sendiri di Windows (pakai `Win + V`).
* Distribusi Play Store / Microsoft Store.
* Delayed rendering clipboard Windows.
* Lebih dari satu grup per perangkat.

---

## 2. Keputusan Teknologi (final)

| Area | Pilihan | Catatan |
|---|---|---|
| Windows | **Rust** (stable, edisi 2021), target `x86_64-pc-windows-msvc` | CRT statis → satu `.exe` tanpa VC++ redist |
| Android | **Kotlin**, Jetpack Compose, coroutines | `minSdk 29`, `targetSdk`/`compileSdk` = API stabil terbaru |
| Kripto Rust | `x25519-dalek`, `chacha20poly1305`, `hkdf`, `hmac`, `sha2`, `subtle`, `zeroize`, `rand` | RustCrypto |
| Kripto Android | **BouncyCastle** `bcprov-jdk18on` (X25519, ChaCha20-Poly1305, HKDF) + JCA (`SHA-256`, `HmacSHA256`) | Murni Java, tanpa Play Services |
| JSON | Rust `serde_json`; Kotlin `kotlinx.serialization` | |
| mDNS | Rust `mdns-sd`; Android `NsdManager` | |
| Async Windows | `tokio` (multi-thread) | Thread Win32 terpisah untuk UI/clipboard |
| Gambar Windows | crate `image` (fitur `png`, `bmp`, `jpeg`) | DIB ↔ PNG |
| Dialog folder Windows | crate `rfd` (`IFileOpenDialog` native) | Dipakai di proses Settings saja |
| QR | Rust `qrcode` (render); Android `com.journeyapps:zxing-android-embedded` (scan) + `com.google.zxing:core` (render) | Tanpa ML Kit |
| Settings UI Windows | `eframe`/`egui`, **proses terpisah** (`transclip.exe --settings`) | RAM daemon tetap kecil |
| IPC Windows | Named pipe `\\.\pipe\transclip-<SID user>`, JSON per baris | |
| Bluetooth Windows | Winsock `AF_BTH` + `WSASetService` (SDP) | Fallback: WinRT `RfcommServiceProvider` (ditentukan di spike S4) |
| Logging | Rust `tracing` + `tracing-appender` (rolling harian, simpan 7 file); Android `Log` + file log berputar di `filesDir/logs` | Tidak pernah log isi clipboard |

Versi crate/library: pakai versi stabil terbaru saat task dikerjakan, kunci di `Cargo.lock` / version catalog Gradle (`gradle/libs.versions.toml`). Catat versi utama di `docs/DECISIONS.md`.

---

## 3. Struktur Repository

```
TransClip/
├── CLAUDE.md                     aturan kerja untuk agent
├── TODO.md                       daftar tugas berurutan (sumber kebenaran progres)
├── blueprint_aplikasi_...md      visi produk
├── README.md                     (dibuat di P8)
├── .gitignore
├── docs/
│   ├── PLAN.md                   dokumen ini
│   ├── PROTOCOL.md               spesifikasi kabel (normatif)
│   ├── DECISIONS.md              log keputusan (ADR ringkas)
│   ├── spikes/                   laporan hasil spike Fase 0
│   ├── test-vectors/             *.json dari tvgen
│   └── manual-tests.md           skenario uji manual + hasil
├── windows/                      Cargo workspace
│   ├── Cargo.toml                [workspace]
│   ├── .cargo/config.toml        crt-static
│   ├── crates/
│   │   ├── proto/                framing, record layer, handshake, pesan, validasi path (murni, tanpa Win32)
│   │   ├── engine/               peer manager, discovery, transfer, antrian (tokio, tanpa Win32 UI)
│   │   └── tvgen/                generator test vector (bin)
│   └── app/                      transclip.exe: main, tray, clipboard, radial, bluetooth, ipc, settings(egui)
│       ├── build.rs              embed manifest + ikon
│       ├── transclip.manifest
│       └── assets/icon.ico
├── android/                      Gradle project
│   ├── settings.gradle.kts
│   ├── gradle/libs.versions.toml
│   ├── proto/                    modul JVM murni (framing, crypto, pesan, validasi) — unit test di JVM
│   └── app/                      aplikasi Android
└── Clipdeck/                     REFERENSI SAJA, read-only, di-.gitignore
```

Pemisahan `proto` / `engine` vs `app` membuat logika inti bisa diuji dengan `cargo test` / JVM test tanpa perangkat.

---

## 4. Arsitektur Logika (berlaku di kedua platform)

```
           ┌──────────────── Platform layer ───────────────┐
           │ ClipboardWatcher   ClipboardWriter   UI/Tray  │
           │ FileSource (Radial / ShareSheet)  Notifier    │
           └───────▲──────────────────┬──────────────▲─────┘
                   │ events            │ commands     │ status
           ┌───────┴──────────────────▼──────────────┴─────┐
           │                    Engine                     │
           │  ClipboardSync   TransferManager   PeerManager │
           │  Dedupe/Conflict  OfflineQueue     Discovery   │
           └───────▲──────────────────┬────────────────────┘
                   │                  │
           ┌───────┴──────────────────▼────────────────────┐
           │ Link (per peer per transport)                 │
           │ Handshake → RecordLayer → FrameCodec          │
           │ TcpTransport          RfcommTransport         │
           └───────────────────────────────────────────────┘
```

### 4.1 Komponen Engine

| Komponen | Tanggung jawab |
|---|---|
| `Discovery` | mDNS register + browse, UDP beacon kirim/terima, cache alamat. Menghasilkan event `PeerSeen {device_id, name, addrs, platform}` |
| `PeerManager` | Satu `Peer` per `device_id`: status (`Offline/Connecting/Online(LAN|BT)/Error`), backoff, aturan dial §5 protokol, memilih link terbaik, menutup duplikat |
| `Link` | Satu koneksi terautentikasi: loop baca, antrian tulis berprioritas (kontrol > chunk), ping/pong, timeout |
| `ClipboardSync` | Menerima `LocalClip` dari platform → filter sensitif → hash → dedupe → kirim ke semua peer online (atau antrian offline). Menerima `text/image` dari peer → dedupe/konflik → `ClipboardWriter` → `ack` |
| `TransferManager` | Batch keluar/masuk, `file_offer/accept`, kanal file, resume, hash, kuota, progres |
| `OfflineQueue` | ≤5 item per peer, dikirim setelah `hello` |
| `SeenCache` | LRU `item_id` (1024 / 10 menit) |

### 4.2 Pemilihan transport (mode Priority/Lazy)

1. Jika peer punya link LAN `Online` → pakai LAN.
2. Jika LAN ke peer itu mati **> 5 detik** **dan** peer ditandai *prioritas Bluetooth* **dan** sudah paired di OS → buka link BT.
3. Saat LAN pulih dan stabil **60 detik** → tutup link BT (`bye reason:"lan_restored"`).
4. Maksimal **3** link BT aktif bersamaan.
5. Gambar lewat BT: jika ukuran ≤ `bt_inline_image_max` (default 2 MiB) kirim langsung, selain itu kirim `announce`; penerima menampilkan notifikasi "Gambar tersedia — ketuk untuk ambil" yang memicu `pull`.
6. File lewat BT: diizinkan, tetapi UI memperingatkan bila > 20 MiB (estimasi waktu ditampilkan).

### 4.3 Siklus hidup item clipboard keluar

```
OS clipboard berubah
  → baca (dengan retry) → apakah ditandai rahasia? ya → STOP
  → apakah bertanda asal TransClip (echo)? ya → STOP
  → normalisasi (teks: apa adanya; DIB → PNG; gambar lain: byte asli)
  → hash sama dengan item terakhir? ya → STOP
  → buat item {id, origin=self, ts=now, hash}
  → untuk setiap peer: Online → kirim; Offline → OfflineQueue
  → tunggu ack (15 s, retry 1×) → status di UI
```

### 4.4 Siklus hidup item clipboard masuk

```
frame text/image diterima
  → id di SeenCache? ya → ack ok, STOP
  → hash == hash clipboard sekarang? ya → ack ok, STOP
  → ts lebih lama dari item terakhir yang diterapkan? ya → ack ok, STOP
  → tulis ke clipboard OS dengan penanda asal TransClip
  → simpan ke riwayat (Android) / cache gambar
  → ack ok
```

### 4.5 Folder tujuan transfer file (dapat diubah)

Berlaku untuk **file/folder** yang diterima. Gambar clipboard tidak terpengaruh (Android: `Pictures/TransClip`, Windows: cache).

| | Windows | Android |
|---|---|---|
| **Default** | Folder Downloads milik pengguna (Known Folder `FOLDERID_Downloads`, sehingga ikut bila pengguna memindahkan Downloads) + `\TransClip\` | `Download/TransClip/` via `MediaStore.Downloads` (tanpa izin) |
| **Ubah** | Settings → Umum → *Folder tujuan* → **Ubah…** membuka dialog pilih folder native (`IFileOpenDialog` + `FOS_PICKFOLDERS`, via crate `rfd`) | Settings → *Folder tujuan* → **Ubah…** membuka pemilih folder sistem (SAF `ACTION_OPEN_DOCUMENT_TREE`), lalu `takePersistableUriPermission` (read + write) agar izin bertahan setelah reboot |
| **Disimpan sebagai** | `download_dir` di `config.toml` (path absolut; kosong = default) | `download_tree_uri` di DataStore (kosong = default) |
| **Penulisan** | `std::fs` dengan prefix long path `\\?\` | Default: `MediaStore.Downloads` + `RELATIVE_PATH`. Kustom: `DocumentFile.fromTreeUri` → buat subfolder & file sesuai `relative_path` |

Aturan bersama:
* **Validasi saat memilih:** folder harus ada dan bisa ditulis (buat lalu hapus file uji `.transclip-write-test`). Gagal → tampilkan pesan, pilihan tidak disimpan.
* **Tombol di UI:** path saat ini (Android: nama folder yang mudah dibaca dari `DocumentFile.name` / path tree), **Ubah…**, **Reset ke default**, **Buka folder** (Windows: Explorer; Android: intent `ACTION_VIEW` ke folder bila didukung, jika tidak buka app Files).
* **Struktur di dalam folder tujuan:** file tunggal → langsung di akar folder tujuan; folder → `<folder tujuan>/<batch_name>/<relative_path>`. Konflik nama → ` (1)`, ` (2)`, ….
* **File sementara** `.transclip-part` ditulis di folder tujuan yang sama (agar rename atomik di volume yang sama).
* **Cek ruang disk** memakai volume folder tujuan (Windows: `GetDiskFreeSpaceExW`; Android: `StorageStatsManager` / `StatFs` untuk default; SAF kustom: lewati cek bila tidak bisa ditentukan).
* **Fallback:** jika saat transfer folder kustom tidak tersedia (drive USB dicabut, folder dihapus, izin SAF dicabut) → simpan ke **default**, kirim notifikasi "Folder tujuan tidak tersedia, file disimpan di <default>", dan tampilkan peringatan di Settings. Setting kustom **tidak** dihapus otomatis (drive mungkin dipasang lagi).
* Perubahan folder hanya berlaku untuk transfer **berikutnya**; transfer yang sedang berjalan/resume tetap di folder lamanya.

---

## 5. Windows — Desain Detail

### 5.1 Proses & thread

| Proses | Isi |
|---|---|
| `transclip.exe` (daemon) | Thread utama: message loop Win32 (tray, clipboard listener window, radial overlay, hook). Runtime tokio di thread pool terpisah untuk engine. Komunikasi antar keduanya lewat channel (`tokio::sync::mpsc` + `PostMessageW` ke HWND untuk membangunkan UI thread) |
| `transclip.exe --settings` | Jendela egui. Bicara ke daemon lewat named pipe. Ditutup penuh saat jendela ditutup |

* **Single instance:** named mutex `Local\TransClip-Daemon`. Instance kedua (tanpa argumen) mengirim perintah `show_settings` lewat pipe lalu keluar.
* **Argumen CLI:** `--settings`, `--pair "<uri>"` (gabung grup dari kode), `--quit`, `--log-level <lvl>`.
* **Manifest:** `PerMonitorV2` DPI awareness, Common Controls v6, `asInvoker`.
* `#![windows_subsystem = "windows"]` di build release (tanpa konsol).

### 5.2 Lokasi data

| Data | Lokasi |
|---|---|
| Konfigurasi | `%APPDATA%\TransClip\config.toml` |
| Rahasia (GK, device_id) | `%APPDATA%\TransClip\secrets.bin`, dienkripsi **DPAPI** (`CryptProtectData`, scope user) |
| Daftar peer | `%APPDATA%\TransClip\peers.json` |
| Log | `%LOCALAPPDATA%\TransClip\logs\transclip.YYYY-MM-DD.log` |
| Cache gambar | `%LOCALAPPDATA%\TransClip\cache\` (dibersihkan saat start, item > 24 jam) |
| Folder tujuan file | Default `<FOLDERID_Downloads>\TransClip\`, dapat diubah (§4.5) |

`config.toml` (default):

```toml
device_name = "<nama komputer>"
port = 47820
paused = false
sync_text = true
sync_images = true
receive_files = true
download_dir = ""            # kosong = Downloads\TransClip (lihat §4.5)
autostart = true
radial_trigger = "RControl"  # nama virtual key
radial_radius_px = 140
bt_priority_peers = []       # device_id
bt_inline_image_max = 2097152
max_incoming_file_bytes = 0  # 0 = tanpa batas
log_level = "info"
```

### 5.3 Clipboard

* **Listener:** jendela message-only (`HWND_MESSAGE`) + `AddClipboardFormatListener`; tangani `WM_CLIPBOARDUPDATE`. Debounce **150 ms**; abaikan bila `GetClipboardSequenceNumber` tidak berubah.
* **OpenClipboard:** retry 10× dengan jeda 20 ms; gagal → log `warn`, lewati.
* **Format yang dibaca:**
  1. Teks: `CF_UNICODETEXT`.
  2. Gambar (urutan): format terdaftar `PNG` → byte langsung; `CF_DIBV5` → konversi ke PNG; `CF_DIB` → konversi ke PNG. Jika clipboard berisi teks **dan** gambar, kirim gambar bila `sync_images`, kecuali sumbernya teks saja.
* **Deteksi rahasia (lewati item):** ada format terdaftar `ExcludeClipboardContentFromMonitorProcessing`; atau `CanIncludeInClipboardHistory` bernilai DWORD 0; atau `Clipboard Viewer Ignore`.
* **Penanda echo:** saat menulis, tambahkan format terdaftar `TransClipOrigin` berisi `item_id` UTF-8. Listener melihat format ini → lewati.
* **Menulis:** teks → `CF_UNICODETEXT`; gambar → `PNG` + `CF_DIBV5` (dari decode); tambahkan `CanUploadToCloudClipboard` = DWORD 0 dan `TransClipOrigin`. Jangan set `CanIncludeInClipboardHistory` (agar masuk `Win + V`).
* **Alpha:** saat DIB→PNG, hormati `BI_BITFIELDS` dan masker alpha di `BITMAPV5HEADER`; untuk DIB 32-bit tanpa alpha valid (semua 0), anggap opaque.

### 5.4 Radial Drop Menu

* **Hook:** `WH_MOUSE_LL` + `WH_KEYBOARD_LL` di thread UI. Callback hook WAJIB cepat (< 1 ms): hanya set state dan `PostMessageW`.
* **Aktivasi:** tombol kiri mouse tertahan **dan** sudah bergerak > `SM_CXDRAG`/`SM_CYDRAG` **dan** tombol pemicu (default **Right Ctrl**, dapat diubah) ditekan → tampilkan overlay berpusat di kursor.
* **Overlay:** jendela `WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE`, digambar dengan `UpdateLayeredWindow` (bitmap premultiplied alpha). **Jangan** pakai `WS_EX_TRANSPARENT` (overlay harus menerima drop). Piksel ring alpha ≥ 1 agar hit-test kena; bagian di luar ring alpha 0.
* **Drop target:** `RegisterDragDrop` dengan implementasi `IDropTarget` (COM, `OleInitialize` di thread UI). `DragOver` → hitung sudut dari pusat → highlight segmen; efek `DROPEFFECT_COPY`. `Drop` → ambil `CF_HDROP` → `DragQueryFileW` → kirim batch ke peer segmen (atau semua). Pusat lingkaran (radius < 40 px) = batal.
* **Segmen:** N peer online + 1 "Kirim ke Semua". Bila N > 7: 6 peer pertama (urut terakhir dipakai) + "Lainnya…" yang membuka sub-ring. Setiap segmen menampilkan ikon platform + nama (dipotong 14 karakter).
* **Sembunyikan** saat: drop, `DragLeave` > 300 ms, tombol mouse dilepas di luar overlay, `Esc`.
* **Ukuran:** radius 140 px × skala DPI monitor tempat kursor berada.
* **Batasan yang didokumentasikan:** drag dari aplikasi elevated (Admin) tidak bisa diterima (UIPI).
* **Cadangan:** jika spike S3 gagal, ganti aktivasi menjadi entri `SendTo` (`%APPDATA%\Microsoft\Windows\SendTo\TransClip.lnk` → `transclip.exe --send <files>`) yang membuka popup pemilih perangkat. Fitur `--send` tetap diimplementasikan sebagai jalur alternatif apa pun hasil spike.

### 5.5 Tray

* `Shell_NotifyIconW` dengan ikon status: normal / paused / ada peer error.
* Menu: status (n perangkat online), *Kirim file…* (dialog buka file), *Jeda sinkronisasi*, *Pengaturan*, *Pairing…*, *Buka folder unduhan*, *Keluar*.
* Notifikasi: balloon `NIF_INFO` (tampil sebagai toast di Windows 11) untuk: file diterima, transfer gagal, perangkat baru bergabung, gambar BT tersedia.
* Tangani `TaskbarCreated` (Explorer restart) → daftarkan ulang ikon.

### 5.6 Settings (egui, proses terpisah)

Tab:
1. **Perangkat** — daftar peer: nama, platform, status, transport, terakhir terlihat; tombol *Lupakan*; toggle *Prioritas Bluetooth*.
2. **Pairing** — *Buat grup baru*, *Tampilkan QR* (berlaku 2 menit, hitung mundur), *Salin kode*, *Gabung dengan kode* (textbox), *Ganti Group Key* (konfirmasi).
3. **Umum** — nama perangkat, autostart, jeda, sinkron teks/gambar, terima file, **Folder tujuan** (path + *Ubah…* / *Reset ke default* / *Buka folder*, §4.5).
4. **Radial menu** — tombol pemicu (rekam tombol), radius, tombol *Uji*.
5. **Bluetooth** — status adaptor, daftar perangkat paired OS, batas inline gambar.
6. **Lanjutan** — port, level log, *Buka folder log*, versi, device_id.

IPC (named pipe, JSON per baris, request `{id, method, params}` → response `{id, result|error}`; daemon juga push `{event, data}`):
`get_status`, `get_config`, `set_config`, `list_peers`, `forget_peer`, `set_bt_priority`, `create_group`, `get_pairing_uri`, `join_group`, `rotate_key`, `set_paused`, `open_log_dir`, `quit`; event `status_changed`, `peers_changed`, `transfer_progress`.

Pipe dibuat dengan security descriptor yang hanya mengizinkan SID user saat ini.

### 5.7 Autostart, firewall, build

* Autostart: `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\TransClip = "<path exe>"`.
* Firewall: listen pertama memicu dialog Windows Defender Firewall. Dokumentasikan agar pengguna memilih **Private network**. Tidak menambah rule otomatis (butuh Admin).
* Build: `cargo build --release -p transclip` dengan `.cargo/config.toml`:
  ```toml
  [target.x86_64-pc-windows-msvc]
  rustflags = ["-C", "target-feature=+crt-static"]
  ```
  Profil release: `lto = "thin"`, `codegen-units = 1`, `opt-level = 3`, `strip = true`, `panic = "abort"`.
* Panic hook: tulis ke log, tampilkan MessageBox singkat, keluar dengan kode ≠ 0.
* Target: ukuran `.exe` < 15 MB, RAM idle daemon < 30 MB (Working Set), CPU idle ≈ 0%.

---

## 6. Android — Desain Detail

### 6.1 Paket & modul

* Package: `io.transclip.app` (ubah bila pengguna punya domain).
* Modul `:proto` (JVM murni, Kotlin): `Frame`, `FrameCodec`, `RecordLayer`, `Handshake`, `Messages` (data class `@Serializable`), `PathValidator`, `Beacon`, `Pairing` (parse/build URI). 100% unit test JVM.
* Modul `:app`:

| Paket | Isi |
|---|---|
| `service` | `SyncService` (foreground), `BootReceiver`, `NetworkMonitor` |
| `engine` | `PeerManager`, `Link`, `ClipboardSync`, `TransferManager`, `OfflineQueue`, `SeenCache` |
| `net` | `TcpServer`, `TcpDialer`, `NsdDiscovery`, `BeaconDiscovery`, `RfcommServer`, `RfcommDialer` |
| `clip` | `ClipboardWriter`, `ClipCaptureActivity` (transparan), `CaptureAccessibilityService`, `SendClipTileService` |
| `share` | `ShareReceiverActivity` |
| `storage` | `SecretStore` (Keystore), `SettingsRepo` (DataStore), `HistoryRepo` (Room atau file JSON), `MediaWriter` |
| `ui` | Compose: `OnboardingScreen`, `HomeScreen`, `PairingScreen`, `HistoryScreen`, `TransfersScreen`, `SettingsScreen`, `DevicePickerSheet` |

* State bersama: `AppState` (singleton) berisi `StateFlow` untuk status link, peer, riwayat, transfer — pola `ClipStore` milik Clipdeck.

### 6.2 Manifest & izin

| Izin | Alasan |
|---|---|
| `INTERNET`, `ACCESS_NETWORK_STATE`, `ACCESS_WIFI_STATE` | LAN |
| `CHANGE_WIFI_MULTICAST_STATE` | `MulticastLock` untuk mDNS/beacon, sekaligus prasyarat FGS `connectedDevice` |
| `BLUETOOTH` (`maxSdkVersion=30`), `BLUETOOTH_CONNECT` | RFCOMM ke perangkat paired |
| `FOREGROUND_SERVICE`, `FOREGROUND_SERVICE_CONNECTED_DEVICE` | Service latar |
| `POST_NOTIFICATIONS` | Notifikasi (API 33+) |
| `RECEIVE_BOOT_COMPLETED` | Mulai otomatis |
| `REQUEST_IGNORE_BATTERY_OPTIMIZATIONS` | Minta pengecualian Doze |
| `CAMERA` | Scan QR |

* **Jangan** minta `BLUETOOTH_SCAN` (tidak scan). **Jangan** memanggil `cancelDiscovery()` (pelajaran Clipdeck).
* `SyncService`: `android:foregroundServiceType="connectedDevice"`, `exported=false`.
* `CaptureAccessibilityService`: `BIND_ACCESSIBILITY_SERVICE`, config XML dengan event types minimum, `canRetrieveWindowContent=false` bila memungkinkan (ditentukan spike S2).
* `allowBackup=false`, `usesCleartextTraffic` tidak relevan (raw socket).

### 6.3 Penyimpanan

| Data | Lokasi |
|---|---|
| GK, device_id | `SecretStore`: kunci AES-GCM di Android Keystore membungkus GK; ciphertext di DataStore |
| Setting | Preferences DataStore |
| Peer | DataStore (JSON) |
| Riwayat (≤100 item) | Room atau file JSON di `filesDir` |
| Gambar diterima | `MediaStore.Images` → `Pictures/TransClip/` (`IS_PENDING` 1 → 0) |
| File diterima | Default `MediaStore.Downloads` → `Download/TransClip/…` (tanpa izin di API 29+). Dapat diubah ke folder mana pun via SAF (§4.5) |
| Log | `filesDir/logs/` berputar 1 MB × 5 |

### 6.4 Clipboard

* **Menulis (dari peer):** `ClipboardManager.setPrimaryClip` di main thread. Teks → `ClipData.newPlainText`; gambar → simpan MediaStore lalu `ClipData.newUri`. Set `ClipDescription.extras` (PersistableBundle) `io.transclip.origin=<item_id>` sebagai penanda echo.
* **Membaca (ke peer) — tiga jalur:**
  1. **Manual (selalu ada):** `ClipCaptureActivity` — Activity transparan tanpa UI; baca clipboard di `onWindowFocusChanged(true)`, kirim, `finish()`. Dipicu oleh: Quick Settings Tile *Kirim clipboard*, aksi notifikasi *Kirim clipboard*, shortcut launcher.
  2. **Share Sheet:** teks/gambar/file yang dibagikan ke TransClip.
  3. **Auto-capture (opsional, default mati):** `CaptureAccessibilityService` mendeteksi kemungkinan aksi salin (event dan heuristik dari spike S2), debounce 300 ms, lalu menampilkan jendela `TYPE_ACCESSIBILITY_OVERLAY` 1×1 yang **focusable** sesaat untuk membaca clipboard, lalu menghapusnya. Rate-limit: maksimal 1 baca / 700 ms. Wajib ada layar penjelasan sebelum pengguna mengaktifkan Accessibility.
* Sebelum mengirim: cek `ClipDescription.EXTRA_IS_SENSITIVE` (API 33+) dan penanda echo.
* Gambar dari clipboard/share: baca byte asli dari URI (`ContentResolver.openInputStream`), `mime` dari `getType`; jangan re-encode.

### 6.5 Service & jaringan

* `SyncService` dimulai saat app dibuka, saat boot (bila diaktifkan), dan dari tile/notifikasi. Notifikasi ongoing `IMPORTANCE_LOW`: judul status (`n perangkat terhubung`), aksi **Kirim clipboard**, **Jeda**, **Putuskan**.
* `MulticastLock` dipegang selama service aktif dan Wi-Fi tersambung.
* `ConnectivityManager.NetworkCallback` memicu beacon + rediscovery saat jaringan berubah.
* Semua I/O di `Dispatchers.IO`, satu coroutine `SupervisorJob` per link.
* Doze: onboarding meminta pengecualian optimasi baterai, menjelaskan alasannya; tetap berfungsi (dengan delay) bila ditolak.

### 6.6 Share Sheet & pemilih perangkat

* `ShareReceiverActivity` menerima `ACTION_SEND` / `ACTION_SEND_MULTIPLE` untuk `text/*`, `image/*`, `*/*`.
* Tampilkan `DevicePickerSheet` (multi-pilih, peer online di atas, opsi *Semua*).
* Teks/gambar tunggal → dikirim sebagai item clipboard; selain itu → batch file.
* Direct Share: `ShortcutManagerCompat.pushDynamicShortcut` untuk 4 peer yang paling sering dipakai.

### 6.7 Build & rilis

* `./gradlew :proto:test :app:testDebugUnitTest :app:assembleRelease`.
* Signing release dari `android/keystore.properties` (tidak di-commit; ada contoh `keystore.properties.example`).
* R8 aktif; aturan keep untuk kelas `@Serializable` dan BouncyCastle yang dipakai.
* Target: APK < 15 MB, RAM service < 60 MB.

---

## 7. UX Alur Utama

### 7.1 Pertama kali (perangkat pertama, Windows)
1. Jalankan `transclip.exe` → tray muncul → Settings terbuka di tab Pairing.
2. *Buat grup baru* → masukkan nama grup → QR + kode ditampilkan.

### 7.2 Menambah Android
1. Instal APK → Onboarding: notifikasi → nearby devices → baterai → (opsional) auto-capture.
2. *Scan QR* → simpan GK → service mulai → terhubung via LAN dalam < 5 detik.

### 7.3 Menambah Windows lain
* Paste kode di *Gabung dengan kode*, atau jalankan `transclip.exe --pair "<uri>"`.

### 7.4 Android ↔ Android
* Android A: *Pairing → Tampilkan QR*; Android B: *Scan QR*.

---

## 8. Strategi Pengujian

| Level | Isi | Alat |
|---|---|---|
| Unit | proto: framing, record, handshake, beacon, pairing URI, validasi path, dedupe, konflik | `cargo test`, JUnit (`:proto:test`) |
| Interop | Kedua implementasi mencocokkan `docs/test-vectors/*.json` | Sama |
| Integrasi engine | Dua engine Rust di localhost saling sync teks, gambar, file, resume | `cargo test -p engine` |
| Interop live | Engine Rust ↔ engine Kotlin (JVM) di localhost, tanpa perangkat | Test JVM yang menjalankan biner `windows/target/.../interop-peer` (dibuat di P5) |
| Fuzz | Parser frame/record/beacon dengan input acak | `proptest` (Rust), test acak JUnit |
| Manual | Skenario di `docs/manual-tests.md` | Pengguna, perangkat asli |

Semua test otomatis WAJIB hijau sebelum task ditandai selesai.

---

## 9. Risiko & Mitigasi

| Risiko | Dampak | Mitigasi |
|---|---|---|
| Auto-capture Android tidak andal/mengganggu IME | Sinkron Android → PC tidak otomatis | Default mati; jalur manual (tile, notifikasi, share) selalu ada; spike S2 |
| Gboard tidak menampilkan gambar dari `ClipData.newUri` | Gambar tidak di riwayat Gboard | Spike S1; gambar tetap ada di clipboard sistem & riwayat app |
| Overlay radial tidak menerima drop dari Explorer | Fitur unggulan Windows gagal | Spike S3; cadangan `SendTo` + `--send` |
| Winsock RFCOMM / SDP bermasalah | BT Windows gagal | Spike S4; cadangan WinRT; BT adalah fitur cadangan, bisa ditunda ke v1.1 |
| AP Isolation / mDNS diblokir | Tidak saling menemukan | Beacon, cache IP, IP di QR, input manual, BT |
| SmartScreen memperingatkan `.exe` tak bertanda tangan | Kepercayaan pengguna | Dokumentasikan; opsional code signing di P8 |
| Firewall memblokir port | Tidak terhubung | Dokumentasi + deteksi (tidak ada koneksi masuk) → tampilkan petunjuk |
| Lisensi GPL Clipdeck | Konflik lisensi | Hanya ambil ide/pola, tidak menyalin kode |

---

## 10. Definition of Done v1.0

* Semua task di `TODO.md` P0–P8 tercentang, kecuali yang ditandai `(opsional)`.
* Semua test otomatis hijau di kedua platform.
* Semua skenario di `docs/manual-tests.md` lulus di minimal: 1 PC Windows 11 + 2 Android (berbeda merek).
* Metrik: latensi teks LAN < 300 ms (p95), gambar 3 MB LAN < 2 s, RAM idle Windows < 30 MB, tidak ada crash dalam uji 24 jam.
* README dan panduan pengguna tersedia, `.exe` dan `.apk` release terbangun dari perintah yang terdokumentasi.
