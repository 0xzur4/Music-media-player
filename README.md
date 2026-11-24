# Joni Music 🎵

A desktop music player for Windows, written in Rust.

## Download

Get the latest Windows build from the
[Releases page](https://github.com/0xzur4/Music-media-player/releases):

- `joni-music-<version>.exe` — portable, just run it

## Features

- **Offline**: play your downloaded songs (mp3, flac, ogg, wav, m4a, aac, opus)
- **Online & local synced lyrics**: LRC lyrics are loaded from a local `.lrc` sidecar file, cache, or fetched automatically from `lrclib.net` and cached for offline use
- **Spotify-style karaoke lyrics**: dynamic cover-tinted gradient background, big bold white active line, smooth auto-scroll, click any line to jump, plus one-click online refresh
- **Modern Spotify-like desktop UI**: full-width bottom player bar (cover art, transport, thin seek bar, inline volume & mute, audio output picker, EQ, lyrics toggle, mini player), crisp custom vector icons, resizable sidebar with colored playlist tiles, and virtualized track table
- **Sorting & scoped search**: search by All / Title / Artist / Album and sort by Default, Title, Artist, Album, or Duration (ascending/descending)
- **5-band Equalizer & playlists**: live biquad EQ with presets (Flat, Rock, Pop, Jazz, Klasik, Bass), custom playlists, and playback queue scoped to the active playlist
- **Persistent settings**: remembers volume, mute, EQ preset/gains, shuffle, repeat, sort, search scope, sidebar width, and resumes the last played track & position
- **Keyboard shortcuts**: `Space`/`K` (play/pause), `←`/`→` (seek ±5s, `Shift` ±15s), `Ctrl+←`/`→` or `P`/`N` (prev/next), `↑`/`↓` (volume), `M` (mute), `S` (shuffle), `R` (repeat), `L` (lyrics), `E` (EQ), `Ctrl+F` (search), `F1` (shortcuts help)
- **Mini player**: borderless always-on-top compact window with cover art, active lyric line, seek bar, vector controls, and manual resize handle
- **CJK support**: Japanese, Korean, and Chinese characters render via bundled Noto Sans CJK subset fonts
- **Audio output selection**: switch headset / earphones / speakers live from the bottom bar without losing track position

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

## License

MIT — see [LICENSE](LICENSE).
