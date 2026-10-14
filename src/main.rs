#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod library;
mod lyrics;
mod playlist;

use audio::Player;
use audio::{output_device_names, DEFAULT_DEVICE_LABEL};
use eframe::egui;
use library::Track;
use lyrics::{get_lyrics, Lyrics};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

#[derive(Clone)]
enum LyricState {
    None,
    Loading,
    Loaded { lyrics: Lyrics, from_cache: bool },
    NotFound,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    Playlist,
    Lyrics,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RepeatMode {
    Off,
    All,
    One,
}

// ---- Palet warna Spotify (diekstrak dari CSS produksi open.spotify.com) ----
const SPOT_BG: egui::Color32 = egui::Color32::from_rgb(0x12, 0x12, 0x12);
const SPOT_BLACK: egui::Color32 = egui::Color32::BLACK;
const SPOT_CARD: egui::Color32 = egui::Color32::from_rgb(0x18, 0x18, 0x18);
const SPOT_HOVER: egui::Color32 = egui::Color32::from_rgb(0x28, 0x28, 0x28);
const SPOT_GREEN: egui::Color32 = egui::Color32::from_rgb(0x1e, 0xd7, 0x60);
const SPOT_WHITE: egui::Color32 = egui::Color32::WHITE;
const SPOT_GRAY: egui::Color32 = egui::Color32::from_rgb(0xb3, 0xb3, 0xb3);
const SPOT_DIM: egui::Color32 = egui::Color32::from_rgb(0xa7, 0xa7, 0xa7);
const SPOT_GREEN_HOVER: egui::Color32 = egui::Color32::from_rgb(0x1f, 0xdf, 0x64);

/// Ikon equalizer animasi untuk baris lagu yang sedang diputar.
fn eq_bars(ui: &mut egui::Ui, t: f64) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        for (i, base) in [12.0f64, 18.0, 8.0].iter().enumerate() {
            let h = base * (0.55 + 0.45 * (t * 4.0 + i as f64 * 1.9).sin().abs());
            let (r, _) = ui.allocate_exact_size(egui::vec2(3.0, 18.0), egui::Sense::hover());
            let bar = egui::Rect::from_min_size(
                egui::pos2(r.center().x - 1.5, r.max.y - h as f32),
                egui::vec2(3.0, h as f32),
            );
            ui.painter().rect_filled(bar, 1.0, SPOT_GREEN);
        }
    });
}

struct MusicApp {
    player: Player,
    tracks: Vec<Track>,
    current: Option<usize>,
    music_dir: Option<PathBuf>,
    lyric_state: LyricState,
    lyric_rx: Receiver<(usize, Option<Lyrics>, bool)>,
    lyric_tx: Sender<(usize, Option<Lyrics>, bool)>,
    scan_rx: Receiver<Vec<Track>>,
    scan_tx: Sender<Vec<Track>>,
    search: String,
    volume: f32,
    status_msg: String,
    lyric_scroll: f32,
    lyric_viewport_h: f32,
    /// Baris lirik aktif pada frame sebelumnya (untuk snap scroll lagu cepat).
    last_active: Option<usize>,
    /// Perangkat output audio pilihan pengguna; None = default sistem.
    audio_device: Option<String>,
    device_names: Vec<String>,
    mini_mode: bool,
    view: View,
    shuffle: bool,
    repeat: RepeatMode,
    shuffle_order: Vec<usize>,
    shuffle_pos: usize,
    /// Lebar sidebar kiri (px). Diubah via drag handle kustom di tepi kanan
    /// sidebar; 240..560, default 360.
    sidebar_width: f32,
    /// Tampilkan popup equalizer.
    show_eq: bool,
    /// Nama preset EQ aktif ("Kustom" bila diubah manual).
    eq_preset: String,
    /// Texture cover art lagu aktif + path pemiliknya.
    cover_texture: Option<egui::TextureHandle>,
    cover_for: Option<PathBuf>,
    /// Daftar playlist pengguna (persist di playlists.json).
    playlists: Vec<playlist::Playlist>,
    /// Index playlist aktif; None = tampilkan semua lagu.
    active_playlist: Option<usize>,
    /// Buffer nama playlist baru + toggle tampil input.
    new_playlist_name: String,
    show_playlist_input: bool,
}

impl MusicApp {
    fn new() -> Self {
        let (lyric_tx, lyric_rx) = mpsc::channel();
        let (scan_tx, scan_rx) = mpsc::channel();
        let player = Player::new().expect("audio output tidak tersedia");

        let mut app = Self {
            player,
            tracks: Vec::new(),
            current: None,
            music_dir: None,
            lyric_state: LyricState::None,
            lyric_rx,
            lyric_tx,
            scan_rx,
            scan_tx,
            search: String::new(),
            volume: 80.0,
            status_msg: String::from("Pilih folder musik untuk mulai."),
            lyric_scroll: 0.0,
            lyric_viewport_h: 500.0,
            last_active: None,
            audio_device: None,
            device_names: output_device_names(),
            mini_mode: false,
            view: View::Lyrics,
            shuffle: false,
            repeat: RepeatMode::Off,
            shuffle_order: Vec::new(),
            shuffle_pos: 0,
            sidebar_width: 360.0,
            show_eq: false,
            eq_preset: String::from("Flat"),
            cover_texture: None,
            cover_for: None,
            playlists: Vec::new(),
            active_playlist: None,
            new_playlist_name: String::new(),
            show_playlist_input: false,
        };
        app.player.set_volume(0.8);

        let (dir, audio_device) = load_config();
        if let Some(dev) = audio_device {
            match app.player.set_output_device(Some(&dev)) {
                Ok(()) => {
                    app.audio_device = Some(dev);
                }
                Err(e) => {
                    app.status_msg =
                        format!("Perangkat audio tersimpan tidak tersedia ({e}); pakai default.");
                }
            }
        }
        if let Some(dir) = dir {
            app.set_music_dir(dir);
        }
        app.playlists = playlist::load();
        app
    }

    fn set_music_dir(&mut self, dir: PathBuf) {
        self.music_dir = Some(dir.clone());
        save_config(&self.music_dir, &self.audio_device);
        self.tracks.clear();
        self.current = None;
        self.player.stop();
        self.lyric_state = LyricState::None;
        self.status_msg = String::from("Memindai folder musik…");
        let tx = self.scan_tx.clone();
        std::thread::spawn(move || {
            let tracks = library::scan(&dir);
            let _ = tx.send(tracks);
        });
    }

    /// Ganti perangkat output audio dan simpan preferensinya.
    fn set_audio_device(&mut self, device: Option<String>) {
        let label = device.as_deref();
        match self.player.set_output_device(label) {
            Ok(()) => {
                self.audio_device = device;
                save_config(&self.music_dir, &self.audio_device);
                let shown = self
                    .audio_device
                    .as_deref()
                    .unwrap_or(DEFAULT_DEVICE_LABEL);
                self.status_msg = format!("Output audio: {shown}");
            }
            Err(e) => {
                self.status_msg = format!("Gagal ganti perangkat audio: {e}");
            }
        }
    }

    fn play_index(&mut self, idx: usize) {
        if idx >= self.tracks.len() {
            return;
        }
        let track = self.tracks[idx].clone();
        match self.player.play(&track) {
            Ok(()) => {
                self.current = Some(idx);
                if self.shuffle {
                    self.ensure_shuffle_order();
                }
                self.lyric_scroll = 0.0;
                self.last_active = None;
                self.status_msg = format!("Memutar: {}", track.display());
                // Ambil lirik di background: cache dulu, kalau belum ada unduh online.
                self.lyric_state = LyricState::Loading;
                let tx = self.lyric_tx.clone();
                std::thread::spawn(move || {
                    let (lyrics, from_cache) = get_lyrics(&track);
                    let _ = tx.send((idx, lyrics, from_cache));
                });
            }
            Err(e) => {
                self.status_msg = format!("Gagal memutar: {e}");
            }
        }
    }

    /// Putar/jeda.
    fn toggle_play(&mut self) {
        if self.player.has_track() {
            self.player.toggle();
        } else if !self.tracks.is_empty() {
            self.play_index(0);
        }
    }

