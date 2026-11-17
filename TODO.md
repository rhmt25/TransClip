# TransClip — TODO (siap dikerjakan)

Urutan task = urutan pengerjaan. Kerjakan **satu task sampai selesai** sebelum lanjut, kecuali dinyatakan boleh paralel.
Baca `CLAUDE.md` dulu. Spesifikasi: `docs/PLAN.md` (arsitektur) dan `docs/PROTOCOL.md` (kabel, normatif).

**Legenda**
* `[ ]` belum · `[~]` sedang dikerjakan · `[x]` selesai · `[!]` terblokir (tulis alasannya di bawah task)
* **[MANUAL]** butuh tindakan pengguna (instal, perangkat asli, izin). Agent menyiapkan semuanya, lalu meminta pengguna menjalankan langkahnya dan melaporkan hasil.
* **Dep** = task yang harus selesai lebih dulu.
* **Selesai jika** = kriteria penerimaan; semua harus terpenuhi.

**Progres:** P0 ◐ (spike & Android SDK menunggu pengguna) · P1 ◐ (sisi Rust selesai; Kotlin menunggu) · P2 ☐ · P3 ☐ · P4 ☐ · P5 ☐ · P6 ☐ · P7 ☐ · P8 ☐

---

## P0 — Persiapan & Spike Risiko

### [x] P0-01 · Inisialisasi repository
* **Lakukan:**
  * `git init`, branch `main`.
  * `.gitignore`: `Clipdeck/`, `windows/target/`, `android/.gradle/`, `android/build/`, `android/**/build/`, `android/local.properties`, `android/keystore.properties`, `*.jks`, `*.keystore`, `.idea/`, `*.iml`, `.DS_Store`, `Thumbs.db`.
  * Buat folder kosong dengan `.gitkeep`: `docs/spikes/`, `docs/test-vectors/`.
  * Buat `docs/manual-tests.md` berisi judul + tabel kosong (kolom: ID, Skenario, Langkah, Hasil yang diharapkan, Hasil, Tanggal).
  * Commit awal berisi dokumen yang sudah ada.
* **Selesai jika:** `git status` bersih; `Clipdeck/` tidak ter-track.

### [x] P0-02 · Toolchain Windows (Rust) [MANUAL]
* **Lakukan:**
  * Minta pengguna memasang: `rustup` (toolchain `stable-x86_64-pc-windows-msvc`) dan **Visual Studio Build Tools** (workload "Desktop development with C++", Windows 11 SDK).
  * Verifikasi: `rustc --version`, `cargo --version`, `rustup show`.
  * Catat versi di `docs/DECISIONS.md`.
* **Selesai jika:** `cargo new --bin hello` di folder scratch berhasil `cargo run`.

### [~] P0-03 · Toolchain Android [MANUAL]
* **Lakukan:**
  * Minta pengguna memasang Android Studio (atau command-line tools) + JDK 17 (bawaan Android Studio), SDK Platform API terbaru, Build-Tools, Platform-Tools.
  * Set `ANDROID_HOME` dan tambahkan `platform-tools` ke PATH.
  * Minta pengguna mengaktifkan USB debugging di ≥1 HP dan memverifikasi `adb devices`.
* **Selesai jika:** `adb devices` menampilkan perangkat; versi dicatat di `docs/DECISIONS.md`.
* **Status (2026-11-18):** cmdline-tools + JDK 23 ada; `sdkmanager --licenses` BELUM diterima. Pengguna perlu menjalankan sendiri: `! & "$env:LOCALAPPDATA\Android\Sdk\cmdline-tools\latest\bin\sdkmanager.bat" --sdk_root="$env:LOCALAPPDATA\Android\Sdk" --licenses` lalu `platform-tools`, `platforms;android-35`, `build-tools;35.0.0`. Rust 1.100.0 + VS Build Tools 2022 (MSVC) sudah terpasang.


### [x] P0-04 · Kerangka workspace Rust
* **Dep:** P0-02
* **Lakukan:** buat struktur `windows/` sesuai PLAN §3: workspace dengan member `crates/proto`, `crates/engine`, `crates/tvgen`, `app`. `.cargo/config.toml` (crt-static), profil release (PLAN §5.7), `rust-toolchain.toml` (channel stable), `rustfmt.toml`, lint `clippy` di `[workspace.lints]` (`unwrap_used = "warn"` di non-test).
* **Selesai jika:** `cargo build --workspace` dan `cargo clippy --workspace -- -D warnings` lulus; `app` menghasilkan `transclip.exe` yang langsung keluar.

### [ ] P0-05 · Kerangka proyek Android
* **Dep:** P0-03
* **Lakukan:** buat `android/` dengan Gradle Kotlin DSL, version catalog `gradle/libs.versions.toml`, modul `:proto` (plugin `kotlin("jvm")` + serialization) dan `:app` (Compose, minSdk 29). Gradle wrapper ikut di-commit. Aktivitas kosong "TransClip".
* **Selesai jika:** `./gradlew :proto:test :app:assembleDebug` lulus; APK terinstal dan terbuka di HP (**[MANUAL]** konfirmasi pengguna).

### [ ] P0-06 · Spike S1 — Gambar ke clipboard Android & Gboard [MANUAL]
* **Dep:** P0-05
* **Lakukan:** di branch `spike/s1`, tombol "Tulis gambar uji": simpan PNG bawaan ke `MediaStore` (`Pictures/TransClip`, `IS_PENDING`) lalu `setPrimaryClip(ClipData.newUri(...))` **dari foreground service** (bukan Activity). Minta pengguna membuka Gboard → ikon clipboard.
* **Selesai jika:** `docs/spikes/S1-gboard-image.md` berisi: merek/versi HP, versi Gboard, apakah gambar muncul, apakah teks muncul, kesimpulan. Hasil negatif tetap valid; update PLAN §9 bila perlu.

