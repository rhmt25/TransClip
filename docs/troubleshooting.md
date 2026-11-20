# Pemecahan masalah

## Perangkat tidak saling terhubung
1. Pastikan keduanya di **Wi-Fi yang sama** dan sudah tergabung di grup yang sama (tab Perangkat di PC menunjukkan statusnya).
2. **Firewall Windows:** saat pertama jalan, pilih **Private network**. Bila terlanjur menolak: Windows Security, *Firewall*, *Allow an app*, centang TransClip untuk jaringan Private. Port yang dipakai: TCP 47820, UDP 47821.
3. **AP isolation / jaringan tamu:** sebagian router memblokir komunikasi antar perangkat. Matikan *AP isolation* atau pakai jaringan utama.
4. **Profil jaringan Publik:** ubah Wi-Fi ke *Private* di Windows (Settings, Network).
5. Kode pairing berlaku 2 menit. Bila ditolak, buat kode baru.

## HP berhenti menerima saat layar mati
Pengoptimal baterai (Doze, mode hemat merek tertentu) menghentikan layanan. Buka Pengaturan Android, Aplikasi, TransClip, Baterai, pilih **Tanpa batasan**. Pada beberapa merek (Xiaomi, Oppo, Vivo, Samsung) aktifkan juga *Mulai otomatis*.

## Xiaomi, POCO, Redmi (HyperOS/MIUI): sinkron berhenti setelah restart
1. Aktifkan **Mulai otomatis**: aplikasi *Keamanan*, *Izin*, *Mulai otomatis*, nyalakan TransClip. Menunya tidak ada di halaman info aplikasi.
2. Pengaturan, Aplikasi, TransClip, *Penghemat baterai*: **Tanpa batasan**. Di Recent Apps, tekan lama kartu TransClip lalu kunci dengan ikon gembok.
3. Bila Pengaturan Aksesibilitas menulis *Layanan ini tidak berfungsi*, sistem melepas layanan setelah aplikasi dimatikan. Beri izin sekali agar TransClip memasangnya ulang sendiri:
   `adb shell pm grant io.transclip.app android.permission.WRITE_SECURE_SETTINGS`
   Tanpa izin itu, matikan lalu nyalakan lagi layanannya secara manual.

## Bluetooth tidak tersambung otomatis
Pasangkan (pair) HP dengan PC di pengaturan Bluetooth sistem, sekali saja. Bluetooth hanya dipakai bila Wi-Fi putus atau berbeda jaringan; saat satu Wi-Fi, LAN yang dipakai.

## SmartScreen memperingatkan saat membuka .exe
File belum ditandatangani sertifikat code signing. Pilih *More info*, lalu *Run anyway*. Cocokkan SHA-256 file dengan `SHA256SUMS.txt`.

## Clipboard dari aplikasi tertentu tidak terkirim
- Aplikasi yang dijalankan sebagai Administrator tidak bisa dibaca proses biasa (UIPI).
- Pengelola sandi yang menandai isi clipboard sebagai sensitif sengaja tidak dikirim.

## Mengambil log
- **Windows:** Settings, tab Lanjutan, *Buka folder log* (`%LOCALAPPDATA%\TransClip\logs`). Naikkan level ke `debug` bila perlu.
- **Android:** `adb logcat -s TransClip`.
Log tidak berisi isi clipboard, kunci, atau nama file (di atas level debug).

## Lain-lain
- **Dua grup tercampur:** *Ganti Group Key* membuat kunci baru dan memutus perangkat yang dicabut.
- **Daemon macet:** `transclip.exe --quit`, lalu jalankan lagi.
