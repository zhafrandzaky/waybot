# 02 — Arsitektur (rencana Rust)

Status: rancangan; belum diimplementasikan.

## Prinsip
1. **Satu sumber kebenaran runtime**: `SessionConfig` (kode, nama, durasi, opsi solver) diisi lewat TUI; tidak ada konstanta kuis.
2. **Browser nyata > HTTP murni**: semua langkah yang berpotensi diawasi dilakukan di Firefox. HTTP internal hanya untuk mengambil data yang memang diambil klien (soal) setelah sesi sah.
3. **Pemisahan lapisan**: TUI, orkestrasi, browser, Wayground, solver, penyimpanan. Selector/API terpusat.
4. **Fail-soft**: kegagalan solver tidak menghentikan sesi; kegagalan satu soal dicatat dan lanjut.
5. **Anti-deteksi adalah fitur**, bukan tambahan: fokus, native input, jitter, tanpa paste, ukuran tetap.

## Stack
- Rust 2021, single binary.
- `ratatui` + `crossterm` untuk TUI full-screen.
- `tokio` untuk HTTP solver/timer; thread khusus untuk Marionette (blocking TCP).
- `serde`/`serde_json`, `reqwest` (rustls), `dotenvy`, `anyhow`/`thiserror`, `tracing` + file appender, `rand`, `unicode-width`.
- `x11rb` (opsional) untuk aktivasi jendela bila chrome focus gagal.
- Tanpa `geckodriver`, tanpa `playwright`, tanpa Node.
- Proses Firefox dikelola langsung (`Command`), profil sementara/persisten di `~/.local/share/waybot/profiles/<id>`.

## Modul
```
src/
  main.rs                 // bootstrap, env, runtime, TUI
  config.rs               // SessionConfig, AppConfig, validasi, .env
  tui/
    mod.rs                // App, event loop, keybinding
    screens/{setup,join,lobby,quiz,done}.rs
    widgets/{timer,question,log,status,help}.rs
  browser/
    mod.rs                // BrowserHandle API tingkat tinggi
    firefox.rs            // launch/kill, profil, prefs, ukuran, window id
    marionette.rs         // framing protokol, command, session
    extension.rs          // tulis & install ekstensi preload sementara
    input.rs              // klik native, ketik per karakter + jitter
    dom.rs                // eval JS, query, tunggu, teks, atribut, rect
    focus.rs              // focus keeper, deteksi blur, fullscreen
  wayground/
    mod.rs                // state machine sesi
    urls.rs               // URL builder (join/pre-game/game)
    selectors.rs          // SEMUA selector, dengan fallback
    join.rs               // checkRoom/join UI, dialog resume
    lobby.rs              // deteksi start, playerId, roomHash
    questions.rs          // getQuestions, normalisasi, media, alt
    quiz.rs               // deteksi soal aktif, pencocokan, eksekusi jawaban
    submit.rs             // klik/isi + submit + verifikasi ack
  solver/
    mod.rs                // AnswerEngine trait
    deepseek.rs           // chat/completions, retry, timeout, biaya
    prompt.rs             // prompt + JSON schema ketat
    vision.rs             // OCR/alt/vision opsional
    cache.rs              // cache jawaban per hash soal
  model/
    question.rs           // tipe, opsi, target, media, html→teks
    answer.rs             // representasi jawaban per tipe + confidence
    session.rs            // state sesi & progres
  store/
    mod.rs                // direktori sesi, JSON, Markdown, atomik
    export.rs
  anticheat/
    mod.rs                // checklist, metrik kepatuhan
    pacing.rs             // scheduler budget total + jitter
  util/
    html.rs, text.rs, time.rs, id.rs
scripts/ (nanti)
```

## Alur state
`Setup → Validating → Launching → Joining → Lobby → Harvesting → WaitingStart → Answering → Finishing → Done/Failed`

