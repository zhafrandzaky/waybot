# 01 — Riset Wayground / Quizizz (temuan mentah)

Status: riset langsung pada 2026-11-03/04, room uji `501008` (selesai) dan `930417` (aktif, menunggu start).
Bukti JSON disimpan di `docs/evidence/`. Alat probe di `docs/research-probe/` (Marionette mentah).

## 1. Identitas aplikasi
- Domain baru: `wayground.com`; merek lama Quizizz. Aset aplikasi tetap di `cf.quizizz.com`.
- Halaman join adalah SPA Vue + Pinia yang di-bundle dari `https://cf.quizizz.com/app-bundle/join/upload/js/index.*.js`.
- API produksi:
  - `https://wayground.com/play-api/...` (join/answer flow)
  - `https://wayground.com/_api/main/...` (auth/user)
  - `https://wayground.com/_gameapi/main/public/...` (game metadata)
  - `https://wayground.com/_quizserver/main/...` (quiz service)
- Socket realtime: `wss://socketv2.wayground.com/_gamesocket/main/ws` (game) dan `wss://socket.wayground.com/_gsocket/main/` (engine.io/socket.io). Answer sendiri dikirim lewat HTTP, bukan socket.
- Room hash 24 hex didapat dari `checkRoom`. URL game memakai blob terenkripsi CryptoJS (AES) berisi `roomHash`, `playerId`, `quizId`, `gameState`, dsb.

## 2. Alur join yang teramati
1. `/join` → input `input[data-cy=gamecode-field]`, tombol `button[data-cy=joinGame-button]`.
2. Klik join → `POST /play-api/v5/checkRoom` body `{"roomCode":"<kode>"}`.
   - Sukses: `{success:true, room:{hash, code, state, options, questions:[...], totalAnswerableQuestions, metadata.qm.<qid>.time}}`.
   - Gagal: `{"success":false,"error":{"message":"Room not found","type":"room.NOT_FOUND"}}` (HTTP 200 + error di body).
3. Bila room `waiting`, aplikasi pindah ke `/join/pre-game/running/<encrypted>/start`.
   - Layar nama: `input[data-cy=enter-name-field]`, tombol `button[data-cy=start-game-button]`.
   - Opsi di layar: tema, read aloud, sound effects.
4. Nama duplikat pada room berjalan → dialog “An incomplete game already exists with this name.” dengan tombol `No` / `Yes` (resume).
5. Klik Start mengirim join lewat socket; aplikasi pindah ke `/join/game/<encrypted>?gameType=live`.
6. Room `waiting` menampilkan “Waiting for the host to start...”.
7. Setelah host start, `room.state = running`, pemain menerima soal.

Catatan: join lewat HTTP juga tersedia (`POST /play-api/v5/join`, `POST /play-api/v5/rejoin`); payload lengkap belum diambil karena UI sudah berhasil. Pendekatan aman: **join lewat UI**.

## 3. Endpoint kunci

### `POST /play-api/v5/checkRoom`
Body: `{"roomCode":"501008"}`. Tanpa auth. Mengembalikan seluruh opsi room.
Field penting: `room.hash`, `room.state`, `room.options`, `room.metadata.qm.<questionId>.time` (contoh 30000 ms), `totalAnswerableQuestions`.

### `POST /play-api/v4/getQuestions`
Body: `{"roomHash":"<hash>","playerId":"<nama>"}`.
Berhasil meski room masih `waiting` dan tanpa cookie. Mengembalikan **semua soal** + struktur lengkap.
**Tidak ada flag jawaban benar.** Contoh lengkap: `docs/evidence/getQuestions-sample.json`.

