# Joni Music 🎵

Music media player desktop untuk Windows, ditulis dengan Rust.

## Fitur

- **Offline**: putar lagu yang sudah didownload (mp3, flac, ogg, wav, m4a, aac, opus)
- **Online**: lirik tersinkron (LRC) otomatis diambil dari internet (lrclib.net)
  saat lagu pertama kali diputar, lalu **disimpan** — pemutaran berikutnya
  lirik langsung tampil offline
- **Lirik karaoke ala Spotify**: baris aktif besar & putih dengan bar hijau
  berjalan, auto-scroll halus, klik baris lirik untuk lompat ke bagian itu
- **Mini player**: saat minimize, berubah jadi jendela kecil transparan
  always-on-top berisi judul + baris lirik aktif; bisa di-resize dan font
  mengikuti ukuran jendela
- Playlist + pencarian, seek bar, kontrol volume, auto-lanjut ke lagu berikut
- Tema gelap ala Spotify

## Tata letak (clone Spotify)

Warna diekstrak dari CSS produksi Spotify (`#121212`, `#000000`,
`#1ed760`, `#b3b3b3`, …) via skill REA `reverse-engineer-anything`.

- **Sidebar kiri** (hitam): logo, navigasi Home / Lirik,
  "Your Library" + pencarian, tombol play hijau besar +
  tabel # | Judul | Album | Durasi (lagu diputar: teks hijau +
  equalizer animasi, klik baris = putar)
- **Konten utama** (`#121212`): 80% tampilan Lirik (karaoke baris
  aktif putih besar, bar hijau, klik baris = lompat) atau tampilan
  Playlist (header + tombol play hijau + tabel lega);
  20% di bawahnya strip kontrol hitam: 🔀 ⏮ ▶/⏸ ⏭ 🔁 terpusat,
  progress + waktu, volume, 🎤 (lirik), 🧲 (mini player)
- 🔀 = acak, 🔁 = ulangi (mati / semua / satu); auto-lanjut
  menghormati shuffle & repeat

## Build

Butuh Rust stable + target Windows:

```sh
rustup target add x86_64-pc-windows-gnu
# linker: x86_64-w64-mingw32-gcc (paket mingw-w64 di Linux, atau MSVC di Windows)
cargo build --release --target x86_64-pc-windows-gnu
```

Hasil: `target/x86_64-pc-windows-gnu/release/joni-music.exe`

## Installer MSI (opsional, via Linux)

```sh
# butuh: wixl (paket terpisah dari msitools)
cp target/x86_64-pc-windows-gnu/release/joni-music.exe wix/
cd wix && wixl -o JoniMusic-<versi>.msi product.wxs
```

## Struktur

- `src/main.rs` — UI (eframe/egui): playlist, kontrol, lirik, mini player
- `src/audio.rs` — playback (rodio)
- `src/library.rs` — scan folder + baca metadata (lofty)
- `src/lyrics.rs` — parse LRC, fetch & cache lirik (lrclib.net)
- `wix/product.wxs` — definisi installer MSI
