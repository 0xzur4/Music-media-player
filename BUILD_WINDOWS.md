# Build di Windows (PC sendiri)

Repo ini bisa dibangun langsung di Windows — tanpa setup cross-compile Linux.

## 1. Install Rust

1. Download `rustup-init.exe` dari https://rustup.rs, jalankan.
2. Pilih install default (stable, target `x86_64-pc-windows-msvc`).
3. Restart terminal, cek: `cargo --version`

## 2. Build .exe portable

```powershell
git clone https://github.com/0xzur4/Music-media-player.git
cd Music-media-player
cargo build --release
```

Hasilnya: `target\release\joni-music.exe` (portable, tinggal dijalankan).

Font CJK untuk judul/lirik Jepang–Korea–China (`assets/fonts/`)
sudah ter-commit di repo, jadi langsung terbawa saat build.

### Naikkan versi rilis

Di `Cargo.toml` → field `version` (sekarang `1.7.0`).

Catatan: repo hanya didistribusikan sebagai exe portable;
tidak ada build MSI.