Bentuk soal:
```json
{
  "_id": "…24hex…",
  "type": "MCQ|MSQ|BLANK|OPEN|DROPDOWN|REORDER|MATCH|DRAGNDROP|DND_IMAGE|HOTSPOT|…",
  "structure": {
    "query": {"text": "<html>", "media": []},
    "options": [{"id":"…","text":"…","media":[],"type":"text"}],
    "targets": [{"id":"…","optionId":["…"]}],
    "settings": {"doesOptionHaveMultipleTargets": false, "fibDataType":"string", "hasCorrectAnswer":true},
    "explain": {"text":"<sering membocorkan jawaban>"},
    "marks": {"correct":1,"incorrect":0}
  },
  "time": 30000
}
```
- `explain` pada room uji sering menyebut jawaban (mis. “Monitor merupakan perangkat output…”) dan dapat dipakai sebagai kunci/verifikasi.
- `BLANK`: `options` kosong, `targets[].optionId` menunjuk ID opsi yang tidak disertakan; jawaban harus diisi teks.
- `OPEN`: `options` berisi placeholder `"-"`; jawaban teks bebas.
- `MSQ`: multi jawaban; `settings.doesOptionHaveMultipleTargets`.
- `POLL`/tanpa jawaban: `marks.correct = 0`.
- `query.text` berisi HTML + `<img src=… alt=…>`; alt bisa membantu bila gambar.

### Endpoint jawaban
- `POST /play-api/v2/update-response`
  Payload (dari bundle `index.*.js`):
  ```json
  {
    "questionId": "<qid>",
    "roomHash": "<hash>",
    "playerId": "<nama>",
    "response": {
      "questionId": "<qid>",
      "attempt": 0,
      "state": "ATTEMPTED",
      "…field skor/akurasi…",
      "response": [<jawaban>]
    }
  }
  ```
  `response` berisi index/ID opsi untuk MCQ/MSQ dan struktur teks untuk BLANK/OPEN.
- `POST /play-api/v4/proceedGame` (atau `/v1/games/<hash>/proceed`) untuk lanjut.
- `POST /play-api/v1/games/<hash>/player-infraction` untuk anti-cheat premium.
- Endpoint metadata: `GET /_gameapi/main/public/v1/games/<hash>/quiz-info`, `GET /_gameapi/main/public/v1/games/<hash>` (butuh sesi pemain), `GET /_quizserver/main/v2/quiz/<quizId>` (butuh ID Mongo asli; hash game tidak diterima).

### Kunci jawaban
- Klien pemain **tidak** menerima kunci. Jawaban dinilai server; `isCorrect` baru ada setelah submit.
- `explain` kadang membocorkan jawaban.
- `GET /_quizserver/main/v2/quiz/<id>` (dipakai layar game) bisa memuat `isCorrect`, tetapi ID yang dipakai berbeda dari hash game dan butuh otorisasi; riset lanjutan diperlukan.

## 4. DOM dan selector yang terverifikasi
- Kode room: `input[data-cy=gamecode-field]`, tombol `button[data-cy=joinGame-button]`.
- Nama: `input[data-cy=enter-name-field]`, tombol `button[data-cy=start-game-button]`.
- Dialog resume: teks “incomplete game already exists”, tombol `No`/`Yes`.
- Waiting: teks “Waiting for the host to start...”.
- Opsi jawaban (dari bundle `Option.*.js`): atribut `data-testid="option-<optionId>"`.
- Opsi drag/drop: `data-testid="dnd-option-<optionId>"`, wadah `data-testid="dnd-options-container"`.
- Tombol submit: `button[data-cy=submit-button]`; label berubah `Submit`/`Next`/`Proceed`/`Save`.
- Nomor soal: `data-testid="current-question-number"`, total: `data-testid="total-question-number"`.
- Grid opsi: `data-testid="options-grid"`.
- Reorder: `data-testid="reorder-option-<index>"`; Match: `data-testid="match-option-card-text"`.
- Tipe lain (BLANK/OPEN/DROPDOWN/HOTSPOT/DND_IMAGE) memakai kelas internal (`fib-*`, `dropdown-*`, dsb.) yang perlu diverifikasi di sesi live.

## 5. Anti-cheat (kode asli)
Modul: `cf.quizizz.com/app-bundle/join/upload/js/useAntiCheating.*.js`.