- `Harvesting` terjadi segera setelah join dan **sebelum** start.
- `WaitingStart` memantau DOM/room state; tidak mengirim jawaban sebelum soal tampil.
- `Answering` hanya jalan setelah nomor soal pertama terlihat.
- Sesi baru secara default. Dengan `Resume sesi` aktif, `last-session.json` dipakai hanya bila room sama, umur <15 menit, dan URL-nya game/pre-game. Tanpa resume, layar percobaan menggantung ditangani dengan **Start Over** sekali, nama diketik ulang, lalu Start.

## TUI
Layar:
1. **Setup**: field kode, nama, durasi (mm:ss), model, jeda min/maks, toggle vision, toggle fullscreen, profil.
2. **Konfirmasi**: ringkasan + peringatan fokus.
3. **Dashboard**: status, timer total, timer soal, nomor soal, daftar soal (scroll), jawaban terpilih, log, indikator anti-cheat (fokus, webdriver, paste, resize, fullscreen).
4. **Ringkasan**: benar/salah/unknown, waktu, file tersimpan.

Keybinding: Tab/Shift-Tab, Enter, Esc, F1 bantuan, F2 log, Ctrl-C abort (konfirmasi). Tidak ada input saat mode menjawab kecuali abort.

## Browser
- Launch: `firefox --marionette --remote-allow-system-access --new-instance --no-remote --profile <dir> --width 1280 --height 860` (tanpa argumen `about:blank`; jendela yang ada dipakai ulang).
- Port Marionette acak, ditulis ke `user.js`; tunggu listener; handshake; `WebDriver:NewSession`.
- `Addon:Install` ekstensi `preload.xpi` (temporary) yang menyuntik `page.js` di `document_start` (world MAIN):
  - `navigator.webdriver → false`
  - hook `WebSocket` (opsional, untuk observasi)
  - penanda `window.__waybot`
- Pakai jendela yang sudah dibuat proses; jendela sesi tambahan ditutup. Navigasi langsung ke `https://wayground.com` tanpa tab baru. Fullscreen dimasukkan setelah URL termuat dan sebelum join.
- Native click: `WebDriver:ElementClick` (fallback koordinat via `WebDriver:PerformActions`).
- Ketik: `WebDriver:ElementSendKeys` satu karakter, delay 45–140 ms, salah ketik sesekali lalu backspace (opsional).
- Fokus: cek `document.hasFocus()`/`visibilityState`; panggil chrome `focus()` bila perlu.
- Screenshot berkala saat error (WebP/PNG) ke folder sesi.

## Join
1. Buka `https://wayground.com/join` (atau beranda lalu klik join).
2. Isi kode per karakter, klik join.
3. Tunggu salah satu: layar nama, error room, atau layar resume.
4. Isi nama, tangani dialog duplikat: `Resume attempt` bila toggle resume aktif, atau `Start Over` + ketik ulang nama bila tidak.
5. Klik start, tunggu URL game dan elemen lobby.

Validasi input: kode 3–11 alfanumerik; nama 1–60 karakter, trim, tanpa karakter kontrol.

## Harvesting soal
1. Ambil `roomHash` dari respons `checkRoom` yang sudah ditangkap saat join (atau panggil ulang endpoint yang sama dari konteks halaman).
2. `POST /play-api/v4/getQuestions` memakai `fetch` di halaman (cookie/header asli) dengan `{roomHash, playerId}`.
3. Normalisasi HTML→teks, kumpulkan media + alt, simpan JSON mentah + JSON bersih.
4. Bila endpoint gagal: fallback tangkap soal dari DOM saat kuis berjalan dan dari store Pinia bila tersedia.
5. Setiap soal diberi hash stabil; cache jawaban dipakai lintas sesi.

## Solver
- `AnswerEngine` trait; implementasi DeepSeek.
- Prompt memuat: tipe soal, teks, opsi berindeks, alt/media, petunjuk `explain`, format keluaran JSON.
- Keluaran wajib:
  ```json
  {"answer": ["<opsi>"], "confidence": 0.0-1.0, "reason": "singkat"}
  ```
- Aturan per tipe:
  - MCQ: 1 opsi; MSQ: 1..n; BLANK: teks; OPEN: teks; DROPDOWN: pilih opsi; REORDER: urutan; MATCH: pasangan; DND/HOTSPOT: target id/koordinat.