    fn next(&mut self) {
        if self.tracks.is_empty() {
            return;
        }
        if self.shuffle {
            self.ensure_shuffle_order();
            if self.shuffle_order.is_empty() {
                return;
            }
            self.shuffle_pos = (self.shuffle_pos + 1) % self.shuffle_order.len();
            let idx = self.shuffle_order[self.shuffle_pos];
            self.play_index(idx);
        } else {
            let n = match self.current {
                Some(i) => (i + 1) % self.tracks.len(),
                None => 0,
            };
            self.play_index(n);
        }
    }

    fn prev(&mut self) {
        if self.tracks.is_empty() {
            return;
        }
        if self.shuffle {
            self.ensure_shuffle_order();
            if self.shuffle_order.is_empty() {
                return;
            }
            self.shuffle_pos = self
                .shuffle_pos
                .checked_sub(1)
                .unwrap_or(self.shuffle_order.len() - 1);
            let idx = self.shuffle_order[self.shuffle_pos];
            self.play_index(idx);
        } else {
            let n = match self.current {
                Some(0) | None => self.tracks.len() - 1,
                Some(i) => i - 1,
            };
            self.play_index(n);
        }
    }

    /// Dipanggil saat lagu selesai: hormati mode shuffle & repeat.
    fn advance(&mut self) {
        if self.tracks.is_empty() {
            return;
        }
        if self.repeat == RepeatMode::One {
            if let Some(i) = self.current {
                self.play_index(i);
            }
            return;
        }
        if self.shuffle {
            self.ensure_shuffle_order();
            if self.shuffle_order.is_empty() {
                return;
            }
            if self.shuffle_pos + 1 < self.shuffle_order.len() {
                self.shuffle_pos += 1;
                let idx = self.shuffle_order[self.shuffle_pos];
                self.play_index(idx);
            } else if self.repeat == RepeatMode::All {
                self.new_shuffle_order();
                self.shuffle_pos = 0;
                let idx = self.shuffle_order[0];
                self.play_index(idx);
            }
            // Repeat mati + sudah di akhir: berhenti.
        } else {
            match self.current {
                Some(i) if i + 1 < self.tracks.len() => self.play_index(i + 1),
                Some(_) if self.repeat == RepeatMode::All => self.play_index(0),
                None => self.play_index(0),
                _ => {}
            }
        }
    }

    /// Pastikan urutan acak sinkron dengan daftar lagu.
    fn ensure_shuffle_order(&mut self) {
        if self.shuffle_order.len() != self.tracks.len() {
            self.new_shuffle_order();
        }
        if let Some(cur) = self.current {
            if let Some(p) = self.shuffle_order.iter().position(|&i| i == cur) {
                self.shuffle_pos = p;
            }
        }
        if !self.shuffle_order.is_empty() {
            self.shuffle_pos = self.shuffle_pos.min(self.shuffle_order.len() - 1);
        }
    }