Jenis pelanggaran yang dikirim ke server:
`pasteDetected`, `windowResizeDetected`, `playerExited`, `playerResumed`, `rightClickDetected`, `fullscreenExitDetected`, `extensionDetected`, `googleLens`.

Mekanisme:
- `playerExited` saat blur/tab hidden; `playerResumed` bila kembali. Ambang alert **3 detik** (`lt=3000`).
- Resize jendela memakai debounce 500 ms; Google Lens dideteksi bila lebar menyusut 20–40% dalam 10 detik.
- `paste`/`copy` memicu infraction (kecuali dinonaktifkan opsi).
- Klik kanan dihitung; bila `disableRightClick` aktif, default dicegah.
- Keluar fullscreen memicu infraction; `useScreenfull` dipantau.
- Deteksi ekstensi memicu `extensionDetected` (premium).
- Hanya dikirim bila `options.antiCheating.enabled && options.antiCheating.alertOnInfraction` atau `focusMode`. Pada room uji: `enabled:true`, `alertOnInfraction:false` → tidak ada alert, tetapi mekanisme tetap berjalan dan bisa aktif pada room premium.

Implikasi:
- Jangan paste; ketik per karakter.
- Jangan resize setelah join.
- Jaga fokus < 3 detik blur; fokuskan jendela sebelum aksi.
- Jangan klik kanan.
- Pertahankan fullscreen jika diminta.
- Preload `navigator.webdriver=false`; meski bukan vektor yang terlihat di modul anti-cheat, tetap dilakukan.

## 6. Deteksi otomasi lain
- Firefox yang dikendalikan WebDriver menyetel `navigator.webdriver = true` walau pref `dom.webdriver.enabled=false`.
- Solusi teruji: ekstensi sementara dengan content script `document_start` yang menyuntik `page.js` ke konteks halaman; hasil uji `navigator.webdriver === false` dan penanda preload muncul.
- Tidak ada dependensi geckodriver: Marionette mentah (TCP 127.0.0.1) sudah diuji dan cukup (native click/send keys, chrome context, addon install).
- Firefox 158: `--remote-debugging-port` kini membuka **WebDriver BiDi**, bukan CDP; `/json/*` 404. `script.addPreloadScript` BiDi belum berhasil pada uji singkat → ekstensi lebih andal.
- `--remote-allow-system-access` diperlukan untuk `Marionette:SetContext(chrome)`.
- Fokus jendela dapat dipulihkan lewat chrome context `BrowserWindowTracker.getTopWindow().focus()` (teruji).

## 7. Timing
- Tiap soal punya `time` (contoh 30000 ms).
- `options.timer=false`, `timer_3="off"` pada room uji, tetapi metadata tetap 30 detik/soal.
- Target pengguna: ~8 detik/soal, total 10 soal ≤ 3 menit. Scheduler harus menghitung dari durasi input dan jumlah soal.

## 8. Risiko yang ditemukan
- Kode room kedaluwarsa/berhenti; join setelah start bisa ditolak kecuali resume dengan nama sama.
- Nama duplikat memunculkan dialog resume.
- Room bisa `loginRequired` (butuh akun).
- Soal gambar butuh OCR/vision; alt text tidak selalu ada.
- `getQuestions` tanpa auth berhasil pada room uji, tetapi bisa berubah.
- Selector internal bisa berubah; pusatkan di modul selector dan sediakan fallback.
- Enkripsi URL bukan penghalang; jangan mengandalkan parsing URL.
- Fullscreen/exit dan blur adalah vektor anti-cheat utama.
- Ekstensi bisa memicu `extensionDetected` (premium); gunakan satu ekstensi minimal.
- Jendela terminal yang difokuskan pengguna membuat browser blur; perlu focus keeper.

## 9. Join HTTP (temuan tambahan)
Join sebenarnya juga lewat HTTP: `POST /play-api/v5/join`. Payload aktual (room uji):
```json
{"roomHash":"…","player":{"id":"<nama>","name":"","origin":"web","isGoogleAuth":false,"avatarId":20,"startSource":"joinRoom","userAgent":"…","uid":"<uuid>","expName":"main_main","expSlot":"16"},"powerupInternalVersion":"20","serverId":"…","ip":"…","user-agent":"…","socketId":"…","authCookie":null,"socketExperiment":"authRevamp"}
```
`socketId`, `serverId`, dan `ip` didapat dari handshake socket; `uid` dari local storage. Respons sama dengan checkRoom + data player. Untuk guest, `playerId == nama`.

