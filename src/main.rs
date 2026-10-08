#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod library;
mod lyrics;

use audio::Player;
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
    mini_mode: bool,
    view: View,
    shuffle: bool,
    repeat: RepeatMode,
    shuffle_order: Vec<usize>,
    shuffle_pos: usize,
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
            mini_mode: false,
            view: View::Lyrics,
            shuffle: false,
            repeat: RepeatMode::Off,
            shuffle_order: Vec::new(),
            shuffle_pos: 0,
        };
        app.player.set_volume(0.8);

        if let Some(dir) = load_music_dir() {
            app.set_music_dir(dir);
        }
        app
    }

    fn set_music_dir(&mut self, dir: PathBuf) {
        self.music_dir = Some(dir.clone());
        save_music_dir(&dir);
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
        let ch = ui.available_height();
        ui.add_space(((ch - 128.0) / 2.0).max(2.0));
        // Baris 1: kontrol terpusat
        ui.allocate_ui_with_layout(
            egui::vec2(246.0, 46.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
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
                            egui::RichText::new("⏮").size(17.0).color(SPOT_GRAY),
                        )
                        .frame(false),
                    )
                    .on_hover_text("Sebelumnya")
                    .clicked()
                {
                    self.prev();
                }
                let play_glyph = if self.player.is_playing() {
                    "⏸"
                } else {
                    "▶"
                };
                if ui
                    .add_sized(
                        egui::vec2(46.0, 46.0),
                        egui::Button::new(
                            egui::RichText::new(play_glyph)
                                .size(19.0)
                                .color(egui::Color32::BLACK),
                        )
                        .fill(SPOT_WHITE)
                        .corner_radius(egui::CornerRadius::same(23)),
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
                            egui::RichText::new("⏭").size(17.0).color(SPOT_GRAY),
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
        ui.add_space(4.0);
        // Baris 2: progress + waktu
        let dur = self.player.duration().max(0.01);
        let pos = self.player.position();
        let bw = (ui.available_width() - 48.0).clamp(220.0, 440.0);
        ui.allocate_ui_with_layout(
            egui::vec2(bw, 22.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.add_sized(
                    egui::vec2(44.0, 0.0),
                    egui::Label::new(
                        egui::RichText::new(fmt_time(pos)).size(11.0).color(SPOT_GRAY),
                    ),
                );
                let sw = (bw - 104.0).max(60.0);
                let mut p = pos;
                if ui
                    .add_sized(
                        egui::vec2(sw, 0.0),
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
        ui.add_space(4.0);
        // Baris 3: volume + lirik + mini
        ui.allocate_ui_with_layout(
            egui::vec2(232.0, 28.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                ui.label(egui::RichText::new("🔊").size(15.0).color(SPOT_GRAY));
                let mut vol = self.volume;
                if ui
                    .add_sized(
                        egui::vec2(110.0, 0.0),
                        egui::Slider::new(&mut vol, 0.0..=100.0).show_value(false),
                    )
                    .changed()
                {
                    self.volume = vol;
                    self.player.set_volume(vol / 100.0);
                }
                let mic_col = if self.view == View::Lyrics {
                    SPOT_GREEN
                } else {
                    SPOT_GRAY
                };
                if ui
                    .add_sized(
                        egui::vec2(34.0, 28.0),
                        egui::Button::new(
                            egui::RichText::new("🎤").size(16.0).color(mic_col),
                        )
                        .frame(false),
                    )
                    .on_hover_text("Lirik")
                    .clicked()
                {
                    self.view = View::Lyrics;
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
            },
        );
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
                    let dur = self.player.duration();
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
                                            let next_t = lyrics
                                                .lines
                                                .get(i + 1)
                                                .map(|l| l.time)
                                                .unwrap_or(dur.max(pos + 4.0));
                                            let span = (next_t - line.time).max(0.5);
                                            let p = ((pos - line.time) / span).clamp(0.0, 1.0);
                                            let bar = egui::Rect::from_min_size(
                                                egui::pos2(resp.rect.min.x, resp.rect.max.y - 2.0),
                                                egui::vec2(resp.rect.width() * p as f32, 3.0),
                                            );
                                            ui.painter().rect_filled(bar, 1.5, SPOT_GREEN);
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
                        if diff.abs() < 0.5 {
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
        self.tracks
            .iter()
            .enumerate()
            .filter(|(_, t)| q.is_empty() || t.display().to_lowercase().contains(&q))
            .map(|(i, _)| i)
            .collect()
    }
}

fn config_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("id", "joni", "JoniMusic")
        .map(|d| d.config_dir().join("config.json"))
}

fn load_music_dir() -> Option<PathBuf> {
    let p = config_path()?;
    let text = std::fs::read_to_string(p).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let dir = v.get("music_dir")?.as_str()?;
    let pb = PathBuf::from(dir);
    if pb.is_dir() {
        Some(pb)
    } else {
        None
    }
}

fn save_music_dir(dir: &PathBuf) {
    if let Some(p) = config_path() {
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let v = serde_json::json!({ "music_dir": dir.to_string_lossy() });
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
        egui::Panel::left("sidebar")
            .resizable(true)
            .default_size(360.0)
            .min_size(240.0)
            .max_size(560.0)
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
                    // Tombol play hijau + info
                    cui.horizontal(|ui| {
                        self.green_play_button(ui, 50.0);
                        ui.add_space(8.0);
                        ui.vertical(|ui| {
                            ui.add_space(6.0);
                            ui.label(
                                egui::RichText::new(format!("{} lagu", self.tracks.len()))
                                    .size(12.0)
                                    .color(SPOT_DIM),
                            );
                            ui.label(
                                egui::RichText::new("Klik baris untuk memutar")
                                    .size(11.0)
                                    .color(SPOT_DIM),
                            );
                        });
                    });
                    cui.add_space(8.0);
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
        // Refresh terus agar lirik & progress sinkron
        ui.ctx().request_repaint_after(Duration::from_millis(200));
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
            Ok(Box::new(MusicApp::new()))
        }),
    )
}