    /// Urutan acak baru (Fisher-Yates + xorshift, tanpa crate tambahan).
    fn new_shuffle_order(&mut self) {
        let mut order: Vec<usize> = (0..self.tracks.len()).collect();
        let mut s: u64 = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E3779B97F4A7C15)
            .wrapping_add(order.len() as u64 + 1);
        if s == 0 {
            s = 0x9E3779B97F4A7C15;
        }
        for i in (1..order.len()).rev() {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            let j = (s % (i as u64 + 1)) as usize;
            order.swap(i, j);
        }
        self.shuffle_order = order;
        self.shuffle_pos = 0;
    }

    /// Pindah tampilan utama (dengan riwayat untuk ‹ ›).
    /// Masuk/keluar mode mini player: jendela kecil transparan always-on-top.
    fn set_mini(&mut self, ctx: &egui::Context, mini: bool) {
        self.mini_mode = mini;
        if mini {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Transparent(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                egui::WindowLevel::AlwaysOnTop,
            ));
            ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(egui::vec2(
                220.0, 110.0,
            )));
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
                400.0, 215.0,
            )));
        } else {
            ctx.send_viewport_cmd(egui::ViewportCommand::Transparent(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                egui::WindowLevel::Normal,
            ));
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
                1100.0, 700.0,
            )));
        }
    }

    /// Tampilan mini player: judul + kontrol, font mengikuti ukuran jendela.
    fn show_mini(&mut self, ui: &mut egui::Ui) {
        // Latar semi-transparan (viewport sudah transparent di mode mini)
        let bg = ui.max_rect();
        ui.painter()
            .rect_filled(bg, 14.0, egui::Color32::from_rgba_unmultiplied(12, 12, 12, 175));

        let w = ui.available_width();
        let title_size = (w / 15.0).clamp(13.0, 34.0);
        let artist_size = (w / 24.0).clamp(10.0, 18.0);
        let pos = self.player.position();

        ui.vertical_centered(|ui| {
            ui.add_space(6.0);
            match self.current.and_then(|i| self.tracks.get(i)) {
                Some(t) => {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&t.title)
                                .size(title_size)
                                .strong()
                                .color(egui::Color32::WHITE),
                        )
                        .truncate(),
                    );
                    if !t.artist.is_empty() {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(&t.artist)
                                    .size(artist_size)
                                    .color(egui::Color32::from_gray(160)),
                            )
                            .truncate(),
                        );
                    }
                }
                None => {
                    ui.label(
                        egui::RichText::new("Joni Music")
                            .size(title_size)
                            .color(egui::Color32::WHITE),
                    );
                }
            }
            // Baris lirik yang sedang dinyanyikan (karaoke mini)
            if let LyricState::Loaded { lyrics, .. } = &self.lyric_state {
                if lyrics.synced {
                    if let Some(i) = lyrics.active_index(pos) {
                        if let Some(line) = lyrics.lines.get(i) {
                            let lsize = (w / 20.0).clamp(11.0, 24.0);
                            let ltext = if line.text.is_empty() {
                                "♪".to_string()
                            } else {
                                line.text.clone()
                            };
                            ui.add_space(2.0);
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(ltext)
                                        .size(lsize)
                                        .strong()
                                        .color(egui::Color32::from_rgb(120, 220, 130)),
                                )
                                .truncate(),
                            );
                        }
                    }
                }
            }
            ui.add_space(4.0);
            // Progress tipis
            let dur = self.player.duration().max(0.01);
            let p = (self.player.position() / dur).clamp(0.0, 1.0) as f32;
            let bar_w = ui.available_width() - 20.0;
            let (br, _) =
                ui.allocate_exact_size(egui::vec2(bar_w, 3.0), egui::Sense::hover());
            ui.painter().rect_filled(
                br,
                1.5,
                egui::Color32::from_rgba_unmultiplied(255, 255, 255, 40),
            );
            ui.painter().rect_filled(
                egui::Rect::from_min_size(br.min, egui::vec2(br.width() * p, 3.0)),
                1.5,
                egui::Color32::from_rgb(29, 185, 84),
            );
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.button("⏮").clicked() {
                    self.prev();
                }
                let play_label = if self.player.is_playing() { "⏸" } else { "▶" };
                if ui.button(egui::RichText::new(play_label).size(18.0)).clicked() {
                    if self.player.has_track() {
                        self.player.toggle();
                    } else if !self.tracks.is_empty() {
                        self.play_index(0);
                    }
                }
                if ui.button("⏭").clicked() {
                    self.next();
                }
                ui.separator();
                if ui.button("⛶").on_hover_text("Kembali ke tampilan penuh").clicked() {
                    let ctx = ui.ctx().clone();
                    self.set_mini(&ctx, false);
                }
            });
        });
    }

    /// Tampilan daftar lagu ala Spotify: header + tabel # | Judul | Album | Durasi.
    /// Tombol play lingkaran hijau ala Spotify.
    fn green_play_button(&mut self, ui: &mut egui::Ui, d: f32) {
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(d, d), egui::Sense::click());
        let c = rect.center();
        let col = if resp.hovered() {
            SPOT_GREEN_HOVER
        } else {
            SPOT_GREEN
        };
        ui.painter().circle_filled(c, d / 2.0, col);
        let glyph = if self.player.is_playing() {
            "⏸"
        } else {
            "▶"
        };
        ui.painter().text(
            c,
            egui::Align2::CENTER_CENTER,
            glyph,
            egui::FontId::proportional(d * 0.42),
            egui::Color32::BLACK,
        );
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if resp.clicked() {
            self.toggle_play();
        }
    }

    /// Tabel lagu # | Judul | Album | Durasi. `compact` = versi rapat untuk sidebar.
    /// Baris aktif: teks hijau + equalizer animasi. Klik baris = putar.
    fn show_track_table(&mut self, ui: &mut egui::Ui, compact: bool) {
        if self.tracks.is_empty() {
            ui.label(
                egui::RichText::new("Pilih folder musik untuk mulai.")
                    .size(13.0)
                    .color(SPOT_DIM),
            );
            return;
        }
        let row_h = if compact { 40.0 } else { 56.0 };
        let num_w = 32.0;
        let album_w = if compact { 92.0 } else { 200.0 };
        let dur_w = 48.0;
        let title_size = if compact { 13.0 } else { 15.0 };
        // Header
        ui.horizontal(|ui| {
            ui.add_sized(
                egui::vec2(num_w, 16.0),
                egui::Label::new(egui::RichText::new("#").size(11.0).color(SPOT_DIM)),
            );
            let rest = ui.available_width();
            let tw = (rest - album_w - dur_w - 24.0).max(60.0);
            ui.add_sized(
                egui::vec2(tw, 16.0),
                egui::Label::new(egui::RichText::new("Judul").size(11.0).color(SPOT_DIM)),
            );
            ui.add_sized(
                egui::vec2(album_w, 16.0),
                egui::Label::new(egui::RichText::new("Album").size(11.0).color(SPOT_DIM)),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new("⏱").size(11.0).color(SPOT_DIM));
            });
        });
        ui.separator();
        let t_now = ui.ctx().input(|i| i.time);
        let playing = self.player.is_playing();
        let mut clicked_idx: Option<usize> = None;
        for (num, idx) in self.filtered_indices().iter().enumerate() {
            let idx = *idx;
            let is_current = self.current == Some(idx);
            let (title, artist, album, dur_s) = {
                let t = &self.tracks[idx];
                (
                    t.title.clone(),
                    t.artist.clone(),
                    t.album.clone(),
                    t.duration_secs,
                )
            };
            let (rect, resp) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), row_h),
                egui::Sense::click(),
            );
            if resp.hovered() {
                ui.painter().rect_filled(rect, 4.0, SPOT_HOVER);
            }
            if resp.clicked() {
                clicked_idx = Some(idx);
            }
            let mut cui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(rect)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            // Kolom #
            cui.allocate_ui_with_layout(
                egui::vec2(num_w, row_h),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    if is_current {
                        if playing {
                            eq_bars(ui, t_now);
                        } else {
                            ui.label(
                                egui::RichText::new("▶").size(12.0).color(SPOT_GREEN),
                            );
                        }
                    } else {
                        ui.label(
                            egui::RichText::new(format!("{}", num + 1))
                                .size(13.0)
                                .color(SPOT_GRAY),
                        );
                    }
                },
            );
            // Kolom Judul + artis
            let rest = cui.available_width();
            let tw = (rest - album_w - dur_w - 24.0).max(60.0);
            cui.allocate_ui_with_layout(
                egui::vec2(tw, row_h),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    let tcol = if is_current { SPOT_GREEN } else { SPOT_WHITE };
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&title)
                                .size(title_size)
                                .strong()
                                .color(tcol),
                        )
                        .truncate(),
                    );
                    let a = if artist.is_empty() {
                        "Unknown Artist".to_string()
                    } else {
                        artist
                    };
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(a).size(12.0).color(SPOT_GRAY),
                        )
                        .truncate(),
                    );
                },
            );
            cui.add_space(8.0);
            cui.add_sized(
                egui::vec2(album_w, row_h),
                egui::Label::new(egui::RichText::new(album).size(12.0).color(SPOT_GRAY))
                    .truncate(),
            );
            cui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(fmt_time(dur_s)).size(12.0).color(SPOT_GRAY),
                );
            });
            // Klik kanan: tambah ke playlist / hapus dari playlist aktif.
            let mut add_to: Option<usize> = None;
            let mut remove_from_active = false;
            resp.context_menu(|ui| {
                if !self.playlists.is_empty() {
                    ui.label(
                        egui::RichText::new("Tambah ke playlist:")
                            .size(12.0)
                            .color(SPOT_GRAY),
                    );
                    for (pi, pl) in self.playlists.iter().enumerate() {
                        if ui.button(format!("🎵 {}", pl.name)).clicked() {
                            add_to = Some(pi);
                        }
                    }
                    ui.separator();
                }
                if self.active_playlist.is_some()
                    && ui.button("🗑 Hapus dari playlist ini").clicked()
                {
                    remove_from_active = true;
                }
            });
            if let Some(pi) = add_to {
                let path = self.tracks[idx].path.clone();
                if self.playlists[pi].add(&path) {
                    playlist::save(&self.playlists);
                }
            }
            if remove_from_active {
                if let Some(pi) = self.active_playlist {
                    let path = self.tracks[idx].path.clone();
                    self.playlists[pi].remove(&path);
                    playlist::save(&self.playlists);
                }
            }
        }
        if let Some(i) = clicked_idx {
            self.play_index(i);
        }
    }

    /// Halaman playlist untuk konten utama (view Home): header lega + tabel.
    fn show_playlist_page(&mut self, ui: &mut egui::Ui) {
        if self.tracks.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(
                    egui::RichText::new("Pilih folder musik untuk mulai.")
                        .size(18.0)
                        .color(SPOT_GRAY),
                );
            });
            return;
        }
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.add_space(16.0);
            self.green_play_button(ui, 56.0);
            ui.add_space(12.0);
            ui.vertical(|ui| {
                ui.label(egui::RichText::new("Playlist").size(13.0).color(SPOT_GRAY));
                ui.label(
                    egui::RichText::new("Koleksi Lagu")
                        .size(34.0)
                        .strong()
                        .color(SPOT_WHITE),
                );
                ui.label(
                    egui::RichText::new(format!("{} lagu", self.tracks.len()))
                        .size(13.0)
                        .color(SPOT_GRAY),
                );
            });
        });
        ui.add_space(10.0);
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.add_space(4.0);
            self.show_track_table(ui, false);
        });
    }

    /// Strip kontrol 20% di bawah konten utama: transport terpusat + progress + volume.
    fn show_control_strip(&mut self, ui: &mut egui::Ui) {
        // Grid 2 kolom: kiri (kontrol terpusat + progress) mengambil seluruh sisa ruang,
        // kanan (lirik, volume, mini) lebar tetap, rata kanan, sejajar tengah vertikal.
        let ch = ui.available_height();
        let block_h = 48.0 + 14.0 + 22.0; // kontrol + jeda + progress
        ui.add_space(((ch - block_h) / 2.0).max(16.0));
        let avail_w = ui.available_width();
        let col_gap = 24.0;
        // Kolom kanan lebar tetap agar isinya tidak kepotong;
        // kolom kiri (progress) memanjang mengikuti lebar jendela.
        let right_w = 300.0f32.min(avail_w * 0.45).max(160.0);
        let left_w = (avail_w - right_w - col_gap).max(180.0);
        ui.horizontal(|ui| {
            // ---- Kolom kiri: kontrol terpusat, progress di bawahnya ----
            ui.allocate_ui_with_layout(
                egui::vec2(left_w, block_h),
                egui::Layout::top_down(egui::Align::Center),
                |ui| {
                    // Baris 1: kontrol
                    ui.allocate_ui_with_layout(
                        egui::vec2(312.0, 48.0),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.spacing_mut().item_spacing.x = 28.0;
                            let dim = SPOT_DIM;
                            let sh_col = if self.shuffle { SPOT_GREEN } else { dim };
                            if ui
                                .add_sized(
                                    egui::vec2(38.0, 38.0),
                                    egui::Button::new(
                                        egui::RichText::new("🔀").size(17.0).color(sh_col),
                                    )
                                    .frame(false),
                                )
                                .on_hover_text("Acak")
                                .clicked()
                            {
                                self.shuffle = !self.shuffle;
                                if self.shuffle {
                                    self.ensure_shuffle_order();
                                }
                            }
                            if ui
                                .add_sized(
                                    egui::vec2(38.0, 38.0),
                                    egui::Button::new(
                                        egui::RichText::new("\u{23EE}").size(17.0).color(SPOT_GRAY),
                                    )
                                    .frame(false),
                                )
                                .on_hover_text("Sebelumnya")
                                .clicked()
                            {
                                self.prev();
                            }
                            let play_glyph = if self.player.is_playing() {
                                "\u{23F8}"
                            } else {
                                "\u{25B6}"
                            };
                            if ui
                                .add_sized(
                                    egui::vec2(48.0, 48.0),
                                    egui::Button::new(
                                        egui::RichText::new(play_glyph)
                                            .size(19.0)
                                            .color(egui::Color32::BLACK),
                                    )
                                    .fill(SPOT_WHITE)
                                    .corner_radius(egui::CornerRadius::same(24)),
                                )
                                .on_hover_text("Putar / Jeda")
                                .clicked()
                            {
                                self.toggle_play();
                            }
                            if ui
                                .add_sized(
                                    egui::vec2(38.0, 38.0),
                                    egui::Button::new(
                                        egui::RichText::new("\u{23ED}").size(17.0).color(SPOT_GRAY),
                                    )
                                    .frame(false),
                                )
                                .on_hover_text("Berikutnya")
                                .clicked()
                            {
                                self.next();
                            }
                            let (rep_glyph, rep_col) = match self.repeat {
                                RepeatMode::Off => ("🔁", dim),
                                RepeatMode::All => ("🔁", SPOT_GREEN),
                                RepeatMode::One => ("🔂", SPOT_GREEN),
                            };
                            if ui
                                .add_sized(
                                    egui::vec2(38.0, 38.0),
                                    egui::Button::new(
                                        egui::RichText::new(rep_glyph).size(17.0).color(rep_col),
                                    )
                                    .frame(false),
                                )
                                .on_hover_text("Ulangi: mati / semua / satu")
                                .clicked()
                            {
                                self.repeat = match self.repeat {
                                    RepeatMode::Off => RepeatMode::All,
                                    RepeatMode::All => RepeatMode::One,
                                    RepeatMode::One => RepeatMode::Off,
                                };
                            }
                        },
                    );
                    ui.add_space(14.0);
                    // Baris 2: progress memenuhi lebar kolom kiri
                    let dur = self.player.duration().max(0.01);
                    let pos = self.player.position();
                    ui.allocate_ui_with_layout(
                        egui::vec2(left_w, 22.0),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.spacing_mut().item_spacing.x = 14.0;
                            ui.add_sized(
                                egui::vec2(44.0, 0.0),
                                egui::Label::new(
                                    egui::RichText::new(fmt_time(pos)).size(11.0).color(SPOT_GRAY),
                                ),
                            );
                            // Slider egui menentukan lebar rel dari spacing().slider_width,
                            // bukan dari add_sized — set langsung agar rel ikut memanjang.
                            let sw = (left_w - 88.0 - 28.0).max(60.0);
                            ui.spacing_mut().slider_width = sw;
                            let mut p = pos;
                            if ui
                                .add(
                                    egui::Slider::new(&mut p, 0.0..=dur)
                                        .show_value(false)
                                        .trailing_fill(true),
                                )
                                .drag_stopped()
                            {
                                self.player.seek(p);
                            }
                            ui.add_sized(
                                egui::vec2(44.0, 0.0),
                                egui::Label::new(
                                    egui::RichText::new(fmt_time(dur)).size(11.0).color(SPOT_GRAY),
                                ),
                            );
                        },
                    );
                },
            );
            ui.add_space(col_gap);
            // ---- Kolom kanan: rata kanan, tengah vertikal ----
            ui.allocate_ui_with_layout(
                egui::vec2(right_w, block_h),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    ui.add_space(12.0); // padding kanan sesuai mockup
                    ui.spacing_mut().item_spacing.x = 8.0;
                    // Tombol equalizer: popup 5 slider vertikal + preset.
                    let eq_col = if self.eq_preset == "Flat" {
                        SPOT_GRAY
                    } else {
                        SPOT_GREEN
                    };
                    if ui
                        .add_sized(
                            egui::vec2(34.0, 28.0),
                            egui::Button::new(
                                egui::RichText::new("🎚").size(16.0).color(eq_col),
                            )
                            .frame(false),
                        )
                        .on_hover_text("Equalizer")
                        .clicked()
                    {
                        self.show_eq = !self.show_eq;
                    }
                    if ui
                        .add_sized(
                            egui::vec2(34.0, 28.0),
                            egui::Button::new(
                                egui::RichText::new("🧲").size(16.0).color(SPOT_GRAY),
                            )
                            .frame(false),
                        )
                        .on_hover_text("Mini player")
                        .clicked()
                    {
                        let ctx = ui.ctx().clone();
                        self.set_mini(&ctx, true);
                    }
                    ui.spacing_mut().item_spacing.x = 14.0;
                    // Tombol volume: klik untuk memunculkan slider vertikal di atasnya.
                    let vol_resp = ui
                        .add_sized(
                            egui::vec2(34.0, 28.0),
                            egui::Button::new(
                                egui::RichText::new("🔊").size(16.0).color(SPOT_GRAY),
                            )
                            .frame(false),
                        )
                        .on_hover_text("Volume");
                    let mut vol = self.volume;
                    let popup = egui::Popup::from_toggle_button_response(&vol_resp)
                        .align(egui::RectAlign::TOP_END)
                        .gap(8.0)
                        .show(|ui| {
                            // Slider vertikal: slider_width menjadi tinggi rel.
                            ui.spacing_mut().slider_width = 110.0;
                            ui.add(
                                egui::Slider::new(&mut vol, 0.0..=100.0)
                                    .vertical()
                                    .show_value(false),
                            )
                        });
                    if popup.is_some_and(|p| p.inner.changed()) {
                        self.volume = vol;
                        self.player.set_volume(vol / 100.0);
                    }
                },
            );
        });
    }

    /// Window equalizer: 5 slider vertikal + preset.
    fn show_eq_window(&mut self, ctx: &egui::Context) {
        if !self.show_eq {
            return;
        }
        let mut open = true;
        egui::Window::new("🎚 Equalizer")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    ui.spacing_mut().slider_width = 90.0;
                    for band in 0..5 {
                        ui.vertical(|ui| {
                            ui.label(
                                egui::RichText::new(crate::audio::EQ_NAMES[band])
                                    .size(11.0)
                                    .color(SPOT_GRAY),
                            );
                            let mut v = self
                                .player
                                .eq_gains()
                                .read()
                                .map(|g| g[band])
                                .unwrap_or(0.0);
                            if ui
                                .add(
                                    egui::Slider::new(&mut v, -12.0..=12.0)
                                        .vertical()
                                        .show_value(false),
                                )
                                .changed()
                            {
                                self.player.set_eq_gain(band, v);
                                self.eq_preset = String::from("Kustom");
                            }
                            ui.label(
                                egui::RichText::new(format!("{:+.0}", v))
                                    .size(10.0)
                                    .color(SPOT_DIM),
                            );
                        });
                    }
                });
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    for (name, gains) in crate::audio::EQ_PRESETS {
                        let active = self.eq_preset == *name;
                        if ui
                            .selectable_label(
                                active,
                                egui::RichText::new(*name).size(12.0).color(if active {
                                    SPOT_WHITE
                                } else {
                                    SPOT_GRAY
                                }),
                            )
                            .clicked()
                        {
                            self.player.set_eq_all(*gains);
                            self.eq_preset = String::from(*name);
                        }
                    }
                });
            });
        if !open {
            self.show_eq = false;
        }
    }

    /// Sinkronkan cover art dengan lagu aktif; dipanggil tiap frame dari ui().
    fn sync_cover(&mut self, ctx: &egui::Context) {
        let path = self.current.and_then(|i| self.tracks.get(i)).map(|t| t.path.clone());
        if path == self.cover_for {
            return;
        }
        self.cover_for = path.clone();
        self.cover_texture = path
            .as_ref()
            .and_then(|p| crate::library::cover_art(p))
            .and_then(|bytes| image::load_from_memory(&bytes).ok())
            .map(|img| {
                let rgba = img.to_rgba8();
                let (w, h) = (rgba.width() as usize, rgba.height() as usize);
                let pixels = rgba
                    .pixels()
                    .map(|p| egui::Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3]))
                    .collect::<Vec<_>>();
                let cimg = egui::ColorImage::new([w, h], pixels);
                ctx.load_texture("cover-art", cimg, egui::TextureOptions::LINEAR)
            });
    }

    /// Tampilan lirik ala Spotify: kolom tengah, baris aktif putih besar + karaoke.
    fn show_lyrics_view(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.vertical_centered(|ui| {
            ui.small(egui::RichText::new("klik baris untuk lompat \u{23E9}").color(SPOT_DIM));
        });
        ui.add_space(8.0);
        let pos = self.player.position();
        let state = self.lyric_state.clone();
        let avail = ui.available_width();
        let col_w = 640.0f32.min(avail - 32.0).max(200.0);
        let pad = ((avail - col_w) / 2.0).max(0.0);
        match state {
            LyricState::Loading => {
                ui.vertical_centered(|ui| {
                    ui.label(egui::RichText::new("\u{23F3} Mengambil lirik\u{2026}").size(20.0).color(SPOT_GRAY));
                });
            }
            LyricState::NotFound => {
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new("Lirik tidak ditemukan untuk lagu ini.")
                            .size(20.0)
                            .color(SPOT_GRAY),
                    );
                    ui.small(egui::RichText::new("Pastikan online saat pertama kali memutar lagu.").color(SPOT_DIM));
                });
            }
            LyricState::None => {
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new("Pilih lagu untuk melihat lirik.")
                            .size(20.0)
                            .color(SPOT_GRAY),
                    );
                });
            }
            LyricState::Loaded { lyrics, .. } => {
                if !lyrics.synced {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.add_space(pad);
                            ui.vertical(|ui| {
                                for line in &lyrics.lines {
                                    ui.add_sized(
                                        egui::vec2(col_w, 0.0),
                                        egui::Label::new(
                                            egui::RichText::new(&line.text)
                                                .size(22.0)
                                                .color(SPOT_DIM),
                                        )
                                        .wrap_mode(egui::TextWrapMode::Wrap),
                                    );
                                    ui.add_space(10.0);
                                }
                            });
                        });
                    });
                } else {
                    let active = lyrics.active_index(pos);
                    // Lagu cepat: kalau baris aktif lompat >= 2 baris dalam
                    // satu frame, scroll langsung snap (tanpa smoothing)
                    // agar lirik tidak tertinggal.
                    let jump = match (self.last_active, active) {
                        (Some(a), Some(b)) => (b as isize - a as isize).unsigned_abs(),
                        _ => 0,
                    };
                    self.last_active = active;
                    let snap = jump >= 2;
                    let mut active_center_y: Option<f32> = None;
                    // Ruang kosong di bawah harus >= setengah tinggi viewport
                    // agar baris terakhir pun bisa di-scroll sampai tengah.
                    let bottom_pad = self.lyric_viewport_h.max(240.0);
                    let output = egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .vertical_scroll_offset(self.lyric_scroll)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.add_space(pad);
                                ui.vertical(|ui| {
                                    ui.add_space(40.0);
                                    for (i, line) in lyrics.lines.iter().enumerate() {
                                        let is_active = Some(i) == active;
                                        let text = if line.text.is_empty() {
                                            "\u{266A}".to_string()
                                        } else {
                                            line.text.clone()
                                        };
                                        let rich = if is_active {
                                            egui::RichText::new(&text)
                                                .size(30.0)
                                                .strong()
                                                .color(SPOT_WHITE)
                                        } else {
                                            egui::RichText::new(&text)
                                                .size(22.0)
                                                .color(SPOT_DIM)
                                        };
                                        let resp = ui.add_sized(
                                            egui::vec2(col_w, 0.0),
                                            egui::Label::new(rich)
                                                .sense(egui::Sense::click())
                                                .wrap_mode(egui::TextWrapMode::Wrap),
                                        );
                                        if resp.clicked() {
                                            self.player.seek(line.time + 0.01);
                                        }
                                        if is_active {
                                            active_center_y = Some(resp.rect.center().y);
                                        }
                                        ui.add_space(16.0);
                                    }
                                    ui.add_space(bottom_pad);
                                });
                            });
                        });
                    if let Some(cy) = active_center_y {
                        let vh = output.inner_rect.height();
                        self.lyric_viewport_h = vh;
                        // cy dalam koordinat layar -> ubah ke offset scroll:
                        // target = posisi konten baris aktif - setengah viewport
                        let target =
                            (cy - output.inner_rect.min.y + self.lyric_scroll - vh / 2.0)
                                .max(0.0);
                        let diff = target - self.lyric_scroll;
                        if snap || diff.abs() < 0.5 {
                            self.lyric_scroll = target;
                        } else {
                            self.lyric_scroll += diff * 0.12;
                        }
                    }
                }
            }
        }
    }

    fn poll_channels(&mut self) {
        // Hasil scan folder
        if let Ok(tracks) = self.scan_rx.try_recv() {
            let n = tracks.len();
            self.tracks = tracks;
            self.status_msg = if n == 0 {
                String::from("Tidak ada file audio di folder ini.")
            } else {
                format!("{n} lagu ditemukan. Klik untuk memutar.")
            };
        }
        // Hasil unduhan lirik
        if let Ok((idx, lyrics, from_cache)) = self.lyric_rx.try_recv() {
            // Abaikan jika pengguna sudah pindah lagu
            if self.current == Some(idx) {
                match lyrics {
                    Some(l) if l.synced => {
                        self.status_msg = if from_cache {
                            String::from("Lirik tersinkron ✓ (tersimpan offline)")
                        } else {
                            String::from("Lirik tersinkron ✓ (baru diunduh & disimpan)")
                        };
                        self.lyric_state = LyricState::Loaded {
                            lyrics: l,
                            from_cache,
                        };
                    }
                    Some(l) => {
                        self.lyric_state = LyricState::Loaded {
                            lyrics: l,
                            from_cache,
                        };
                        self.status_msg =
                            String::from("Lirik ditemukan (tanpa timestamp sinkron).");
                    }
                    None => {
                        self.lyric_state = LyricState::NotFound;
                        self.status_msg =
                            String::from("Lirik tidak ditemukan (perlu internet).");
                    }
                }
            }
        }
    }

    fn filtered_indices(&self) -> Vec<usize> {
        let q = self.search.to_lowercase();
        // Bila playlist aktif, hanya tampilkan lagu di playlist itu.
        let in_playlist: Option<std::collections::HashSet<PathBuf>> =
            self.active_playlist.and_then(|i| {
                self.playlists
                    .get(i)
                    .map(|pl| pl.paths.iter().cloned().collect())
            });
        self.tracks
            .iter()
            .enumerate()
            .filter(|(_, t)| q.is_empty() || t.display().to_lowercase().contains(&q))
            .filter(|(_, t)| {
                in_playlist
                    .as_ref()
                    .map(|set| set.contains(&t.path))
                    .unwrap_or(true)
            })
            .map(|(i, _)| i)
            .collect()
    }
}