### [ ] P0-07 · Spike S2 — Auto-capture clipboard Android [MANUAL]
* **Dep:** P0-05
* **Lakukan:** di branch `spike/s2`:
  * `AccessibilityService` yang mencatat event (type, package, className, text) ke log saat pengguna menyalin teks di Chrome, WhatsApp, Gboard (salin dari papan ketik), dan app catatan.
  * Uji pembacaan via jendela `TYPE_ACCESSIBILITY_OVERLAY` 1×1 focusable dan via `ClipCaptureActivity` transparan.
  * Ukur: apakah pembacaan berhasil, apakah keyboard tertutup/teks yang sedang diketik terganggu, latensi.
* **Selesai jika:** `docs/spikes/S2-auto-capture.md` memuat tabel per app × metode, heuristik event final yang dipilih, dan keputusan (pakai / tidak / hanya untuk event tertentu). Keputusan dicatat di `DECISIONS.md`.

### [ ] P0-08 · Spike S3 — Radial overlay menerima drop dari Explorer [MANUAL]
* **Dep:** P0-04
* **Lakukan:** biner contoh `windows/app/examples/radial_spike.rs`: hook `WH_MOUSE_LL` + `WH_KEYBOARD_LL`, saat drag + Right Ctrl tampilkan jendela layered (lingkaran semi-transparan) dengan `IDropTarget`; saat drop, tampilkan MessageBox daftar file.
* **Selesai jika:** `docs/spikes/S3-radial-drop.md` menjelaskan: berhasil/tidak dari Explorer, Desktop, Chrome (unduhan), aplikasi elevated; perilaku multi-monitor & DPI; keputusan aktivasi final (pemicu default).

### [ ] P0-09 · Spike S4 — RFCOMM Windows ↔ Android [MANUAL]
* **Dep:** P0-04, P0-05
* **Lakukan:** server RFCOMM di Windows (Winsock `AF_BTH` + `WSASetService` dengan UUID PROTOCOL §2) yang mengirim balik byte; klien Android `createRfcommSocketToServiceRecord`. Uji juga arah sebaliknya (Android server `listenUsingRfcommWithServiceRecord`, Windows klien) dan Android ↔ Android.
* **Selesai jika:** `docs/spikes/S4-rfcomm.md` mencatat keberhasilan tiap arah, throughput (KB/s) untuk 1 MB, dan API final Windows (Winsock vs WinRT).

### [ ] P0-10 · Review hasil spike
* **Dep:** P0-06 … P0-09
* **Lakukan:** perbarui `docs/PLAN.md` dan `TODO.md` sesuai hasil spike (mis. ubah pemicu radial, coret auto-capture, tunda BT). Hapus branch spike setelah kode penting dipindahkan/dicatat.
* **Selesai jika:** pengguna menyetujui perubahan plan (**[MANUAL]**).

---

## P1 — Protokol & Kriptografi (inti, tanpa platform)

> P1-01…P1-07 (Rust) dan P1-08…P1-12 (Kotlin) boleh paralel setelah P1-06 menghasilkan test vector.

### [x] P1-01 · Rust `proto`: tipe dasar & JSON pesan
* **Dep:** P0-04
* **File:** `windows/crates/proto/src/{lib.rs,ids.rs,msg.rs,error.rs}`
* **Lakukan:**
  * `DeviceId([u8;16])` (hex, `Ord`), `GroupId([u8;8])`, `ItemId`/`XferId`/`BatchId` (UUID string tervalidasi), helper `b64url` tanpa padding.
  * Enum `Message` dengan `#[serde(tag = "t")]` untuk **semua** pesan PROTOCOL §6 & §9 (`snake_case`), plus `Message::Unknown` untuk tipe tak dikenal. Field `size` dikelola codec, bukan oleh struct.
  * `ProtoError` (thiserror): `Limit`, `BadHeader`, `Auth`, `Io`, `Version`, `Closed`.
* **Selesai jika:** test round-trip serde untuk setiap varian; JSON tak dikenal → `Unknown`; field ekstra diabaikan.

### [x] P1-02 · Rust `proto`: codec frame
* **Dep:** P1-01
* **File:** `proto/src/frame.rs`
* **Lakukan:** `encode_frame(&Header, &[u8]) -> Vec<u8>`; decoder inkremental `FrameDecoder::push(&[u8]) -> Result<Vec<Frame>>` (tidak mengasumsikan frame utuh per pembacaan); batas header 1..=65536 dan batas payload per tipe (PROTOCOL §10); varian async `read_frame`/`write_frame` untuk `AsyncRead/AsyncWrite` (tokio).
* **Selesai jika:** test: frame terpotong per byte, banyak frame dalam satu buffer, header 0 / 65537, size melebihi batas, JSON rusak → error yang tepat; property test (`proptest`) encode→decode identik.

### [x] P1-03 · Rust `proto`: record layer
* **Dep:** P1-02
* **File:** `proto/src/record.rs`
* **Lakukan:** `RecordWriter`/`RecordReader` (ChaCha20-Poly1305, nonce `0^4 || u64 BE`, counter per arah, batas L 17..=65552), adaptor `SecureStream<T: AsyncRead+AsyncWrite>` yang mengimplementasikan `AsyncRead + AsyncWrite` sehingga codec frame bisa dipakai di atasnya. Flush memaksa record dikirim.
* **Selesai jika:** test: round-trip, tag diubah 1 bit → error, counter berurutan, record terpotong, L di luar batas.