## 10. DOM layar soal (terverifikasi live, room 930417)
- Wadah soal: `div.quiz-container[data-cy=quiz-container][data-quesid="<qid>"]`.
- Atribut tipe pada wadah: `isquestionmatchandreorderbased`, `isquestionorderbased`, `isquestionmatchbased`, `slidesflowcontroltype`.
- Nomor soal: `[data-testid=current-question-number]`; total: `[data-testid=total-question-number]`.
- Opsi: `button.option[data-cy="option-<index>"]` (bukan `data-testid` di tema ini).
  - `role="option"`, `aria-label="Option 1, <teks>"`, `aria-selected="false|true"`.
  - Grid: `.options-grid[role=listbox]`, `aria-multiselectable="true"` untuk MSQ, kelas opsi memuat `is-msq`.
- Label instruksi MSQ: “Select all correct options”.
- BLANK: `input[data-cy=fib-text-input]` (`placeholder="Type answer..."`, `maxlength=1000`).
- Submit: `button[data-cy=submit-button]`; nonaktif sampai ada jawaban; label `Submit`.
- Review setelah jawab/timeout: teks “Answer Explanation” berisi `explain`; tombol Next berupa `div` bertekst `Next question` (bukan `<button>`).
- Tombol fullscreen: `button[aria-label=Fullscreen]`.
- Room uji **tidak auto-advance** setelah waktu habis; menunggu “Next question”. Jadi pemain mengatur perpindahan soal.
- Timer soal terlihat (hitung mundur 30 → 0), lalu masuk mode review.
- Setelah jawaban dikirim, muncul review + tombol Next; tidak ada navigasi otomatis.

Catatan: `data-testid="option-<optionId>"` ada di kode komponen, tetapi DOM aktual memakai `data-cy` berbasis indeks. Pencocokan jawaban harus memakai teks/`aria-label`, bukan ID opsi dari API (ID opsi tidak muncul di DOM).

## 11. Payload jawaban aktual (terverifikasi live)
Submit jawaban memakai `POST /_gameapi/main/public/v1/games/<roomHash>/proceed` (jalur migrasi; fallback `POST /play-api/v4/proceedGame`). Payload berisi objek `response`:

MCQ:
```json
{"questionId":"…","questionType":"MCQ","response":1,"responseType":"original","timeTaken":9404,"answer":[],"state":"attempted","provisional":{…}}
```
`response` = indeks opsi API (0-based), yang juga tampil sebagai `data-cy="option-<index>"` pada DOM (opsi visual diacak, indeks tetap).

MSQ: `response` berupa array indeks, mis. `[3,1]`.
BLANK:
```json
{"questionType":"BLANK","response":{"media":null},
 "answer":[{"type":"BlankTargetObject","descriptor":"Answer",
   "value":[{"targetId":"<blank-id>","value":{"text":"Central Processing Unit"}}]}]}
```
OPEN:
```json
{"questionType":"OPEN","response":{"version":"2.0","text":"<p>…</p>","media":null,"sttResponse":null},"answer":[]}
```
Semua menyertakan `provisional.scores` dan `scoreBreakups` yang dihitung klien, `quizVersionId`, `powerupEffects`, `isLastPlayerResponse`.

Respons server mengembalikan `marks` (1 = benar), `metadata.wrongIndices`/`wrongTargetOptions`, leaderboard, dan `question.structure`.