fn config_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("id", "joni", "JoniMusic")
        .map(|d| d.config_dir().join("config.json"))
}

fn load_config() -> (Option<PathBuf>, Option<String>) {
    let p = match config_path() {
        Some(p) => p,
        None => return (None, None),
    };
    let text = match std::fs::read_to_string(p) {
        Ok(t) => t,
        Err(_) => return (None, None),
    };
    let v: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(_) => return (None, None),
    };
    let dir = v
        .get("music_dir")
        .and_then(|s| s.as_str())
        .map(PathBuf::from)
        .filter(|pb| pb.is_dir());
    let audio_device = v
        .get("audio_device")
        .and_then(|s| s.as_str())
        .map(|s| s.to_string());
    (dir, audio_device)
}

fn save_config(dir: &Option<PathBuf>, audio_device: &Option<String>) {
    if let Some(p) = config_path() {
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let v = serde_json::json!({
            "music_dir": dir.as_ref().map(|d| d.to_string_lossy()),
            "audio_device": audio_device,
        });
        let _ = std::fs::write(p, v.to_string());
    }
}

fn fmt_time(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    format!("{:02}:{:02}", s / 60, s % 60)
}

impl eframe::App for MusicApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_channels();
        self.sync_cover(ui.ctx());

        // Lanjut otomatis ke lagu berikut saat selesai (hormati shuffle & repeat)
        if self.player.finished() {
            self.advance();
        }

        // Minimize -> ubah jadi mini player kecil transparan
        let minimized = ui.ctx().input(|i| i.viewport().minimized);
        if minimized == Some(true) && !self.mini_mode {
            let ctx = ui.ctx().clone();
            self.set_mini(&ctx, true);
        }

        // ---- Mode mini player ----
        if self.mini_mode {
            egui::CentralPanel::no_frame().show(ui, |ui| {
                self.show_mini(ui);
            });
            ui.ctx().request_repaint_after(Duration::from_millis(200));
            return;
        }

        // ---- Sidebar kiri: logo, nav, Your Library + tabel playlist ----
        // Lebar dikendalikan manual via `sidebar_width` + drag handle kustom
        // di bawah (bukan Panel::resizable bawaan egui yang handle-nya
        // tak terlihat & sulit digrab dengan Frame::NONE).
        egui::Panel::left("sidebar")
            .resizable(false)
            .exact_size(self.sidebar_width)
            .frame(egui::Frame::NONE.fill(SPOT_BLACK).inner_margin(16.0))
            .show(ui, |ui| {
                // Split eksplisit: konten atas + footer 70px.
                // (JANGAN pakai with_layout(bottom_up) di awal: kursor induk
                // lompat ke bawah sehingga list lagu terdorong keluar layar.)
                let r = ui.available_rect_before_wrap();
                let footer_h = 70.0;
                let content_r =
                    egui::Rect::from_min_max(r.min, egui::pos2(r.max.x, r.max.y - footer_h));
                let footer_r =
                    egui::Rect::from_min_max(egui::pos2(r.min.x, r.max.y - footer_h), r.max);
                {
                    let mut cui = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(content_r)
                            .layout(egui::Layout::top_down(egui::Align::Min)),
                    );
                    cui.label(
                        egui::RichText::new("🎵 Joni Music")
                            .size(20.0)
                            .strong()
                            .color(SPOT_WHITE),
                    );
                    cui.add_space(10.0);
                    cui.horizontal(|ui| {
                        for (icon, label, v) in
                            [("🏠", "Home", View::Playlist), ("🎤", "Lirik", View::Lyrics)]
                        {
                            let active = self.view == v;
                            if ui
                                .selectable_label(
                                    active,
                                    egui::RichText::new(format!("{icon}  {label}"))
                                        .size(14.0)
                                        .strong()
                                        .color(if active { SPOT_WHITE } else { SPOT_GRAY }),
                                )
                                .clicked()
                            {
                                self.view = v;
                            }
                            ui.add_space(10.0);
                        }
                    });
                    cui.add_space(8.0);
                    cui.label(
                        egui::RichText::new("Your Library")
                            .size(15.0)
                            .strong()
                            .color(SPOT_GRAY),
                    );
                    cui.add_space(4.0);
                    cui.add(
                        egui::TextEdit::singleline(&mut self.search)
                            .hint_text("🔍 Cari lagu...")
                            .desired_width(f32::INFINITY),
                    );
                    cui.add_space(10.0);
                    // ---- Playlist pengguna: chips + buat/hapus ----
                    cui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("Playlist")
                                .size(13.0)
                                .strong()
                                .color(SPOT_GRAY),
                        );
                        if ui
                            .small_button("＋")
                            .on_hover_text("Buat playlist baru")
                            .clicked()
                        {
                            self.show_playlist_input = !self.show_playlist_input;
                        }
                    });
                    if self.show_playlist_input {
                        cui.horizontal(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut self.new_playlist_name)
                                    .hint_text("Nama playlist...")
                                    .desired_width(140.0),
                            );
                            if ui.small_button("Buat").clicked()
                                && !self.new_playlist_name.trim().is_empty()
                            {
                                self.playlists.push(playlist::Playlist::new(
                                    self.new_playlist_name.trim(),
                                ));
                                playlist::save(&self.playlists);
                                self.new_playlist_name.clear();
                                self.show_playlist_input = false;
                            }
                        });
                        cui.add_space(4.0);
                    }
                    if !self.playlists.is_empty() {
                        cui.horizontal_wrapped(|ui| {
                            let mut delete_idx: Option<usize> = None;
                            let mut select_idx: Option<Option<usize>> = None;
                            // "Semua" untuk kembali ke library penuh.
                            if ui
                                .selectable_label(
                                    self.active_playlist.is_none(),
                                    egui::RichText::new("Semua").size(12.0),
                                )
                                .clicked()
                            {
                                select_idx = Some(None);
                            }
                            for (i, pl) in self.playlists.iter().enumerate() {
                                let active = self.active_playlist == Some(i);
                                if ui
                                    .selectable_label(
                                        active,
                                        egui::RichText::new(format!(
                                            "🎵 {} ({})",
                                            pl.name,
                                            pl.paths.len()
                                        ))
                                        .size(12.0),
                                    )
                                    .on_hover_text("Klik untuk filter; klik kanan untuk hapus")
                                    .clicked()
                                {
                                    select_idx = Some(Some(i));
                                }
                                // Klik kanan pada chip = hapus playlist.
                                // (egui: context menu per chip)
                                // Disederhanakan: tombol × kecil saat aktif.
                                if active
                                    && ui
                                        .small_button("×")
                                        .on_hover_text("Hapus playlist ini")
                                        .clicked()
                                {
                                    delete_idx = Some(i);
                                }
                            }
                            if let Some(sel) = select_idx {
                                self.active_playlist = sel;
                                self.current = None;
                                self.player.stop();
                            }
                            if let Some(i) = delete_idx {
                                self.playlists.remove(i);
                                playlist::save(&self.playlists);
                                self.active_playlist = None;
                            }
                        });
                        cui.add_space(4.0);
                    }
                    cui.add_space(6.0);
                    // (Tombol play hijau + info dihapus 2026-10-15 atas permintaan zura:
                    //  duplikat dari tombol play di control strip bawah.)
                    cui.add_space(8.0);
                    // Now playing: cover art + judul/artis lagu aktif.
                    if let Some(t) = self.current.and_then(|i| self.tracks.get(i)) {
                        cui.horizontal(|ui| {
                            let cover_size = 52.0;
                            if let Some(tex) = &self.cover_texture {
                                ui.add(
                                    egui::Image::new(tex)
                                        .max_size(egui::vec2(cover_size, cover_size))
                                        .corner_radius(6.0),
                                );
                            } else {
                                let (rect, _) = ui.allocate_exact_size(
                                    egui::vec2(cover_size, cover_size),
                                    egui::Sense::hover(),
                                );
                                ui.painter().rect_filled(
                                    rect,
                                    6.0,
                                    egui::Color32::from_rgb(0x28, 0x28, 0x28),
                                );
                                ui.painter().text(
                                    rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    "♪",
                                    egui::FontId::proportional(24.0),
                                    SPOT_DIM,
                                );
                            }
                            ui.add_space(8.0);
                            ui.vertical(|ui| {
                                ui.add_space(4.0);
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(&t.title)
                                            .size(13.0)
                                            .strong()
                                            .color(SPOT_WHITE),
                                    )
                                    .truncate(),
                                );
                                if !t.artist.is_empty() {
                                    ui.label(
                                        egui::RichText::new(&t.artist)
                                            .size(11.0)
                                            .color(SPOT_GRAY),
                                    );
                                }
                            });
                        });
                        cui.add_space(8.0);
                    }
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(&mut cui, |ui| {
                            self.show_track_table(ui, true);
                        });
                }
                // Garis pemisah + footer menempel di bawah
                ui.painter().line_segment(
                    [footer_r.left_top(), footer_r.right_top()],
                    egui::Stroke::new(1.0, SPOT_HOVER),
                );
                {
                    let mut fui = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(footer_r)
                            .layout(egui::Layout::top_down(egui::Align::Min)),
                    );
                    fui.add_space(8.0);
                    fui.horizontal(|ui| {
                        if ui.small_button("📁").on_hover_text("Folder Musik").clicked() {
                            if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                                self.set_music_dir(dir);
                            }
                        }
                        if ui.small_button("🔄").on_hover_text("Pindai Ulang").clicked() {
                            if let Some(dir) = self.music_dir.clone() {
                                self.set_music_dir(dir);
                            }
                        }
                        if ui.small_button("🧲").on_hover_text("Mini player").clicked() {
                            let ctx = ui.ctx().clone();
                            self.set_mini(&ctx, true);
                        }
                        // Pemilih perangkat output audio: headset/earphone yang baru
                        // dicolok sering tidak jadi default otomatis, jadi suara
                        // tetap keluar dari speaker laptop. Pilih manual di sini.
                        let sel = self
                            .audio_device
                            .as_deref()
                            .unwrap_or(DEFAULT_DEVICE_LABEL);
                        let shown: String = sel.chars().take(16).collect();
                        let combo = egui::ComboBox::from_id_salt("audio_device")
                            .selected_text(
                                egui::RichText::new(format!("🔈 {shown}")).size(11.0),
                            )
                            .width(170.0);
                        let resp = combo.show_ui(ui, |ui| {
                            // Refresh daftar tiap popup dibuka agar perangkat
                            // yang baru dicolok ikut muncul.
                            self.device_names = output_device_names();
                            let mut picked: Option<String> = None;
                            if ui
                                .selectable_label(
                                    self.audio_device.is_none(),
                                    DEFAULT_DEVICE_LABEL,
                                )
                                .on_hover_text("Ikuti default sistem")
                                .clicked()
                            {
                                picked = Some(String::new());
                            }
                            for name in self.device_names.clone() {
                                if ui
                                    .selectable_label(
                                        self.audio_device.as_deref() == Some(name.as_str()),
                                        &name,
                                    )
                                    .clicked()
                                {
                                    picked = Some(name);
                                }
                            }
                            picked
                        });
                        if let Some(Some(p)) = resp.inner {
                            let device = if p.is_empty() { None } else { Some(p) };
                            self.set_audio_device(device);
                        }
                    });
                    fui.add_space(4.0);
                    fui.small(
                        egui::RichText::new(self.status_msg.clone())
                            .size(11.0)
                            .color(SPOT_DIM),
                    );
                }
            });
        // ---- Konten utama: 80% lirik/playlist + 20% strip kontrol ----
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(SPOT_BG).inner_margin(egui::Margin::ZERO))
            .show(ui, |ui| {
                let r = ui.available_rect_before_wrap();
                let ch = (r.height() * 0.2).clamp(124.0, 190.0);
                let split_y = r.max.y - ch;
                let top_r =
                    egui::Rect::from_min_max(r.min, egui::pos2(r.max.x, split_y));
                let bot_r =
                    egui::Rect::from_min_max(egui::pos2(r.min.x, split_y), r.max);
                // Latar strip kontrol hitam
                ui.painter().rect_filled(bot_r, 0.0, SPOT_BLACK);
                {
                    let mut top_ui = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(top_r)
                            .layout(egui::Layout::top_down(egui::Align::Min)),
                    );
                    match self.view {
                        View::Playlist => self.show_playlist_page(&mut top_ui),
                        View::Lyrics => self.show_lyrics_view(&mut top_ui),
                    }
                }
                ui.painter().line_segment(
                    [bot_r.left_top(), bot_r.right_top()],
                    egui::Stroke::new(1.0, SPOT_HOVER),
                );
                {
                    let mut bot_ui = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(bot_r)
                            .layout(egui::Layout::top_down(egui::Align::Center)),
                    );
                    self.show_control_strip(&mut bot_ui);
                }
            });
        // ---- Drag handle kustom di tepi kanan sidebar ----
        // Strip 12px yang selalu terlihat samar & menyala saat hover/drag.
        // (Menggantikan Panel::resizable bawaan yang handle-nya tak kasat
        // mata dengan Frame::NONE sehingga pengguna mengira tidak bisa.)
        {
            let win_h = ui.max_rect().max.y;
            let x = self.sidebar_width;
            let strip = egui::Rect::from_min_max(
                egui::pos2(x - 6.0, 0.0),
                egui::pos2(x + 6.0, win_h),
            );
            let resp = ui.interact(
                strip,
                egui::Id::new("sidebar_resize"),
                egui::Sense::drag(),
            );
            if resp.dragged() {
                if let Some(p) = resp.interact_pointer_pos() {
                    // Sisakan minimal 200px untuk panel tengah.
                    let max_w = (ui.max_rect().width() - 200.0).max(240.0);
                    self.sidebar_width = p.x.clamp(240.0, 560.0).min(max_w);
                }
            }
            if resp.hovered() || resp.dragged() {
                ui.ctx()
                    .set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
            // Garis vertikal: samar saat idle, hijau terang saat hover/drag.
            let line_col = if resp.dragged() {
                SPOT_GREEN
            } else if resp.hovered() {
                SPOT_GREEN_HOVER
            } else {
                egui::Color32::from_rgba_unmultiplied(255, 255, 255, 28)
            };
            let lw = if resp.hovered() || resp.dragged() { 2.5 } else { 1.0 };
            ui.painter().line_segment(
                [egui::pos2(x, 0.0), egui::pos2(x, win_h)],
                egui::Stroke::new(lw, line_col),
            );
        }
        // Window equalizer (bila dibuka dari tombol 🎚).
        self.show_eq_window(ui.ctx());
        // Refresh terus agar lirik & progress sinkron.
        // Saat lagu diputar, repaint tiap frame supaya scroll lirik mulus;
        // saat pause cukup 5x/detik untuk hemat CPU.
        if self.player.is_playing() {
            ui.ctx().request_repaint();
        } else {
            ui.ctx().request_repaint_after(Duration::from_millis(200));
        }
    }
}