- Suhu rendah (0–0.3), `response_format` JSON bila didukung, retry 2× dengan backoff, timeout 20 s.
- Bila confidence < ambang atau JSON invalid → pilihan heuristik (mis. opsi dengan kemiripan token tertinggi) dan tandai `low_confidence`.
- Biaya dicatat (token in/out) di log.

## Gambar / OCR
Urutan fallback:
1. alt text + konteks sekitarnya.
2. OCR lokal bila tersedia (deteksi `tesseract` di PATH; jalankan di file sementara).
3. Vision API: kirim `image_url` (data URL/base64) ke model vision yang dikonfigurasi di `.env` bila mendukung; jika ditolak, lanjut tanpa gambar dan confidence diturunkan.
4. Selalu simpan URL gambar di JSON sesi.

## Eksekusi jawaban (per tipe)
- Cocokkan soal DOM dengan daftar lewat teks ternormalisasi/hash; bila tidak cocok, pakai urutan/tipe.
- MCQ: klik `[data-testid="option-<id>"]`; auto-submit pada MCQ.
- MSQ: klik semua opsi benar, lalu `submit-button`.
- BLANK/DROPDOWN: buka kontrol, isi/pilih teks, submit.
- OPEN: isi textarea, submit.
- REORDER/MATCH/DRAG: klik-urutkan/pasangkan sesuai instruksi DOM; fallback drag via `PerformActions`.
- HOTSPOT: klik koordinat relatif dari data soal; fallback pilih opsi pertama.
- Setelah submit: tunggu indikator jawaban terkirim/soal berubah; verifikasi `response` di store/DOM; retry maksimal 2×.

## Pacing
- Anggaran total = durasi input − margin (mis. 12%).
- Anggaran per soal = total / jumlah soal.
- Jeda target = `min(8s, anggaran_per_soal × 0.7)` dengan jitter ±25%; batas bawah 2.5 s.
- Jika tertinggal, kurangi jeda; jika soal tidak terjawab mendekati batas, jawab cepat berurutan.
- Timer mulai saat soal pertama tampil.
- “Satu soal per 8 detik” adalah default; bisa diubah di Setup.

## Anti-cheat
- Preload `navigator.webdriver=false`.
- Tanpa paste; cek `document.activeElement` sebelum mengetik.
- Kunci ukuran jendela setelah join; catat resolusi; deteksi resize dan pulihkan bila perlu (meski resize sendiri menimbulkan infraction, pemulihan lebih baik).
- Focus keeper sebelum tiap aksi; jika blur terdeteksi > 2.5 s, fokuskan ulang.
- Klik kanan tidak dilakukan; menu konteks dibiarkan.
- Fullscreen: bila room meminta, klik tombol fullscreen native; jangan keluar.
- Log metrik: jumlah blur, durasi blur, resize, fullscreen, `navigator.webdriver`.

## Penyimpanan
`~/.local/share/waybot/sessions/<kode>-<timestamp>/`
- `meta.json` (room, nama, durasi, hash, versi app)
- `questions.raw.json`, `questions.json`
- `answers.json`, `answers.md`
- `events.ndjson`, `screenshots/`
- `result.json`
Tulis atomik (tmp + rename). Tidak menyimpan API key.

## Konfigurasi `.env`
```
DEEPSEEK_API_KEY=
DEEPSEEK_BASE_URL=https://api.deepseek.com
DEEPSEEK_MODEL=deepseek-v4.1-flash
DEEPSEEK_VISION_MODEL=
WAYBOT_PROFILE=
WAYBOT_HEADFUL=true
WAYBOT_OCR=tesseract
```
Semua bisa ditimpa argumen CLI.

## Observabilitas
- `tracing` level info ke file + layer TUI.
- Panel log menyaring rahasia.
- Crash hook menutup Firefox dengan rapi.

## Keamanan
- API key hanya dibaca env, tidak ditampilkan.
- Profil browser terpisah dari profil harian.
- Tidak mengunggah soal ke selain endpoint DeepSeek yang dikonfigurasi.