### [x] P1-04 · Rust `proto`: handshake
* **Dep:** P1-03
* **File:** `proto/src/handshake.rs`, `proto/src/group.rs`
* **Lakukan:** `GroupKey` (zeroize on drop), `group_id(&GroupKey)`, `initiate(stream, gk) -> SecureStream`, `respond(stream, gk) -> SecureStream` persis PROTOCOL §6 (timeout 10 s, constant-time compare, tolak DH nol, `wrong_group`). Fungsi derivasi murni `derive(…)` dipisah agar bisa diuji dengan input tetap.
* **Selesai jika:** test: dua sisi di `tokio::io::duplex` berhasil dan saling bertukar frame; GK berbeda → keduanya gagal tanpa panic; `gid` salah → `wrong_group`; hs dimodifikasi → gagal; timeout bekerja.

### [x] P1-05 · Rust `proto`: beacon, pairing URI, validasi path
* **Dep:** P1-01
* **File:** `proto/src/{beacon.rs,pairing.rs,path.rs}`
* **Lakukan:** encode/decode beacon (PROTOCOL §4.2); `PairingInfo` build/parse URI (§3.1, termasuk kedaluwarsa); `validate_relative_path(&str) -> Result<Vec<String>, PathError>` (§9.3) dan `sanitize_file_name` untuk nama tujuan + penamaan ` (n)`.
* **Selesai jika:** test mencakup setiap aturan penolakan path (minimal 30 kasus), beacon dengan nama UTF-8 multi-byte, URI dengan karakter khusus di nama grup.

### [x] P1-06 · `tvgen`: generator test vector
* **Dep:** P1-02 … P1-05
* **File:** `windows/crates/tvgen/src/main.rs`
* **Lakukan:** hasilkan `docs/test-vectors/{frames,handshake,records,beacon,paths}.json` dengan input deterministik (seed tetap, kunci tetap). Tambahkan test di `proto` yang membaca file itu dan memverifikasi (agar file tidak basi).
* **Selesai jika:** `cargo run -p tvgen` menghasilkan file stabil (dijalankan 2× → diff kosong); file di-commit.

### [x] P1-07 · Rust `proto`: dedupe & konflik
* **Dep:** P1-01
* **File:** `proto/src/sync_rules.rs`
* **Lakukan:** `SeenCache` (LRU 1024, TTL 10 menit, waktu diinjeksi untuk test), `ConflictClock` (aturan `ts` + `origin`, PROTOCOL §9.2).
* **Selesai jika:** unit test kasus: duplikat, kedaluwarsa TTL, ts sama beda origin, ts mundur.

### [ ] P1-08 · Kotlin `:proto`: pesan & codec frame
* **Dep:** P0-05, P1-06
* **File:** `android/proto/src/main/kotlin/io/transclip/proto/{Ids.kt,Messages.kt,FrameCodec.kt}`
* **Lakukan:** padanan P1-01 + P1-02 (kotlinx.serialization dengan class discriminator `t`, `ignoreUnknownKeys = true`, tipe tak dikenal → `Unknown`). API blocking berbasis `InputStream/OutputStream`.
* **Selesai jika:** test JUnit lulus terhadap `frames.json`, plus kasus batas yang sama seperti P1-02.

### [ ] P1-09 · Kotlin `:proto`: record layer & handshake
* **Dep:** P1-08
* **File:** `RecordLayer.kt`, `Handshake.kt`, `GroupKey.kt`
* **Lakukan:** BouncyCastle `X25519Agreement`, `ChaCha20Poly1305` (`ChaCha7539Engine` + AEAD BC), `HKDFBytesGenerator(SHA256Digest)`, HMAC JCA; `MessageDigest.isEqual` untuk compare; `SecureInputStream`/`SecureOutputStream`.
* **Selesai jika:** test lulus terhadap `handshake.json` dan `records.json`; test handshake dua thread via `PipedInputStream/PipedOutputStream` berhasil.

### [ ] P1-10 · Kotlin `:proto`: beacon, pairing, path, dedupe
* **Dep:** P1-08
* **Selesai jika:** test lulus terhadap `beacon.json` dan `paths.json`; `SeenCache` & `ConflictClock` dengan test yang sama seperti P1-07.

### [ ] P1-11 · Tes interop langsung Rust ↔ Kotlin
* **Dep:** P1-04, P1-09
* **File:** `windows/crates/proto/examples/echo_peer.rs`, `android/proto/src/test/.../LiveInteropTest.kt`
* **Lakukan:** `echo_peer` = responder TCP di port acak (dicetak ke stdout) dengan GK dari argumen, mengembalikan setiap frame. Test Kotlin menjalankan biner itu (path dari env `TRANSCLIP_ECHO_PEER`; **skip** bila env tidak ada), handshake sebagai initiator, kirim 100 frame acak, verifikasi balasan.
* **Selesai jika:** test lulus secara lokal; dokumentasikan cara menjalankannya di `CLAUDE.md` bagian perintah.

### [ ] P1-12 · Review protokol
* **Dep:** P1-01 … P1-11
* **Lakukan:** periksa ulang implementasi vs `PROTOCOL.md` baris per baris; perbaiki ketidakcocokan (di kode atau dokumen + DECISIONS).
* **Selesai jika:** checklist kecocokan ditulis di PR/commit message; semua test hijau.