Catatan UI:
- MCQ auto-submit saat opsi diklik.
- MSQ butuh tombol Submit.
- BLANK: isi `input[data-cy=fib-text-input]`, lalu Submit (tombol baru aktif setelah teks).
- OPEN: editor Tiptap `div.ProseMirror[contenteditable=true]`; tombol Submit baru aktif setelah teks; klik Submit kadang perlu diulang setelah event input selesai.
- Setelah submit/timeout: panel “Answer Explanation” + `div` “Next question”.
- Halaman terakhir: “THE END”, lalu `playerGameOver`, rejoin `state=completed`, dan `player-summary`.

## 12. Kunci jawaban resmi (PENTING)
`GET /_quizserver/main/v2/quiz/<quizIdMongo>?bypassProfanity=false&returnPrivileges=false&source=join` (publik, tanpa auth) mengembalikan `data.quiz.info.questions[]` lengkap dengan:
- `structure.answer`:
  - MCQ: indeks jawaban benar (`1`).
  - MSQ: array indeks (`[0,2,3]`).
  - BLANK: `[{"targetId":"…","optionId":["…"]}]`; teks benar ada di `structure.options[]` (yang **tidak** dikirim oleh `getQuestions`).
  - OPEN: `null`; gunakan `explain`.
- `structure.options[]` lengkap dengan `text`.
- `explain`, `time`, media.

`quizId` Mongo tidak ada di `checkRoom`/`getQuestions`. Sumber yang teramati:
- respons `join`/`rejoin` di store socket memuat `room.quizId`;
- respons `POST /play-api/v6/rejoin` (HTTP) memuat `data.room.quizId` + `versionId`;
- URL game hanya berisi `roomHash|playerId` (AES CryptoJS, passphrase `"game"`), bukan quizId.
- `getGameData` butuh otorisasi (403 tanpa kredensial game).

Rencana pengambilan: hook `WebSocket`/fetch di `document_start` lewat ekstensi → baca join/rejoin → ambil `quizId` → fetch quiz data → simpan kunci. Fallback: DeepSeek bila fetch gagal.

## 13. Perilaku kuis live yang teramati
- Soal dan opsi **diacak** (`jumble:true`, `jumbleAnswers:true`). Urutan `getQuestions` ≠ urutan tampil.
- Pemetaan opsi memakai `data-cy=option-<indeksAPI>`; teks opsi ada di `aria-label`.
- Timer soal 30 detik; setelah habis masuk mode review, **tidak** auto-advance. “Next question” harus diklik.
- Room uji: `timer=false`, tetapi hitung mundur tetap berjalan dan berhenti di review.
- Gambar dirender tanpa `alt` (alt dari API hilang), jadi butuh vision/OCR.
- Setelah game selesai, `player-summary` memberi `correct/incorrect/timedout/yetToBeGraded`.
- `getQuestions` mengembalikan soal tanpa `answer` dan tanpa `options` untuk BLANK.

## 14. Pengaturan room vs perilaku review (room 629787)
Room 629787 punya opsi sama seperti 823944:
```json
{"showAnswers":true,"showAnswers_2":"never","studentQuizReview":true,"studentQuizReview_2":"no","timer":false,"timer_3":"off","preventSkipping":true}
```
Perilaku: setelah menjawab/timeout, muncul “Answer Explanation” + tombol “Next question”; **tidak auto-advance**.

Kesimpulan:
- `timer=false` (dan `timer_3="off"`) = tidak ada timer ketat/auto-progress; pemain menekan Next.
- `showAnswers_2="never"` saja tidak menghilangkan layar penjelasan; `showAnswers=true` masih aktif.
- Untuk perilaku “klik jawaban → langsung soal berikutnya”, host perlu:
  1. menyalakan timer soal (Classic/Strict, mis. 30 detik), atau
  2. mematikan “Show answers”/review, dan
  3. memastikan mode bukan student-paced manual.
- Program harus menangani **dua-duanya**: bila `Next question` muncul, klik dengan jeda manusia; bila auto-advance, tunggu soal berikutnya tanpa klik.

`quizId` tetap tidak muncul di respons `join`/`rejoin` HTTP saat state `waiting`. Kemungkinan besar baru muncul di data socket setelah host mulai. Rencana: hook jaringan + ambil kunci saat start, sebelum jawaban pertama.
