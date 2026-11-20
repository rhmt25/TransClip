# Membuat rilis

1. Jalankan `.\scripts\build-release.ps1` dari root repo. Skrip menjalankan seluruh test lalu menghasilkan `dist\TransClip-<ver>-windows-x64.exe`, `dist\TransClip-<ver>.apk`, dan `dist\SHA256SUMS.txt`.
2. **Penandatanganan APK (manual):** buat keystore di luar repo:
   ```powershell
   keytool -genkeypair -v -keystore C:\kunci\transclip.jks -alias transclip -keyalg RSA -keysize 4096 -validity 10000
   ```
   Lalu buat `android\keystore.properties` (sudah ada di `.gitignore`):
   ```
   storeFile=C:/kunci/transclip.jks
   storePassword=...
   keyAlias=transclip
   keyPassword=...
   ```
   Simpan keystore dan sandinya dengan aman; tanpa itu pembaruan APK tidak bisa dipasang di atas versi lama.
3. Windows code signing bersifat opsional (butuh sertifikat); tanpa itu SmartScreen memperingatkan.
4. Jalankan seluruh `docs\manual-tests.md` dengan build release, lalu beri tag `v1.1.0`.
