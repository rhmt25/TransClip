# Kinerja & sumber daya (P7-04)

Pengukuran awal pada mesin pengembang (Windows 11, build debug kecuali dicatat). Pengukuran lengkap pada build release dan perangkat nyata dikerjakan di uji penerimaan (P8-05).

| Metrik | Hasil | Target PLAN §10 | Catatan |
|---|---|---|---|
| RAM daemon idle (Windows, debug) | ~26 MB | kecil | Settings egui berjalan di proses terpisah (~105 MB), tertutup saat jendela ditutup |
| Transfer 24 MiB + resume | lulus, hash cocok | n/a | test `engine` |
| Transfer file 2,5 MB HP ke PC (Wi-Fi) | selesai, hash cocok | n/a | uji manual 2026-11-18 |
| Latensi teks p95 (100 sampel) | **belum diukur** | < 1 s | [MANUAL] |
| Transfer 1 GB (MB/s) | **belum diukur** | | [MANUAL] |
| RAM Android idle / saat transfer | **belum diukur** | | [MANUAL] |
| Baterai Android 8 jam idle | **belum diukur** | | [MANUAL] |

Selisih terhadap target belum dapat dinilai sebelum pengukuran manual di atas dilakukan.
