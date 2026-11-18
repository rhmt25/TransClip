# UI/UX Design Guidelines (Anti-AI Slop)

Patuhi pedoman desain berikut secara ketat setiap kali membangun antarmuka pengguna (Windows Desktop maupun Android):

## 1. Prinsip Anti-Slop (Larangan Keras)
* **DILARANG menggunakan warna hitam pekat (`#000000`) atau putih murni (`#FFFFFF`)** sebagai warna latar belakang utama. Gunakan warna netral lembut (*off-white* dan *dark slate/charcoal*) agar nyaman di mata.
* **DILARANG menggunakan gradasi mencolok (*neon/purple-pink gradients*)**, efek *glassmorphism* berlebihan, atau bayangan (*drop-shadow*) tebal yang membuat elemen tampak mengambang tidak wajar.
* **DILARANG membuat kartu di dalam kartu (*nested cards*)** atau terlalu banyak ikon dekoratif yang tidak memiliki fungsi interaktif.
* **Prioritaskan kepadatan informasi yang bersih (*clean density*):** Gunakan spasi (*whitespace*) yang konsisten (kelipatan `4px` atau `8px`), garis pemisah tipis (`1px solid`), dan tipografi yang jelas sebagai pembentuk hierarki visual.

## 2. Sistem Warna (Light & Dark Theme Tokens)
Aplikasi wajib mendukung **Light Mode** dan **Dark Mode** dengan **tombol pengubah tema (*Theme Switcher*)** yang mudah diakses di header/sudut jendela pengaturan.

### Light Mode (Terang — Soft Off-White)
* **Background Utama (`bg-canvas`):** `#F7F9FC` (Putih lembut kebiruan, tidak menyilaukan)
* **Surface / Kartu (`bg-surface`):** `#FFFFFF` (Hanya untuk permukaan kontainer di atas canvas)
* **Surface Hover / Sekunder (`bg-subtle`):** `#EDF2F7`
* **Teks Utama (`text-primary`):** `#1A202C`
* **Teks Sekunder (`text-secondary`):** `#4A5568`
* **Border / Garis Pemisah (`border-subtle`):** `#E2E8F0` (`1px`)
* **Warna Primer (`accent-primary`):** `#0EA5E9` (Biru muda cerah & bersih / *Sky Blue*)
* **Warna Primer Hover (`accent-hover`):** `#0284C7`
* **Teks di Atas Warna Primer (`text-on-accent`):** `#FFFFFF`

### Dark Mode (Gelap — Soft Charcoal, Bukan Hitam Pekat)
* **Background Utama (`bg-canvas`):** `#12151A` (Abu-abu gelap lembut, nyaman untuk OLED & IPS)
* **Surface / Kartu (`bg-surface`):** `#1A1E26` (Sedikit lebih terang dari canvas untuk membedakan elevasi tanpa bayangan)
* **Surface Hover / Sekunder (`bg-subtle`):** `#232934`
* **Teks Utama (`text-primary`):** `#F1F5F9`
* **Teks Sekunder (`text-secondary`):** `#94A3B8`
* **Border / Garis Pemisah (`border-subtle`):** `#2A313D` (`1px`)
* **Warna Primer (`accent-primary`):** `#38BDF8` (Biru muda pastel cerah agar kontras optimal di latar gelap)
* **Warna Primer Hover (`accent-hover`):** `#7DD3FC`
* **Teks di Atas Warna Primer (`text-on-accent`):** `#0B111A` (Gelap pekat agar rasio kontras WCAG AAA terpenuhi)

### Warna Status Semantik (Gunakan Seperlunya)
* **Terhubung / Aktif (LAN/BT):** `#10B981` (Light) / `#34D399` (Dark)
* **Peringatan / Fallback:** `#F59E0B` (Light) / `#FBBF24` (Dark)
* **Terputus / Error:** `#EF4444` (Light) / `#F87171` (Dark)

## 3. Komponen & Layout
* **Tombol Pengubah Tema (Theme Toggle):**
  * Tempatkan di pojok kanan atas *Settings Window* (Windows) dan *Top App Bar* (Android).
  * Gunakan ikon minimalis (Matahari / Bulan / Auto Sistem) dengan transisi warna halus (`150ms ease`).
* **Kontrol & Input:**
  * Gunakan sudut membulat (*border-radius*) yang konsisten dan modern namun tidak terlalu bulat (`8px` untuk kartu/kontainer, `6px` untuk tombol dan input).
  * Gunakan *toggle switch* bergaya natif (mengikuti bahasa desain *Windows 11 Fluent* di desktop dan *Material 3* di Android) berwarna biru muda (`accent-primary`) saat aktif.
* **Tipografi:**
  * Gunakan font sistem natif agar cepat dan menyatu dengan OS (`Segoe UI Variable` / `Segoe UI` untuk Windows 11, `Roboto` / `Inter` untuk Android).
  * Gunakan *monospace font* (`Consolas` / `JetBrains Mono`) khusus untuk menampilkan `Device ID`, alamat IP lokal, *hash*, atau kode PIN *pairing*.
* **Radial Drop Menu (Khusus Windows Overlay):**
  * Gunakan latar semi-transparan gelap lembut (`rgba(18, 21, 26, 0.88)`) dengan tepian `1px solid rgba(56, 189, 248, 0.3)` dan efek *highlight* biru muda (`#38BDF8`) saat kursor mengarah ke segmen perangkat.

Untuk radial desain bentuknya ikuti di filee seperti pada gambar E:\Projek\TransClip\pedoman_desain\bentuk radial desain.jpg namun untuk warnanya sesuaikan tema namun khusus radial ini tetap ada efek transparannya seperti di filee