fn main() -> eframe::Result<()> {
    let ver = env!("CARGO_PKG_VERSION");
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 700.0])
            .with_title(format!("Joni Music v{ver}")),
        ..Default::default()
    };
    eframe::run_native(
        &format!("Joni Music v{ver}"),
        options,
        Box::new(|cc| {
            // Tema Spotify: warna diekstrak dari CSS produksi open.spotify.com
            let mut visuals = egui::Visuals::dark();
            visuals.window_fill = egui::Color32::from_rgb(0x12, 0x12, 0x12);
            visuals.panel_fill = egui::Color32::from_rgb(0x12, 0x12, 0x12);
            visuals.faint_bg_color = egui::Color32::from_rgb(0x28, 0x28, 0x28);
            visuals.extreme_bg_color = egui::Color32::from_rgb(0x18, 0x18, 0x18);
            visuals.selection.bg_fill = egui::Color32::from_rgb(0x1e, 0xd7, 0x60);
            visuals.selection.stroke.color = egui::Color32::from_rgb(0x1e, 0xd7, 0x60);
            cc.egui_ctx.set_visuals(visuals);
            // Font CJK (Jepang/Korea/China) sebagai fallback agar judul lagu
            // dan lirik berhuruf hiragana/katakana/kanji/hangul/hanzi tampil benar.
            let mut fonts = egui::FontDefinitions::default();
            fonts.font_data.insert(
                "noto_cjk_sc".to_owned(),
                std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
                    "../assets/fonts/NotoSansCJK-sc.ttf"
                ))),
            );
            fonts.font_data.insert(
                "noto_cjk_kr".to_owned(),
                std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
                    "../assets/fonts/NotoSansCJK-kr.ttf"
                ))),
            );
            for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                let entry = fonts.families.get_mut(&family).unwrap();
                entry.push("noto_cjk_sc".to_owned());
                entry.push("noto_cjk_kr".to_owned());
            }
            cc.egui_ctx.set_fonts(fonts);
            // Monitor thread: bangunkan UI tiap 500ms agar pengecekan
            // `player.finished()` -> `advance()` tetap jalan walau repaint
            // loop utama terhambat (mis. setelah minimize di Windows).
            // Tanpa ini, lagu yang selesai saat UI tertidur tidak lanjut
            // ke lagu berikutnya.
            let rep_ctx = cc.egui_ctx.clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_millis(500));
                rep_ctx.request_repaint();
            });
            Ok(Box::new(MusicApp::new()))
        }),
    )
}

