# Joni Music 🎵

A desktop music player for Windows, written in Rust.

## Features

- **Offline**: play your downloaded songs (mp3, flac, ogg, wav, m4a, aac, opus)
- **Online**: synced lyrics (LRC) are fetched automatically from the internet
  (lrclib.net) the first time a song plays, then **cached** — subsequent
  plays show lyrics instantly, offline
- **Spotify-style karaoke lyrics**: big white active line with a running green
  bar, smooth auto-scroll, click any line to jump to that part
- **Mini player**: minimizing switches to a small transparent always-on-top
  window with the title + active lyric line; resizable, font scales with
  the window
- **CJK support**: Japanese (hiragana, katakana, kanji), Korean (hangul) and
  Chinese characters render correctly in song titles and lyrics, via bundled
  Noto Sans CJK subset fonts
- Playlist + search, seek bar, volume control, auto-advance to the next song
- Dark Spotify-like theme

## Layout (Spotify clone)

Colors extracted from Spotify production CSS (`#121212`, `#000000`,
`#1ed760`, `#b3b3b3`, …).

- **Left sidebar** (black): logo, Home / Lyrics navigation,
  "Your Library" + search, big green play button +
  # | Title | Album | Duration table (playing song: green text +
  animated equalizer, click a row to play)
- **Main content** (`#121212`): top 80% shows the Lyrics view (karaoke with
  big white active line, green bar, click a line to jump) or the Playlist
  view (header + big green play button + roomy table);
  bottom 20% is a black control strip: centered 🔀 ⏮ ▶/⏸ ⏭ 🔁,
  progress + time, volume button (click to pop up a vertical slider),
  🧲 (mini player)
- 🔀 = shuffle, 🔁 = repeat (off / all / one); auto-advance
  respects shuffle & repeat

## Build

Requires stable Rust + the Windows target:

```sh
rustup target add x86_64-pc-windows-gnu
# linker: x86_64-w64-mingw32-gcc (mingw-w64 package on Linux, or MSVC on Windows)
cargo build --release --target x86_64-pc-windows-gnu
```

Output: `target/x86_64-pc-windows-gnu/release/joni-music.exe`

## MSI installer (optional, via Linux)

```sh
# requires: wixl (a separate package from msitools)
cp target/x86_64-pc-windows-gnu/release/joni-music.exe wix/
cd wix && wixl -o JoniMusic-<version>.msi product.wxs
```

## Project structure

- `src/main.rs` — UI (eframe/egui): playlist, controls, lyrics, mini player
- `src/audio.rs` — playback (rodio)
- `src/library.rs` — folder scanning + metadata (lofty)
- `src/lyrics.rs` — LRC parsing, lyric fetching & caching (lrclib.net)
- `assets/fonts/` — bundled CJK fonts (Noto Sans CJK subsets for
  Japanese/Chinese + Korean hangul)
- `wix/product.wxs` — MSI installer definition