---

## P2 — Engine Bersama (Rust) & Daemon Windows

### [ ] P2-01 · Engine: Link
* **Dep:** P1-12
* **File:** `windows/crates/engine/src/link.rs`
* **Lakukan:** `Link` membungkus `SecureStream`: task baca → `mpsc` event; task tulis dengan dua antrian (kontrol prioritas tinggi, data rendah); ping 30 s, dead 90 s; `hello` exchange; penutupan bersih (`bye`). Generik atas transport (`AsyncRead+AsyncWrite+Send`).
* **Selesai jika:** test duplex: hello, ping/pong dengan waktu tokio dijeda (`tokio::time::pause`), dead timeout, frame kontrol menyela antrian data besar.

### [ ] P2-02 · Engine: Discovery
* **Dep:** P2-01
* **File:** `engine/src/discovery/{mdns.rs,beacon.rs,cache.rs}`
* **Lakukan:** register + browse `_transclip._tcp` (`mdns-sd`), filter `gid`; UDP beacon kirim tiap 10 s dan saat diminta, terima & filter (`socket2`, `SO_BROADCAST`, `SO_REUSEADDR`); cache alamat di `peers.json`. Output `Stream<PeerSeen>`.
* **Selesai jika:** test integrasi dua instans engine di localhost saling menemukan via beacon (mDNS boleh ditandai `#[ignore]` bila tidak stabil di CI).

### [ ] P2-03 · Engine: PeerManager & aturan dial
* **Dep:** P2-02
* **File:** `engine/src/peers.rs`
* **Lakukan:** state per peer, backoff (2→30 s), aturan dial `device_id` lebih kecil + fallback 30 s, penanganan duplikat, listener TCP port config, status publik via `watch` channel. Batas 10 peer (peer ke-11 ditolak dengan log + event UI).
* **Selesai jika:** test: dua engine saling terhubung tepat satu link; matikan satu → status Offline → hidupkan → reconnect; dial bersamaan → tersisa satu.

### [ ] P2-04 · Engine: ClipboardSync & OfflineQueue
* **Dep:** P2-03, P1-07
* **File:** `engine/src/clip.rs`, `engine/src/queue.rs`
* **Lakukan:** API `submit_local(LocalClip)` dan trait `ClipboardSink { fn apply(&self, RemoteClip) -> Result<()> }` yang diimplementasikan platform. Siklus PLAN §4.3/4.4, ack + retry 1×, antrian offline 5 item per peer.
* **Selesai jika:** test 3 engine di localhost: teks dari A sampai di B dan C tepat sekali; tidak ada echo; peer offline menerima item setelah online; item > 8 MiB ditolak.

### [ ] P2-05 · Daemon: kerangka app Windows
* **Dep:** P0-04
* **File:** `windows/app/src/{main.rs,single_instance.rs,paths.rs,logging.rs,config.rs,secrets.rs}`
* **Lakukan:** parsing argumen (PLAN §5.1), mutex single instance, path data (PLAN §5.2, gunakan `SHGetKnownFolderPath`), logging `tracing` rolling, `Config` (TOML, default, validasi, simpan atomik via file temp + rename), `SecretStore` DPAPI, panic hook, `build.rs` + manifest + ikon.
* **Selesai jika:** menjalankan exe 2× → instans kedua keluar; file config & log dibuat; secrets terenkripsi (tidak terbaca sebagai teks); `clippy -D warnings` lulus.

### [ ] P2-06 · Daemon: thread UI, tray & jembatan ke engine
* **Dep:** P2-05, P2-03
* **File:** `app/src/{ui_thread.rs,tray.rs,bridge.rs}`
* **Lakukan:** message loop Win32, jendela message-only, `Shell_NotifyIconW` + menu (PLAN §5.5), `TaskbarCreated`, balloon notification; runtime tokio di thread lain; `bridge.rs` = channel dua arah UI↔engine (`PostMessageW` + `WM_APP`).
* **Selesai jika:** **[MANUAL]** tray muncul, menu berfungsi (Keluar menutup bersih, Jeda mengubah ikon), restart Explorer → ikon kembali.

### [ ] P2-07 · Daemon: clipboard listener & writer
* **Dep:** P2-06, P2-04
* **File:** `app/src/clipboard/{mod.rs,read.rs,write.rs,dib.rs,formats.rs}`
* **Lakukan:** sesuai PLAN §5.3 (debounce, retry, format, rahasia, penanda echo, DIB↔PNG dengan alpha). Implementasikan `ClipboardSink`.
* **Selesai jika:** unit test `dib.rs` (DIB 24-bit, 32-bit dengan & tanpa alpha, BI_BITFIELDS → PNG → decode identik piksel); **[MANUAL]** dua PC (atau PC + `echo_peer`/engine test) — salin teks & screenshot (Win+Shift+S) → muncul di `Win + V` sisi lain; menyalin dari KeePass/Bitwarden tidak terkirim.

### [ ] P2-08 · Daemon: IPC named pipe
* **Dep:** P2-06
* **File:** `app/src/ipc/{server.rs,protocol.rs,client.rs}`
* **Lakukan:** pipe dengan DACL hanya user sekarang; method & event PLAN §5.6; klien dipakai oleh proses settings dan instans kedua.
* **Selesai jika:** test integrasi server↔client dalam proses; instans kedua memicu `show_settings`.

### [ ] P2-09 · Daemon: autostart
* **Dep:** P2-05
* **Selesai jika:** toggle config menulis/menghapus nilai `Run`; path exe ber-spasi dikutip; **[MANUAL]** login ulang → daemon berjalan.

