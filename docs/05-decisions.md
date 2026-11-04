# 05 — Keputusan Teknis (ADR ringkas)

## ADR-01: Marionette mentah, bukan geckodriver
- **Keputusan**: klien Marionette TCP langsung.
- **Alasan**: tanpa binary tambahan, kontrol penuh (chrome context, native input), lebih sedikit bagian yang bisa terdeteksi.
- **Konsekuensi**: implementasi framing/protocol sendiri; butuh tes ketahanan.

## ADR-02: Ekstensi preload sementara
- **Keputusan**: install WebExtension minimal via `Addon:Install`; page script di `document_start`.
- **Alasan**: `navigator.webdriver=false` tidak bisa lewat pref; BiDi preload belum berfungsi pada uji.
- **Konsekuensi**: satu ekstensi bisa memicu `extensionDetected` (premium); diminimalkan dan dihapus saat browser ditutup.

## ADR-03: Join lewat UI, bukan HTTP
- **Keputusan**: gunakan form dan tombol asli.
- **Alasan**: alur socket/resume paling lengkap; kecil kemungkinan beda dengan klien resmi.
- **Konsekuensi**: lebih lambat dan rapuh terhadap DOM; selector dipusatkan.

## ADR-04: getQuestions lewat fetch halaman
- **Keputusan**: panggil endpoint dari konteks halaman setelah join.
- **Alasan**: memakai cookie/header asli; soal lengkap lebih awal.
- **Konsekuensi**: fallback ke DOM/store wajib ada.

## ADR-05: Solver DeepSeek + fallback heuristik
- **Keputusan**: DeepSeek untuk jawaban, dengan JSON ketat dan confidence.
- **Alasan**: kunci tidak tersedia untuk pemain.
- **Konsekuensi**: butuh validasi output, retry, dan jawaban cadangan.

## ADR-06: Pacing berbasis anggaran
- **Keputusan**: target 8 detik + jitter, tetapi dibatasi anggaran total dinamis.
- **Alasan**: memenuhi kesan manusia sekaligus batas 3 menit.
- **Konsekuensi**: jika waktu mepet, prioritas penyelesaian.

## ADR-07: Penyimpanan per sesi
- **Keputusan**: folder sesi dengan JSON/Markdown, tulis atomik.
- **Alasan**: audit, resume, bukti.

## ADR-08: Fullscreen opsional
- **Keputusan**: masuk fullscreen hanya bila diminta room atau diaktifkan pengguna; jangan keluar.
- **Alasan**: infraction fullscreen exit.

## ADR-09: Kunci jawaban dari quiz data
- **Keputusan**: ambil `quizId` dari hook join/rejoin, lalu `GET /_quizserver/main/v2/quiz/<id>?source=join` dan ekstrak `structure.answer`.
- **Alasan**: kunci resmi tersedia publik dan jauh lebih akurat daripada menebak; `getQuestions` tidak memuat kunci.
- **Konsekuensi**: butuh hook jaringan `document_start`; fallback `explain` + DeepSeek bila gagal.

## ADR-10: Pemetaan opsi lewat `data-cy` + teks
- **Keputusan**: opsi dicocokkan dengan indeks API dari `data-cy="option-<index>"` dan diverifikasi teks `aria-label`.
- **Alasan**: urutan visual diacak; ID opsi tidak muncul di DOM.
- **Konsekuensi**: parser harus memetakan kunci (`structure.answer`) ke indeks DOM yang sama.

## ADR-11: Submit lewat UI
- **Keputusan**: gunakan klik/ketik UI, biarkan aplikasi membentuk payload `proceed`.
- **Alasan**: payload memuat skor/timeTaken/provisional yang dihitung klien; lebih kecil risiko ditolak.
- **Konsekuensi**: butuh verifikasi ack dan penanganan tombol nonaktif.

## ADR-12: TUI minimal + panel Lanjutan
- **Keputusan**: hanya kode/nama/durasi di panel utama; opsi lain di balik `F3`, nilai awal dari `.env`/flag; input dikunci saat Quiz.
- **Alasan**: alur cepat, risiko salah ketik kecil, mencegah ketikan terminal mengganggu browser.
- **Konsekuensi**: flag CLI wajib untuk uji berulang; keeper diberi jeda 5 s saat user abort.

## ADR-13: Pemisahan AnswerPolicy dan AdvancePolicy
- **Keputusan**: keputusan maju soal tidak dicampur dengan eksekusi jawaban; prioritas berbasis snapshot + guard + retry berbatas.
- **Alasan**: room dapat mencampur auto-advance, review+Next, dan interstisial; urutan tidak boleh di-hardcode.
- **Konsekuensi**: butuh fixture snapshot, `events.ndjson`, dan indikator mode di TUI.