## Pipeline kunci jawaban (revisi)
1. Ekstensi `document_start` memasang hook `WebSocket` (log frame) dan `fetch`/XHR (log request+response) ke `window.__waybotNet`.
2. Join lewat UI → hook menangkap request `v5/join` (berisi `serverId`, `socketId`, `ip`, `uid`) dan/atau frame `join`/`rejoin` (berisi `room.quizId`).
3. Ambil `quizId` Mongo; panggil `GET /_quizserver/main/v2/quiz/<quizId>?...&source=join` dari konteks halaman.
4. Normalisasi `structure.answer` menjadi kunci per `questionId`:
   - MCQ → teks opsi benar (`options[answer].text`).
   - MSQ → daftar teks opsi benar.
   - BLANK → teks dari `options[optionId]` per `targetId`.
   - OPEN → `explain`.
5. Jika langkah 3/4 gagal, pakai `explain` sebagai hint lalu DeepSeek.
6. Kunci disimpan di `answers.json` dengan `source: "key" | "explain" | "ai"` dan confidence.

Catatan: `getQuestions` tetap dipakai untuk daftar soal/tipe/media; kunci diambil dari quiz data. Semua pencocokan opsi memakai `data-cy=option-<indeksAPI>` dan teks `aria-label`, bukan ID opsi.

## Format submit per tipe (dari payload nyata)
- MCQ: `response: <indeks>`, `answer: []`.
- MSQ: `response: [<indeks>…]`.
- BLANK: `response: {media:null}`, `answer: [{type:"BlankTargetObject", descriptor:"Answer", value:[{targetId, value:{text}}]}]`.
- OPEN: `response: {version:"2.0", text:"<p>…</p>", media:null, sttResponse:null}`, `answer: []`.
- Umum: `responseType:"original"`, `timeTaken` ms, `state:"attempted"`, `provisional` (skor), `quizVersionId`, `isLastPlayerResponse`.
- Endpoint: `POST /_gameapi/main/public/v1/games/<hash>/proceed`; fallback `/play-api/v4/proceedGame`.
- Verifikasi: `marks > 0` = benar; `rows` di `player-summary` dipakai untuk ringkasan.

## Editor OPEN
- Editor Tiptap `div.ProseMirror[contenteditable=true]`; isi via native send keys.
- Submit baru aktif setelah event input; tunggu `disabled=false` dan klik ulang bila perlu.
- Simpan HTML paragraf sesuai format `version:"2.0"`.

## Penanganan advance soal
- Deteksi dua mode: `manual-review` (ada “Answer Explanation”/“Next question”) dan `auto-advance` (soal berganti sendiri).
- Mode manual: setelah verifikasi submit, jeda manusia 0.8–1.6 s, klik `div` “Next question”, tunggu nomor soal berubah.
- Mode auto: tidak klik apa pun; tunggu `[data-testid=current-question-number]` berubah dengan timeout.
- Jangan pernah mengklik “Next question” sebelum jawaban terkirim (tombol bisa tidak ada).
- Budget pacing tetap dihitung; advance manual menambah waktu dan harus diperhitungkan.

## Spesifikasi input TUI (dikunci)
Layar: `Setup → Konfirmasi → Lobby → Quiz → Ringkasan`. Semua keyboard-only, tanpa mouse,
alternate screen + restore saat panic. Teks Indonesia, tanpa emoji.

- **Setup** (satu-satunya layar berinput): wajib `Kode Room` (3–11 alfanumerik), `Nama`
  (1–60, trim, tanpa karakter kontrol), `Durasi` (`mm:ss`, 0:30–60:00, ≥ jumlah_soal×3 s
  setelah harvest). Panel "Lanjutan" (`F3`, nilai awal dari `.env`/flag): provider, model
  (dari `GET /models`, cache), thinking, reasoning effort, vision, OCR, fullscreen, profil,
  jeda min/maks. Error inline di kanan field; `Enter` nonaktif sampai valid; `Tab`/`↑↓`
  pindah; `Esc` kosongkan field, keluar bila kosong.
