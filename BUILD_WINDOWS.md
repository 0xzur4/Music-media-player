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

## 3. Build installer MSI (opsional)

Pakai WiX Toolset v3 (terakhir: 3.14.x):

1. Download installer WiX v3 dari
   https://github.com/wixtoolset/wix3/releases
   (file `wix314*.exe`), jalankan, lalu **restart terminal**
   supaya `candle.exe` dan `light.exe` masuk PATH.
2. Copy hasil build ke folder wix:

```powershell
copy target\release\joni-music.exe wix\joni-music.exe
cd wix
candle.exe product.wxs
light.exe -sval product.wixobj
```

Hasilnya: `wix\JoniMusic-<versi>.msi`
(per-user install ke `%LocalAppData%\Joni Music`, tanpa admin).

### Naikkan versi rilis

Dua tempat harus diganti agar sinkron (sekarang `1.6.7`):

1. `Cargo.toml` → field `version`
2. `wix/product.wxs` → atribut `Version="1.6.7"` pada `<Product ...>`

`Id="*"` pada Product berarti GUID baru dibuat otomatis tiap build,
jadi MSI hasil rebuild tidak bentrok dengan versi lama.