---

## P3 — Aplikasi Android Inti

### [ ] P3-01 · Penyimpanan & identitas
* **Dep:** P1-10
* **File:** `app/.../storage/{SecretStore.kt,SettingsRepo.kt,PeerRepo.kt}`
* **Lakukan:** device_id dibuat sekali; GK dibungkus kunci Keystore AES-256-GCM (alias `transclip_gk_wrap`); DataStore untuk setting & peer.
* **Selesai jika:** instrumented test (atau Robolectric) menyimpan/memuat GK; restart app → device_id sama.

### [ ] P3-02 · Engine Kotlin: Link, PeerManager, Discovery
* **Dep:** P3-01
* **File:** `app/.../engine/*`, `app/.../net/{TcpServer.kt,TcpDialer.kt,NsdDiscovery.kt,BeaconDiscovery.kt}`
* **Lakukan:** padanan P2-01…P2-03 dengan coroutine. `NsdManager` register+discover+resolve (serialisasi resolve, satu per waktu — keterbatasan API lama); `MulticastLock`; `NetworkCallback`.
* **Selesai jika:** unit test JVM untuk Link (dengan stream pipa) dan aturan dial; **[MANUAL]** HP dan PC (daemon P2) saling terhubung, status tampil di log kedua sisi.

### [ ] P3-03 · SyncService & notifikasi
* **Dep:** P3-02
* **File:** `service/SyncService.kt`, `service/BootReceiver.kt`
* **Lakukan:** PLAN §6.5: FGS `connectedDevice`, notifikasi ongoing + aksi (Kirim clipboard, Jeda, Putuskan), start saat boot bila diaktifkan, `AppState` StateFlow.
* **Selesai jika:** **[MANUAL]** service bertahan saat app di-swipe dari recents dan saat layar mati 30 menit; reboot → service jalan.

### [ ] P3-04 · Clipboard writer & riwayat
* **Dep:** P3-03
* **File:** `clip/ClipboardWriter.kt`, `storage/MediaWriter.kt`, `storage/HistoryRepo.kt`
* **Lakukan:** implementasi `ClipboardSink` Android (PLAN §6.4 bagian menulis), penanda echo, simpan riwayat ≤100 item (gambar mereferensikan URI MediaStore).
* **Selesai jika:** **[MANUAL]** salin teks & screenshot di PC → muncul di clipboard HP dan riwayat Gboard (sesuai hasil S1) dalam < 1 s.

### [ ] P3-05 · Jalur capture manual: Activity, Tile, notifikasi
* **Dep:** P3-04
* **File:** `clip/ClipCaptureActivity.kt`, `clip/SendClipTileService.kt`
* **Lakukan:** Activity transparan (theme `Translucent.NoTitleBar`, `excludeFromRecents`, `noHistory`, `taskAffinity=""`) membaca di `onWindowFocusChanged(true)`; cek `EXTRA_IS_SENSITIVE` & penanda echo; kirim via service; toast singkat "Terkirim ke n perangkat". Tile & aksi notifikasi memanggilnya (Android 14+: gunakan `PendingIntent` untuk `startActivityAndCollapse`).
* **Selesai jika:** **[MANUAL]** salin teks di HP → tekan tile → muncul di `Win + V` PC; gambar yang disalin dari galeri juga terkirim.

### [ ] P3-06 · Auto-capture Accessibility (opsional, sesuai S2)
* **Dep:** P3-05, P0-10
* **File:** `clip/CaptureAccessibilityService.kt`, `res/xml/accessibility_config.xml`
* **Lakukan:** implementasi heuristik final dari S2; rate-limit; toggle di Settings (default mati); layar penjelasan sebelum membuka setelan Accessibility.
* **Selesai jika:** **[MANUAL]** di app yang diuji di S2, menyalin teks terkirim otomatis tanpa mengganggu pengetikan; mematikan toggle menghentikan semua pembacaan.
* *Lewati dan tandai `[x] (dibatalkan: lihat DECISIONS)` bila S2 memutuskan tidak dipakai.*

### [ ] P3-07 · UI dasar: Onboarding & Home
* **Dep:** P3-03
* **File:** `ui/OnboardingScreen.kt`, `ui/HomeScreen.kt`, `ui/theme/*`
* **Lakukan:** Onboarding bertahap (notifikasi → nearby devices → baterai → auto-capture opsional), setiap langkah bisa dilewati dengan penjelasan konsekuensi; Home: status, daftar peer (nama, platform, transport, status), 5 item terakhir, tombol *Kirim clipboard*. Material 3, dynamic color, mode gelap.
* **Selesai jika:** **[MANUAL]** alur onboarding di HP baru (clear data) lancar; Home memperbarui status secara live.

---

## P4 — Pairing & Pengaturan

### [ ] P4-01 · Windows: proses Settings egui (kerangka)
* **Dep:** P2-08
* **File:** `app/src/settings/{mod.rs,app.rs,tabs/*.rs}`
* **Lakukan:** `transclip.exe --settings` membuka jendela eframe yang terhubung ke pipe; bila daemon tidak jalan → tawarkan menjalankannya. Enam tab PLAN §5.6 (isi bertahap), skala DPI benar, font sistem (Segoe UI) dengan fallback emoji.
* **Selesai jika:** **[MANUAL]** jendela terbuka dari tray, menutupnya mengakhiri prosesnya (cek Task Manager), RAM daemon tidak naik.