#[cfg(test)]
mod progress_bar_tests {
    // Tes headless: memastikan REL slider yang digambar benar-benar selebar
    // yang diminta. egui::Slider mengabaikan lebar dari add_sized dan selalu
    // memakai spacing().slider_width (default 100px) — regresi v1.6.7.
    fn lebar_rel_terlebar(shapes: &[egui::epaint::ClippedShape]) -> Option<f32> {
        shapes
            .iter()
            .filter_map(|cs| match &cs.shape {
                egui::epaint::Shape::Rect(rs) => {
                    let (w, h) = (rs.rect.width(), rs.rect.height());
                    // Rel slider: tipis dan horizontal (bukan handle/latar).
                    if h < 14.0 && w > 50.0 {
                        Some(w)
                    } else {
                        None
                    }
                }
                _ => None,
            })
            .max_by(|a, b| a.partial_cmp(b).unwrap())
    }

    fn gambar(draw: impl FnMut(&mut egui::Ui)) -> Vec<egui::epaint::ClippedShape> {
        let ctx = egui::Context::default();
        let mut raw = egui::RawInput::default();
        raw.screen_rect = Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1280.0, 800.0),
        ));
        let mut out = ctx.run_ui(raw, draw);
        out.textures_delta.clear(); // headless: tidak ada GPU yang mengonsumsi tekstur
        out.shapes
    }

    #[test]
    fn rel_slider_memanjang_mengikuti_slider_width() {
        let shapes = gambar(|ui| {
            ui.spacing_mut().slider_width = 480.0;
            let mut p = 10.0;
            ui.add(
                egui::Slider::new(&mut p, 0.0..=269.0)
                    .show_value(false)
                    .trailing_fill(true),
            );
        });
        let w = lebar_rel_terlebar(&shapes).expect("rel slider tidak tergambar");
        assert!(
            (w - 480.0).abs() < 3.0,
            "lebar rel = {w:.1}px, harusnya ~480px"
        );
    }

    #[test]
    fn pola_lama_add_sized_tidak_memanjangkan_rel() {
        // Pola v1.6.7 (rusak): add_sized(480) — rel tetap 100px.
        let shapes = gambar(|ui| {
            let mut p = 10.0;
            ui.add_sized(
                egui::vec2(480.0, 0.0),
                egui::Slider::new(&mut p, 0.0..=269.0)
                    .show_value(false)
                    .trailing_fill(true),
            );
        });
        let w = lebar_rel_terlebar(&shapes).expect("rel slider tidak tergambar");
        assert!(
            (w - 100.0).abs() < 3.0,
            "lebar rel pola lama = {w:.1}px, harusnya ~100px"
        );
    }
}

