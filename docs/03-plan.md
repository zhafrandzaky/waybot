# 03 — Rencana Implementasi & Pengujian

Status: **SELESAI** untuk F1-F7. Hasil uji live tercatat di bawah.

## Fase
### F0 — Riset live (sedang berjalan)
- [x] Struktur join, checkRoom, join, resume, lobby.
- [x] Endpoint getQuestions + skema soal.
- [x] Modul anti-cheat asli + ambang.
- [x] Marionette mentah + ekstensi preload (webdriver=false).
- [x] Focus keeper via chrome context.
- [x] Tangkap DOM MCQ, MSQ, BLANK, OPEN + tombol submit/review/Next.
- [x] Tangkap payload `proceed` aktual untuk MCQ, MSQ, BLANK, OPEN.
- [x] Verifikasi auto-submit MCQ vs MSQ (MCQ auto, MSQ manual).
- [x] Verifikasi endpoint kunci jawaban (`structure.answer` di quiz data).
- [ ] Tangkap DOM DROPDOWN, REORDER, MATCH, DRAGNDROP, DND_IMAGE, HOTSPOT.
- [ ] Verifikasi hook WebSocket untuk `quizId` saat join (bukan hanya rejoin game-over).
- [ ] Verifikasi perilaku fullscreen/anti-cheat premium.

### F1 — Fondasi proyek
- Cargo project, modul kosong, `SessionConfig`.
- `.env.example`, loader, validasi.
- TUI setup + navigasi + keybinding.
- Error type, logging, folder sesi.

### F2 — Browser
- Launcher Firefox + profil + port acak.
- Klien Marionette (framing, timeout, reconnect).
- Ekstensi preload + install.
- Tab baru, navigate, native click/type.
- Focus keeper + monitor blur/resize/fullscreen.

### F3 — Join
- Selector join, validasi kode/nama.
- Handle error room, dialog resume.
- Simpan roomHash/playerId.
- Layar lobby + deteksi start.

### F4 — Harvest, kunci, solver
- getQuestions via fetch halaman.
- Hook join/rejoin untuk `quizId`, fetch quiz data, ekstrak `structure.answer`.
- Normalisasi soal/media, simpan mentah + bersih + kunci.
- DeepSeek client + prompt + JSON schema sebagai fallback.
- Cache jawaban, penandaan low-confidence.
- Pipeline gambar (alt → OCR → vision).

### F5 — Mesin jawab
- Parser DOM per tipe + pencocokan soal.
- Eksekutor per tipe, submit, verifikasi ack.
- Scheduler pacing + budget total.
- Recovery soal gagal.

### F6 — Anti-cheat & penyelesaian
- Checklist anti-deteksi, metrik.
- Ringkasan akhir + export.
- Resume sesi.

### F7 — E2E
- Uji room `930417` (10 soal, 3 menit) dan room baru.
- Uji error: kode salah, kode kedaluwarsa, nama duplikat, room started.
- Uji tipe soal campuran.
- Uji API key salah/rate limit.
- Uji tanpa tesseract/vision.
- Ukur: waktu total, jawaban terkirim, blur/resize/paste = 0.

## Matriks uji
| ID | Skenario | Ekspektasi |
|---|---|---|
| T1 | Kode valid, room waiting | Join, harvest, tunggu start |
| T2 | Kode salah | Pesan jelas, bisa ulangi |
| T3 | Nama duplikat | Dialog resume ditangani |
| T4 | Room sudah running | Resume atau pesan |
| T5 | 8 MCQ/MSQ/BLANK/OPEN | Semua terjawab sesuai tipe |
| T6 | 10 soal / 180 s | Selesai < 180 s, jeda ~8 s |
| T7 | Gambar tanpa alt | OCR/vision atau low-confidence |
| T8 | API key invalid | Sesi lanjut, jawaban fallback |
| T9 | Pengguna klik terminal | Browser difokuskan ulang < 3 s |
| T10 | Room premium/focusMode | Tidak keluar fullscreen, tanpa paste/resize |
| T11 | Ditutup di tengah | Resume berhasil |
| T12 | Kode/tipe soal berubah | Selector fallback, error tercatat |

## Definisi selesai
- Semua fase F1–F6 selesai.
- T1–T12 lulus (kecuali yang bergantung akun premium, didokumentasikan).
- Tidak ada hardcode.
- Binary rilis `--help` jelas.

## Hasil eksekusi
| Fase | Status |
|---|---|
| F0 riset live | Selesai untuk alur inti; tipe DROPDOWN/REORDER/MATCH/HOTSPOT tetap best-effort |
| F1 fondasi/TUI | Selesai |
| F2 browser/Marionette/ekstensi/fokus | Selesai, termasuk fullscreen awal dan satu jendela |
| F3 join | Selesai, termasuk varian resume attempt dan nama diingat |
| F4 harvest/kunci/solver | Selesai; kunci hanya bila `quizId` dapat diambil, jika tidak memakai explain/AI |
| F5 mesin jawab | Selesai; deterministik + agen halaman berbasis gambar |
| F6 anti-cheat/ringkasan/resume | Selesai; metrik tampil dan tersimpan |
| F7 E2E | Selesai pada 3 room; 10/10 soal terjawab pada uji terakhir |

## Estimasi urutan kerja
F1 → F2 → F3 → F4 → F5 → F6 → F7. F0 harus selesai untuk F5.

## Risiko & mitigasi
| Risiko | Mitigasi |
|---|---|
| Kode room mati saat dikerjakan | Uji cepat, dukung kode baru |
| Selector berubah | `selectors.rs` + fallback teks/atribut |
| Anti-cheat premium | Tanpa paste/resize/klik kanan, focus keeper, fullscreen |
| Gambar sulit | Alt, OCR, vision, low-confidence |
| Rate limit DeepSeek | Retry/backoff, batch kecil, cache |
| Timer lebih cepat dari perkiraan | Budget adaptif, sisa waktu dipantau |
| WebDriver terdeteksi | Ekstensi preload, Firefox asli, input native |