### [ ] P4-02 · Windows: alur pairing
* **Dep:** P4-01
* **Lakukan:** *Buat grup baru*, QR (`qrcode` → tekstur egui) dengan hitung mundur 2 menit, *Salin kode*, *Gabung dengan kode*, argumen `--pair`. Bergabung → simpan GK → engine restart koneksi.
* **Selesai jika:** dua instans Windows (atau Windows + test peer) bergabung via kode dan terhubung; QR terbaca oleh aplikasi scanner umum.

### [ ] P4-03 · Android: alur pairing
* **Dep:** P3-07
* **File:** `ui/PairingScreen.kt`
* **Lakukan:** *Scan QR* (zxing-android-embedded, minta izin kamera), *Tempel kode*, *Buat grup*, *Tampilkan QR* (zxing core → Bitmap), validasi kedaluwarsa, konfirmasi bila mengganti grup yang ada.
* **Selesai jika:** **[MANUAL]** scan QR dari PC → terhubung < 5 s; Android A ↔ Android B via QR.

### [ ] P4-04 · Pengaturan lengkap (kedua platform)
* **Dep:** P4-02, P4-03
* **Lakukan:** semua opsi PLAN §5.2 (`config.toml`) dan padanannya di Android `SettingsScreen` (nama perangkat, jeda, teks/gambar, terima file, boot, auto-capture, prioritas BT per peer, *Lupakan perangkat*, level log, ekspor log via share).
* **Selesai jika:** setiap setting berpengaruh nyata (dicek manual per item, dicatat di `manual-tests.md`).

### [ ] P4-05 · Rotasi kunci grup
* **Dep:** P4-04, P2-04, P3-02
* **Lakukan:** pesan `rekey` / `rekey_ack` (PROTOCOL §9.4) di kedua engine; UI *Ganti Group Key* dengan pilihan perangkat yang dicabut; perangkat yang dicabut dihapus dari daftar.
* **Selesai jika:** test engine Rust: 3 peer, cabut 1 → 2 sisanya reconnect dengan GK baru, yang dicabut gagal handshake; **[MANUAL]** skenario sama dengan perangkat asli.

---

## P5 — Transfer File & Folder

### [ ] P5-01 · Engine Rust: TransferManager
* **Dep:** P2-04
* **File:** `engine/src/transfer/{mod.rs,outgoing.rs,incoming.rs,store.rs}`
* **Lakukan:** PROTOCOL §9.3 lengkap: batch, `dir`, koneksi file terpisah (LAN), streaming SHA-256, `.transclip-part`, resume, rename konflik ` (n)`, validasi path, kuota, maksimal 3 transfer paralel per peer, progres via `watch`, pembatalan dua arah. Trait `IncomingStore` agar platform menentukan lokasi tulis.
* **Selesai jika:** test integrasi localhost: folder 3 tingkat (termasuk folder kosong & nama Unicode) tiba identik (hash per file); putus di tengah 200 MB → resume dari offset; path jahat ditolak; batal di tengah membersihkan part.

### [ ] P5-02 · Biner peer interop live
* **Dep:** P5-01
* **File:** `windows/crates/engine/examples/interop_peer.rs`
* **Lakukan:** engine lengkap headless yang bisa diperintah via stdin (kirim teks, kirim file, tampilkan item diterima) untuk test Kotlin.
* **Selesai jika:** dipakai oleh P5-03; perintah terdokumentasi di `CLAUDE.md`.

### [ ] P5-03 · Engine Kotlin: TransferManager
* **Dep:** P5-02, P3-02
* **Lakukan:** padanan P5-01; `IncomingStore` Android menulis ke default `MediaStore.Downloads` (`Download/TransClip/…`). Folder kustom ditambahkan di P5-09, jadi rancang `IncomingStore` sebagai interface dengan dua implementasi (`MediaStoreDownloads`, `SafTree`).
* **Selesai jika:** test JVM interop live (skip bila env tidak ada): kirim/terima folder dengan `interop_peer`, hash cocok, resume bekerja.

### [ ] P5-04 · Windows: Radial Drop Menu
* **Dep:** P5-01, P2-06, P0-10
* **File:** `app/src/radial/{hook.rs,overlay.rs,drop_target.rs,render.rs,geometry.rs}`
* **Lakukan:** PLAN §5.4 sesuai keputusan S3. `geometry.rs` (sudut → segmen, sub-ring) diuji unit. Render dengan Direct2D atau tiny-skia ke DIB premultiplied.
* **Selesai jika:** unit test geometri (N=1..12, batas sudut); **[MANUAL]** drag 1 file, banyak file, dan 1 folder dari Explorer ke segmen peer & "Semua" → tiba di tujuan; Esc membatalkan; multi-monitor dengan DPI berbeda tampil benar.

### [ ] P5-05 · Windows: SendTo & dialog kirim
* **Dep:** P5-01, P2-08
* **Lakukan:** `transclip.exe --send <paths…>` → kirim ke daemon via pipe → popup pemilih perangkat (egui kecil atau menu radial di tengah layar); buat/hapus shortcut `SendTo` dari setting; menu tray *Kirim file…*.
* **Selesai jika:** **[MANUAL]** klik kanan file → Kirim ke → TransClip → pilih perangkat → terkirim.