#[cfg(test)]
mod volume_popup_tests {
    // Tes headless: klik tombol volume -> popup slider vertikal muncul di atasnya.
    fn rel_vertikal_tertinggi(shapes: &[egui::epaint::ClippedShape]) -> Option<f32> {
        shapes
            .iter()
            .filter_map(|cs| match &cs.shape {
                egui::epaint::Shape::Rect(rs) => {
                    let (w, h) = (rs.rect.width(), rs.rect.height());
                    if w < 14.0 && h > 50.0 {
                        Some(h)
                    } else {
                        None
                    }
                }
                _ => None,
            })
            .max_by(|a, b| a.partial_cmp(b).unwrap())
    }

    #[test]
    fn popup_volume_muncul_dengan_slider_vertikal() {
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0));
        let btn_center = std::cell::Cell::new(egui::Pos2::ZERO);

        // Replika tombol volume + popup seperti di show_control_strip.
        let mut gambar = |ui: &mut egui::Ui| {
            let r = ui.add_sized(egui::vec2(34.0, 28.0), egui::Button::new("VOL"));
            btn_center.set(r.rect.center());
            let mut vol = 70.0;
            egui::Popup::from_toggle_button_response(&r)
                .align(egui::RectAlign::TOP_END)
                .gap(8.0)
                .show(|ui| {
                    ui.spacing_mut().slider_width = 110.0;
                    ui.add(
                        egui::Slider::new(&mut vol, 0.0..=100.0)
                            .vertical()
                            .show_value(false),
                    )
                });
        };
        let mut frame = |events: Vec<egui::Event>| {
            let mut raw = egui::RawInput::default();
            raw.screen_rect = Some(screen);
            raw.events = events;
            let mut out = ctx.run_ui(raw, &mut gambar);
            out.textures_delta.clear();
            out.shapes
        };

        let s1 = frame(vec![]);
        assert!(
            rel_vertikal_tertinggi(&s1).is_none(),
            "popup seharusnya tertutup sebelum tombol diklik"
        );

        let c = btn_center.get();
        let klik = |pressed: bool| {
            vec![
                egui::Event::PointerMoved(c),
                egui::Event::PointerButton {
                    pos: c,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::default(),
                },
            ]
        };
        let _s2 = frame(klik(true));
        let _s3 = frame(klik(false));
        let s4 = frame(vec![]);
        let s3 = s4;

        let h = rel_vertikal_tertinggi(&s3).expect("slider vertikal tidak muncul setelah klik");
        assert!(
            (h - 110.0).abs() < 3.0,
            "tinggi rel vertikal = {h:.1}px, harusnya ~110px"
        );
    }
}