## ADR-14: Fullscreen sebelum join
- **Keputusan**: masuk fullscreen sebelum navigasi/join (chrome `win.fullscreen`, fallback `niri msg action fullscreen-window`), default aktif, `--no-fullscreen` untuk mematikan.
- **Alasan**: tile/resize setelah join memicu `windowResizeDetected`; fullscreen lebih awal menghilangkannya dan mengurangi `fullscreenExitDetected`.
- **Konsekuensi**: fokus/keeper harus mempertahankan fullscreen sampai selesai; jangan pernah keluar sendiri.

## ADR-15: Profil persisten dan satu jendela
- **Keputusan**: profil Firefox tetap di `~/.local/share/waybot/profile`, pakai window/tab yang sudah ada, simpan `last-session.json` dan resume bila room sama (<15 menit); hambatan UI ditutup otomatis.
- **Alasan**: join berulang memicu dialog resume, onboarding, dan risiko deteksi; uid/cookie stabil lebih manusiawi.
- **Konsekuensi**: proses lama dibersihkan dengan SIGTERM lalu SIGKILL; adopsi proses tidak dipakai karena sesi Marionette lama tidak andal.

## ADR-16: Agen halaman berbasis gambar memimpin jawaban
- **Keputusan**: saat kuis berjalan, model membaca screenshot viewport penuh + snapshot DOM lalu memilih satu aksi; deterministik menjadi jalur cepat/cadangan.
- **Alasan**: aturan per tipe rapuh ketika urutan soal berbeda atau layout berubah.
- **Konsekuensi**: butuh validasi aksi, verifikasi hasil, batas langkah, dan log latensi.

## ADR-17: Jawaban awal mengalah saat ragu
- **Keputusan**: agen menandai `doubt`; bila true, opsi dikoreksi ke jawaban yang disiapkan saat join (hanya soal itu).
- **Alasan**: jawaban sudah dihitung sebelum kuis; screenshot hanya untuk menemukan kontrol.
- **Konsekuensi**: penyimpangan tetap dicatat untuk audit.

## ADR-18: Screenshot penuh 1920x1080 PNG
- **Keputusan**: kirim viewport penuh, resolusi asli, lossless; tanpa crop.
- **Alasan**: hasil uji menunjukkan latensi vision tetap 1,4-7,8 detik dan akurasi lebih baik pada soal bergambar.
- **Konsekuensi**: token lebih tinggi; panggilan vision dibatasi waktu dan dijalankan paralel dengan jeda.

## ADR-19: Pengetikan berpotongan
- **Keputusan**: dua karakter awal 45-140 ms, sisanya potongan 10-14 karakter dengan jeda 25-60 ms.
- **Alasan**: esai 367 karakter turun dari ~34 detik ke ~6 detik tanpa clipboard.
- **Konsekuensi**: tetap event per karakter; verifikasi isi dan fallback `insertText` dipertahankan.

## ADR-20: Deteksi akhir sesi
- **Keputusan**: sesi dianggap selesai bila tidak ada soal dan halaman berstatus akhir selama 15 detik, atau agen mengembalikan `done`.
- **Alasan**: mencegah perulangan di layar survei/ringkasan setelah kuis habis.
- **Konsekuensi**: ringkasan sesi ditulis dan browser ditutup.

## ADR-21: Sesi baru secara default
- **Keputusan**: setiap peluncuran membuat sesi baru; resume hanya dengan toggle `Resume sesi` atau `--resume`, dan hanya untuk URL game/pre-game room yang sama (<15 menit).
- **Alasan**: resume ke halaman beranda sempat membuat program melewati join dan macet.
- **Konsekuensi**: `last-session.json` dihapus saat start tanpa resume; layar percobaan menggantung memakai Start Over.

## ADR-22: Identitas soal berbasis nomor
- **Keputusan**: kunci kunjungan, sidik jari deduplikasi, dan kunci percobaan memakai nomor soal + teks + opsi terurut.
- **Alasan**: soal kembar dan frame transisi memicu jawaban ganda sekaligus soal terlewat.
- **Konsekuensi**: nomor soal wajib ikut dikirim; tanpa nomor, deduplikasi memakai teks/opsi saja.

## ADR-24: Penutupan di soal terakhir
- **Keputusan**: bila soal terakhir sudah terjawab dan tidak ada kemajuan 20 detik, sesi ditutup.
- **Alasan**: host bisa tidak menampilkan ringkasan sehingga sesi menunggu sampai batas waktu.
- **Konsekuensi**: ringkasan dan metrik tetap tersimpan.

## ADR-23: Hitungan waktu mulai saat soal terlihat
- **Keputusan**: timer TUI mulai pada event soal pertama, bukan saat jawaban pertama.
- **Alasan**: room bisa berstatus running sementara soal belum tampil.
- **Konsekuensi**: fase lobby menampilkan sisa penuh.