### [ ] P5-06 · Windows: penerimaan & notifikasi
* **Dep:** P5-01, P2-06
* **Lakukan:** `IncomingStore` Windows (default `FOLDERID_Downloads\TransClip`, membaca `download_dir` dari config, long path `\\?\`), notifikasi saat batch selesai (klik → buka folder di Explorer dengan file terpilih), progres di tooltip tray.
* **Selesai jika:** **[MANUAL]** kirim folder dari HP → tiba di `Downloads\TransClip\<folder>` dengan struktur utuh.

### [ ] P5-07 · Android: Share Sheet, pemilih perangkat, Direct Share
* **Dep:** P5-03
* **File:** `share/ShareReceiverActivity.kt`, `ui/DevicePickerSheet.kt`
* **Lakukan:** PLAN §6.6.
* **Selesai jika:** **[MANUAL]** bagikan 1 foto, 10 foto, PDF, dan teks dari app lain → tiba di PC; Direct Share menampilkan peer yang sering dipakai.

### [ ] P5-08 · Android: layar Transfer & Riwayat
* **Dep:** P5-07
* **Lakukan:** `TransfersScreen` (progres, batal, ulangi), `HistoryScreen` (ketuk = salin lagi, tahan = hapus, bersihkan semua).
* **Selesai jika:** **[MANUAL]** progres bergerak saat transfer besar; batal di HP menghentikan transfer di PC.

### [ ] P5-09 · Folder tujuan yang dapat diubah (Windows & Android)
* **Dep:** P5-06, P5-03, P4-04
* **Spesifikasi:** PLAN §4.5 (wajib dibaca utuh).
* **Lakukan — Windows:**
  * `app/src/storage/target_dir.rs`: resolve folder efektif (`download_dir` kosong → `FOLDERID_Downloads\TransClip`, buat bila belum ada), validasi bisa ditulis, deteksi tidak tersedia → fallback default + event `target_dir_fallback`.
  * Settings tab Umum: tampilkan path, **Ubah…** (`rfd` folder picker), **Reset ke default**, **Buka folder**; method IPC `set_download_dir` (validasi di daemon, balas error yang jelas).
  * Menu tray *Buka folder unduhan* membuka folder efektif.
  * Cek ruang disk (`GetDiskFreeSpaceExW`) pada volume folder efektif sebelum `file_accept`.
* **Lakukan — Android:**
  * `storage/SafTreeStore.kt`: implementasi `IncomingStore` berbasis `DocumentFile.fromTreeUri` (buat subfolder bertingkat, file part, rename, konflik ` (n)`).
  * `storage/TargetDirRepo.kt`: simpan/hapus `download_tree_uri`, `takePersistableUriPermission` (read+write), lepas izin lama (`releasePersistableUriPermission`) saat diganti/reset, cek `persistedUriPermissions` saat start.
  * SettingsScreen: path yang mudah dibaca, **Ubah…** (`ActivityResultContracts.OpenDocumentTree`), **Reset ke default**, **Buka folder**.
  * Fallback ke `MediaStore.Downloads` bila tree tidak tersedia + notifikasi + banner peringatan di Settings.
* **Lakukan — engine (kedua sisi):** folder efektif diambil saat `file_accept` dan disimpan per `batch_id`, sehingga resume tetap ke folder yang sama walau setting diubah di tengah jalan.
* **Selesai jika:**
  * Unit test Windows: resolve default, path kustom valid, path tidak ada → fallback, path read-only → validasi gagal.
  * Unit test Kotlin (`SafTreeStore` dengan fake `DocumentFile`/Robolectric): folder bertingkat & konflik nama.
  * **[MANUAL]** Windows: ubah ke `D:\Terima` → file & folder tiba di sana; cabut USB yang dijadikan tujuan → fallback ke Downloads + notifikasi; Reset → kembali ke default.
  * **[MANUAL]** Android: ubah ke folder di penyimpanan internal & SD card (jika ada) → file tiba di sana; reboot HP → setting masih berlaku; hapus folder tujuan dari app Files → fallback + notifikasi.

---

## P6 — Bluetooth Cadangan

> Bila P0-10 memutuskan BT ditunda ke v1.1, tandai seluruh P6 `[x] (ditunda)` dan lanjut ke P7.

### [ ] P6-01 · Windows: transport RFCOMM
* **Dep:** P0-09, P2-03
* **File:** `app/src/bluetooth/{server.rs,client.rs,sdp.rs,devices.rs}`
* **Lakukan:** API hasil S4; server terdaftar dengan UUID PROTOCOL §2; klien ke perangkat paired; daftar perangkat paired OS (nama + alamat) untuk UI; bungkus socket sebagai `AsyncRead+AsyncWrite` (thread blocking + channel bila perlu).
* **Selesai jika:** **[MANUAL]** handshake + hello sukses melalui BT dengan HP (Wi-Fi HP dimatikan).

### [ ] P6-02 · Android: transport RFCOMM
* **Dep:** P0-09, P3-02
* **File:** `net/RfcommServer.kt`, `net/RfcommDialer.kt`
* **Selesai jika:** **[MANUAL]** Android ↔ Windows dan Android ↔ Android terhubung via BT.

### [ ] P6-03 · Mode Priority/Lazy & failover (kedua engine)
* **Dep:** P6-01, P6-02
* **Lakukan:** PLAN §4.2 lengkap (10 s, 60 s, maks 3 link BT, prioritas per peer), multiplexing chunk 8 KiB di link BT, `announce`/`pull` untuk gambar besar, notifikasi "Gambar tersedia".
* **Selesai jika:** test engine dengan transport palsu (LAN & BT disimulasikan) untuk failover dan pemulihan tanpa duplikasi; **[MANUAL]** matikan Wi-Fi di tengah sesi → teks tetap tersinkron via BT dalam < 15 s; nyalakan lagi → kembali ke LAN, link BT ditutup setelah 60 s.

---

## P7 — Hardening, Kinerja, Keamanan

### [ ] P7-01 · Fuzzing & batas
* **Lakukan:** `proptest` untuk frame/record/beacon/pairing (Rust) dan test acak JUnit (Kotlin) 10 000 iterasi; tambah `cargo fuzz` target bila memungkinkan di Windows (opsional).
* **Selesai jika:** tidak ada panic/crash; semua input salah berakhir dengan error terkendali.

### [ ] P7-02 · Rate limit & kuota
* **Lakukan:** per peer: maks 20 item clipboard / 10 s, maks 3 transfer paralel, `max_incoming_file_bytes`, cek ruang disk sebelum `file_accept` (`disk_full`).
* **Selesai jika:** test engine memverifikasi `busy`, `quota`, `disk_full`.

### [ ] P7-03 · Audit keamanan mandiri
* **Lakukan:** checklist di `docs/security-review.md`: tidak ada log isi clipboard/kunci; zeroize kunci; DACL pipe; validasi semua input dari jaringan; tidak ada `unwrap` pada data jaringan; QR tidak disimpan; file diterima tidak pernah dieksekusi/dibuka otomatis; dependency audit (`cargo audit`, `./gradlew dependencyCheck` atau setara).
* **Selesai jika:** semua item checklist berstatus OK atau punya catatan risiko yang diterima pengguna.

### [ ] P7-04 · Kinerja & sumber daya
* **Lakukan:** ukur dan catat di `docs/perf.md`: latensi teks p95 (100 sampel), gambar 3 MB, transfer 1 GB (MB/s), RAM idle & saat transfer (Windows, Android), CPU idle, baterai Android 8 jam idle (**[MANUAL]**).
* **Selesai jika:** target PLAN §10 terpenuhi atau selisihnya dijelaskan + disetujui pengguna.

### [ ] P7-05 · Ketahanan
* **Lakukan:** uji 24 jam (2 PC/1 PC + 2 HP), ganti jaringan Wi-Fi, sleep/hibernate Windows (tangani `WM_POWERBROADCAST` → reconnect), restart router, mode pesawat, kehabisan disk.
* **Selesai jika:** hasil dicatat di `manual-tests.md`; tidak ada crash/kebocoran memori (> 20% naik).

---

## P8 — Rilis

### [ ] P8-01 · Versi & changelog
* **Lakukan:** versi `1.0.0` di `Cargo.toml` (app) dan `versionName`/`versionCode` Android; `CHANGELOG.md`; versi ditampilkan di Settings dan `hello.app`.

### [ ] P8-02 · Build release reproducible
* **Lakukan:** skrip `scripts/build-release.ps1` yang menjalankan test lalu menghasilkan `dist/TransClip-<ver>-windows-x64.exe` dan `dist/TransClip-<ver>.apk` + `SHA256SUMS.txt`.
* **Selesai jika:** skrip berjalan bersih dari clone baru (setelah toolchain terpasang).

### [ ] P8-03 · Penandatanganan [MANUAL]
* **Lakukan:** pandu pengguna membuat keystore Android (simpan di luar repo) dan mengisi `keystore.properties`. Code signing Windows **(opsional)** bila pengguna punya sertifikat.

### [ ] P8-04 · Dokumentasi pengguna
* **Lakukan:** `README.md` (fitur, unduh, instal, pairing, firewall *Private network*, izin Android dan alasannya, keterbatasan: UIPI, auto-capture, BT), `docs/troubleshooting.md` (tidak terhubung, AP isolation, Doze, SmartScreen, cara ambil log).

### [ ] P8-05 · Uji penerimaan akhir [MANUAL]
* **Lakukan:** jalankan seluruh `docs/manual-tests.md` dengan build release di 1 PC Windows 11 + 2 Android berbeda merek.
* **Selesai jika:** semua lulus; tag git `v1.0.0`.

---

## Lampiran — Skenario uji manual minimum (salin ke `docs/manual-tests.md` di P0-01)

| ID | Skenario |
|---|---|
| M01 | Teks PC → semua HP (< 1 s), muncul di Gboard |
| M02 | Screenshot Win+Shift+S → HP, kualitas identik (bandingkan hash PNG) |
| M03 | Teks HP (tile) → PC, muncul di Win+V |
| M04 | Foto galeri HP (share) → PC sebagai gambar clipboard |
| M05 | Salin dari password manager → tidak terkirim |
| M06 | Tidak ada loop/duplikat dengan 1 PC + 2 HP aktif |
| M07 | Radial: 1 file, 20 file, folder bertingkat → 1 peer dan "Semua" |
| M08 | Share 10 foto dari HP → PC, struktur & hash cocok |
| M09 | File 2 GB, cabut Wi-Fi di 50%, sambung lagi → resume, hash cocok |
| M10 | Wi-Fi mati → BT mengambil alih teks; Wi-Fi kembali → LAN |
| M11 | HP layar mati 1 jam → masih menerima teks |
| M12 | Reboot PC & HP → otomatis tersambung lagi |
| M13 | Pairing via QR dan via kode teks; QR kedaluwarsa ditolak |
| M14 | Rotasi kunci: perangkat dicabut tidak bisa terhubung |
| M15 | Path jahat (dikirim via `interop_peer`) ditolak |
| M16 | Drag dari app Admin → didokumentasikan tidak didukung, tidak crash |
| M17 | Folder tujuan: default di Download (kedua platform) → ubah ke folder lain → file tiba di folder baru → folder dicabut/dihapus → fallback ke Download + notifikasi → Reset ke default |