- **Flag** (melewati Setup): `--room --name --duration --model --base-url --thinking
  --reasoning-effort --vision/--no-vision --ocr --fullscreen --profile --jeda-min --jeda-maks
  --yes`. Flag menang atas `.env`; sisa field jatuh ke Setup.
- **Konfirmasi**: ringkasan + peringatan fokus; `Enter` lanjut, `Esc` kembali utuh.
- **Lobby**: read-only, status join, jumlah soal, cakupan sumber jawaban, banner SIAP.
- **Quiz**: read-only, input dikunci; hanya `Ctrl-C` (konfirmasi) dan `F2`. Keeper menahan
  diri 5 s saat terminal difokuskan untuk abort, blur ditandai `manual`, lalu fokus kembali.
- **Ringkasan**: metrik + path sesi; `R` ulang, `Esc` keluar.
- Aksesibilitas: hormati `NO_COLOR`, minimum 80×24.
- API key hanya status terbaca/absen, tidak pernah ditampilkan.

## AnswerPolicy / AdvancePolicy (dikunci)
Menjawab dan maju dipisah. Loop pengamat 200–400 ms mengklasifikasi snapshot DOM/store/URL;
aksi hanya dijalankan bila precondition terlihat.

`AdvancePolicy` prioritas atas→bawah:
1. indeks soal berubah sendiri → `auto`, tanpa klik;
2. ack terkirim + Next ada & enabled & stabil ≥250 ms → klik sekali;
3. ack terkirim + feedback tampil, Next belum ada → tunggu (indeks berubah = auto);
4. Next `disabled` → tunggu sampai enabled atau sisa waktu habis;
5. interstisial → tunggu, tanpa klik buta;
6. dialog infraction/fullscreen → tangani lalu lanjut;
7. tak ada perubahan > batas → retry Next ≤2× backoff, catat `stuck`.

Guard: submit hanya bila `questionId` berbeda dari terakhir dijawab dan belum di-ack; satu soal
= maksimal satu kirim dan satu klik; ack diverifikasi; mode room hanya diinferensi untuk
menyetel timeout default, snapshot tetap dievaluasi ulang. Transisi ditulis ke `events.ndjson`.
Mode diuji lewat fixture snapshot (auto, review, feedback-lalu-auto, Next disabled, interstikal,
stuck) sebelum uji live.

## Revisi browser (dikunci)
- Fullscreen dimasukkan **sebelum** navigate/join; ukuran tidak diubah lagi setelah join.
- Satu window/tab dipakai ulang (`WebDriver:GetWindowHandles`), bukan tab baru per run.
- Profil persisten `~/.local/share/waybot/profile`; `last-session.json` hanya untuk resume opt-in.
- Penanganan hambatan generik tiap tick: popup login, cookie/consent, tombol Continue/Got it, prompt fullscreen.
- Kegagalan resume jatuh ke join normal.

## Implementasi final (koreksi atas rencana awal)
- **Tidak ada crop**: screenshot viewport penuh 1920x1080 PNG, dikirim bersama snapshot DOM.
- **Agen memimpin**: `run_agent` membaca halaman, memilih aksi, divalidasi, dieksekusi, diverifikasi; maksimum 10 langkah, 4 aksi tidak valid.
- **Vision paralel dengan jeda**: panggilan gambar berjalan bersamaan dengan jeda manusia; bila gagal, aksi memakai DOM + jawaban tersimpan.
- **Jawaban awal otoritatif saat ragu** lewat penanda `doubt` dan `suggested_indices`.
- **Pengetikan berpotongan** (2 karakter lambat + potongan 10-14 karakter).
- **Satu jendela**: jendela ekstra ditutup; `reconnect` memakai jendela Wayground yang sudah ada, tanpa tab baru.
- **Deteksi akhir**: 15 detik tanpa soal pada halaman akhir, atau aksi `done`.
- **Metrik**: latensi vision per langkah, `action_ms`, `answer_ms`, jumlah langkah agen, metrik anti-deteksi.
