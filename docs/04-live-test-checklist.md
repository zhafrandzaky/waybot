# 04 — Checklist Riset Live

Status: alur inti terverifikasi di room 823944, 501008, 930417, 180115, 609710, 130376, 099609. Uji terakhir 10/10 soal terjawab.

## Sudah terverifikasi
- [x] Alur join, lobby, start (resume baru diuji sebagian).
- [x] `getQuestions` dan skema semua tipe pada quiz uji.
- [x] DOM MCQ/MSQ (`button.option[data-cy=option-N]`, `aria-label`, `aria-selected`).
- [x] DOM BLANK (`input[data-cy=fib-text-input]`), OPEN (Tiptap `div.ProseMirror`), submit (`button[data-cy=submit-button]`), review + “Next question”.
- [x] Payload asli MCQ, MSQ, BLANK, OPEN lewat `proceed`.
- [x] Kunci jawaban resmi lewat `_quizserver/main/v2/quiz/<quizId>` (`structure.answer`).
- [x] Penilaian server (`marks`), ringkasan `player-summary`.
- [x] Soal/opsi diacak; mapping opsi via `data-cy` + `aria-label`.
- [x] Gambar tanpa alt di DOM.
- [x] Timer 30 detik lalu review, tanpa auto-advance.

## Masih perlu (butuh room baru)
- [ ] Verifikasi `quizId` didapat dari hook join/rejoin di `document_start` (bukan hanya dari rejoin game-over).
- [ ] Uji tipe DROPDOWN, REORDER, MATCH, DRAGNDROP, DND_IMAGE, HOTSPOT (belum ada di quiz uji).
- [ ] Verifikasi payload tipe tersebut.
- [ ] Verifikasi kunci `structure.answer` untuk MSQ/BLANK di room lain.
- [ ] Uji room premium/focusMode (fullscreen, alertOnInfraction).
- [ ] Uji join setelah start / resume nama sama.
- [ ] Uji `loginRequired`.
- [ ] Uji rate limit DeepSeek + fallback.
- [ ] Uji gambar tanpa explain.

## Temuan implementasi (baru)
- [x] Urutan `getQuestions` tidak selalu sama dengan urutan kuis live; pencocokan harus lewat teks soal.
- [x] Sebagian tipe memakai wadah soal tanpa `data-quesid`; id disintesis dari nomor soal.
- [x] Opsi dirender ganda (satu set tersembunyi); klik harus pada elemen yang benar-benar terlihat.
- [x] Popup login Google menutupi halaman dan menelan klik; harus ditutup.
- [x] Setelah kuis muncul layar survei/ringkasan; tanpa deteksi akhir program berputar di sana.
- [x] Pengetikan per karakter terlalu lambat untuk esai panjang; potongan karakter diperlukan.
- [x] `WebDriver:GetWindowHandles` mengembalikan array langsung, bukan di dalam `value`.
- [ ] Jalur **Start Over** (resume mati) pada room dengan percobaan menggantung: harus Start Over sekali, nama diketik ulang, lalu Start.
- [ ] Jalur **Resume attempt** (resume hidup) pada room yang sama: harus melanjutkan percobaan, bukan mengulang soal.
- [ ] Uji TUI manual penuh (Setup → Konfirmasi) tanpa flag, termasuk panel `F3` dan toggle.
