# 00 — Konsolidasi Kebutuhan (SUMBER KEBENARAN)

Status: **DIIMPLEMENTASIKAN** dan sudah diuji live. Folder `docs/research-probe/` hanya alat riset, bukan bagian aplikasi.

## Ringkasan produk
CLI terminal full-screen (Rust + TUI) yang mengotomatisasi sesi kuis Wayground (dahulu Quizizz) memakai Firefox asli yang terlihat. Pengguna memasukkan kode room, nama, dan durasi pengerjaan; program membuka tab Firefox baru, membuka `wayground.com`, join dengan kredensial itu, mengambil seluruh soal + format jawaban, menyimpan semuanya, menunggu kuis dimulai, lalu menjawab tiap soal secara manusiawi (sekitar satu soal tiap 8 detik, dengan jitter) dan memastikan seluruh soal selesai sebelum batas waktu.

## Kebutuhan fungsional
1. CLI berbahasa Rust dengan UI terminal rapi, full-screen, navigasi keyboard.
2. Konfigurasi rahasia di `.env` (minimal `DEEPSEEK_API_KEY`, base URL, model).
3. Input dinamis saat runtime, tidak di-hardcode:
   - kode room,
   - nama tampilan,
   - durasi pengerjaan kuis (detik/menit),
   - opsional: model, jeda min/maks, mode jawab, jalur profil Firefox.
4. Program membuka **tab baru** di Firefox, membuka `https://wayground.com`, lalu join room dengan input pengguna. Browser harus jendela nyata (headful), bukan latar belakang/headless.
5. Setelah berhasil join, ambil **daftar lengkap soal** beserta tipe/opsi/media. Simpan ke disk sebelum kuis dimulai.
6. Bila kunci jawaban tersedia dari aplikasi, gunakan; bila tidak, jawab dengan DeepSeek API memakai model terbaru yang dikonfigurasi (default mengarah ke “v4.1 flash”). Semua jawaban disimpan lebih dulu.
7. Baca soal bergambar (media/gambar) memakai alt text, OCR, dan/atau model vision yang dikonfigurasi.
8. Tunggu sampai host memulai kuis; jangan menebak-nebak atau mengirim jawaban sebelum soal tampil.
9. Jawab soal satu per satu dengan ritme manusia: target ~8 detik/soal, jitter, klik/ketik native, tanpa paste.
10. Selesaikan seluruh soal sebelum batas waktu (contoh acuan: 10 soal / 3 menit). Perhitungan memakai durasi dinamis dan jumlah soal sebenarnya.
11. Cocokkan soal yang tampil dengan daftar tersimpan; verifikasi bahwa jawaban terkirim; lanjut ke soal berikutnya.
12. Tampilkan progres, timer, soal, jawaban, dan log di TUI.
13. Semua interaksi berjalan di dalam Firefox; CLI hanya orkestrator.

## Kebutuhan anti-deteksi
1. Firefox asli, profil persisten yang bersih, ukuran jendela tetap.
2. Sembunyikan `navigator.webdriver` sejak `document_start` (preload).
3. Klik dan ketik memakai event native/trusted, bukan `element.click()` murni.
4. Ketik per karakter dengan jeda acak; jangan pakai clipboard/paste.
5. Jaga fokus jendela Firefox; jangan biarkan blur lebih dari 3 detik.
6. Jangan resize jendela setelah join; jangan klik kanan; pertahankan fullscreen bila diminta.
7. Bila memungkinkan, gunakan respons API yang sama dengan klien resmi dari konteks halaman (cookie/header asli).
8. Hindari pola waktu yang persis sama; jitter pada semua jeda.

## Non-fungsional
- Tidak ada hardcode room/kode/soal/nama.
- Gagal dengan pesan jelas. Sesi baru secara default; resume tersedia opt-in di TUI/flag.
- Semua soal/jawaban tersimpan sebagai JSON + ringkasan Markdown.
- Log tidak memuat API key.
- Dapat dijalankan ulang untuk kuis berbeda.

## Kriteria penerimaan
- Menjalankan binary → layar input → isi kode/nama/durasi → Firefox terbuka, tab baru, join sukses.
- Daftar seluruh soal (dan tipe/opsi/gambar) tersimpan sebelum kuis mulai.
- Jawaban dihasilkan untuk semua soal dan tersimpan.
- Setelah host mulai, program menjawab otomatis dengan jeda manusiawi, dan semua soal terjawab < durasi yang diinput.
- `navigator.webdriver === false` di halaman.
- Tidak ada paste, resize, klik kanan, atau blur panjang selama sesi.
- Hasil akhir terlihat di UI dan file sesi.

## Batasan/ketergantungan
- Butuh `firefox` terpasang dan sesi grafis X/Wayland.
- Kunci jawaban **tidak** dikirim ke pemain pada room live normal; harus diselesaikan solver.
- Kode room hanya valid selama room hidup.
- Perubahan DOM/API Wayground mungkin terjadi; selector dipusatkan di satu modul.

## Revisi implementasi (mengikat)
- Jawaban soal disiapkan seluruhnya setelah join dan **sebelum** kuis dimulai, disimpan di `answers.json` dengan label sumber dan confidence.
- Saat kuis berjalan, agen berbasis model **membaca screenshot viewport penuh 1920x1080 (PNG)** plus snapshot DOM, lalu memilih satu aksi; aksi divalidasi dan diverifikasi.
- Jawaban tersimpan tetap menjadi acuan saat bacaan screenshot meragukan (`doubt`), dan hanya untuk soal yang bersangkutan.
- Jeda maksimum 10 detik; panggilan vision berjalan paralel dengan jeda. Bila gambar gagal, aksi dilanjutkan memakai snapshot DOM dan jawaban tersimpan.
- Pengetikan tanpa clipboard: 2 karakter awal lambat, sisanya potongan 10-14 karakter.
- Sesi berakhir bila tidak ada soal baru dan halaman berstatus akhir (ringkasan/survei/papan skor) selama 15 detik, atau agen menyatakan `done`.

## Hasil uji terakhir
- Room 10 soal: 10/10 terjawab dan terverifikasi, total 97 detik dari batas 180 detik, median 9,9 detik/soal, maksimum 16 detik.
- `navigator.webdriver === false`, tanpa paste, tanpa resize setelah join, satu jendela, fullscreen sejak awal.
