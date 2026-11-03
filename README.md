# Waybot

CLI terminal layar penuh (Rust) yang mengotomatisasi sesi kuis [Wayground](https://wayground.com) lewat Firefox asli yang terlihat. Pengguna memasukkan kode room, nama, dan durasi; program bergabung lewat UI, menyiapkan seluruh jawaban sebelum kuis dimulai, lalu menjawab tiap soal memakai agen yang membaca halaman.

## Fitur

- TUI lima layar (`Setup → Konfirmasi → Lobby → Quiz → Ringkasan`), keyboard saja, tanpa mouse.
- Firefox headful dengan profil persisten, satu jendela, dan fullscreen sebelum join.
- Join lewat form asli, termasuk dialog "resume attempt" dan nama yang sudah diingat browser.
- Mengambil daftar soal lewat klien halaman setelah join, menyimpan soal dan jawaban ke folder sesi.
- Jawaban dibuat sebelum kuis dimulai memakai provider OpenAI-compatible (Venice atau DeepSeek) dan disimpan dengan label sumber serta confidence.
- Saat kuis berjalan, agen membaca screenshot viewport penuh (1920x1080 PNG) dan snapshot DOM, lalu memilih aksi: pilih opsi, isi editor, submit, next, atau tutup hambatan.
- Aksi divalidasi, diverifikasi, dan diulang terbatas; jawaban tersimpan dipakai saat bacaan gambar meragukan.
- Jeda berbasis anggaran waktu (target manusiawi, selesai sebelum durasi yang diinput).
- Anti-deteksi: `navigator.webdriver === false`, input native tanpa paste, tanpa resize setelah join, focus keeper, metrik dicatat.
- Resume otomatis bila proses terputus; sesi dapat dijalankan ulang.

## Kebutuhan

- Linux dengan sesi grafis (diuji pada Wayland/niri dengan XWayland aktif).
- Firefox terpasang dan dapat dijalankan.
- Rust 1.85+ (`cargo`).
- API key provider yang kompatibel dengan `chat/completions` dan mendukung JSON; model vision diperlukan untuk soal bergambar.

## Instalasi

```bash
cargo build --release
```

## Konfigurasi

Salin `.env.example` menjadi `.env` lalu isi:

```bash
DEEPSEEK_API_KEY=...
DEEPSEEK_BASE_URL=https://api.deepseek.com
DEEPSEEK_MODEL=deepseek-flash
DEEPSEEK_VISION_MODEL=deepseek-flash
DEEPSEEK_THINKING=auto
DEEPSEEK_REASONING_EFFORT=low
```

`DEEPSEEK_BASE_URL` dapat diarahkan ke penyedia lain yang kompatibel. Nilai rahasia tidak pernah ditampilkan di TUI maupun disimpan di folder sesi.

## Pemakaian

```bash
./target/release/waybot
```

Isi kode room, nama, dan durasi (`mm:ss`). `F3` membuka opsi lanjutan (model, thinking, vision, OCR, fullscreen, profil, jeda).

Untuk pengujian berulang, semua nilai dapat diberikan sebagai argumen:

```bash
./target/release/waybot --room 123456 --name Budi --duration 03:00 --yes
```

Alur: join → ambil soal → siapkan jawaban → tampilkan `SIAP — SILAKAN START QUIZ` → tunggu host → jawab otomatis → ringkasan dan file sesi.

## Struktur

```
src/
  main.rs        bootstrap, argumen, .env
  config.rs      validasi konfigurasi dan sesi
  tui.rs         layar dan input
  app.rs         orkestrasi sesi
  browser/       Firefox, Marionette, DOM, input, fokus
  wayground/     join, harvest, eksekusi jawaban, advance
  solver/        klien model, prompt, agen halaman
  store.rs       folder sesi
  anticheat/     pacing, metrik
docs/            riset, arsitektur, ADR, hasil uji
```

## Pengujian

```bash
cargo test
cargo clippy --all-targets -- -D warnings
```

## Batasan

- Urutan dan perilaku soal dapat berubah dari sisi Wayground; selector dipusatkan dan agen menjadi cadangan.
- Tipe eksotis (DROPDOWN, REORDER, MATCH, HOTSPOT, DRAGNDROP) baru didukung secara best-effort.
- Kunci jawaban resmi hanya tersedia bila `quizId` berhasil diambil; jika tidak, jawaban berasal dari `explain` atau model.
- Akurasi jawaban bergantung pada model; confidence dan sumber selalu dicatat.

## Lisensi

Belum ditentukan.