#[cfg(test)]
mod cjk_font_tests {
    // Pastikan font CJK yang dibundel memuat glyph untuk tiap aksara yang didukung:
    // hiragana, katakana, kanji/hanzi, dan hangul.
    const SC: &[u8] = include_bytes!("../assets/fonts/NotoSansCJK-sc.ttf");
    const KR: &[u8] = include_bytes!("../assets/fonts/NotoSansCJK-kr.ttf");

    fn punya_glyph(font: &[u8], ch: char) -> bool {
        ttf_parser::Face::from_slice(font, 0)
            .map(|f| f.glyph_index(ch).is_some())
            .unwrap_or(false)
    }

    #[test]
    fn font_sc_memuat_kana_dan_kanji() {
        // Hiragana, katakana, tanda baca Jepang
        for ch in [
            'あ', 'い', 'う', 'ア', 'イ', 'ウ', 'ー', '。', '、', '「', '」', '・',
        ] {
            assert!(punya_glyph(SC, ch), "glyph hilang: {ch}");
        }
        // Kanji/hanzi umum (Jepang & China)
        for ch in [
            '日', '本', '語', '歌', '愛', '心', '漢', '字', '中', '國', '龍', '龜',
        ] {
            assert!(punya_glyph(SC, ch), "glyph hilang: {ch}");
        }
    }

    #[test]
    fn font_kr_memuat_hangul() {
        for ch in ['한', '국', '어', '가', '나', '다', '김', '각', '힣', '뷁'] {
            assert!(punya_glyph(KR, ch), "glyph hilang: {ch}");
        }
        // Jamo
        for ch in ['\u{1100}', '\u{1161}', '\u{11A8}'] {
            assert!(punya_glyph(KR, ch), "glyph hilang: U+{:04X}", ch as u32);
        }
    }
}
