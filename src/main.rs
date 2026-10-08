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
    mini_mode: bool,
    view: View,
    view_hist: Vec<View>,
    view_hist_pos: usize,
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
            mini_mode: false,
            view: View::Playlist,
            view_hist: vec![View::Playlist],
            view_hist_pos: 0,
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
    fn goto_view(&mut self, v: View) {
        if self.view != v {
            self.view_hist.truncate(self.view_hist_pos + 1);
            self.view_hist.push(v);
            self.view_hist_pos = self.view_hist.len() - 1;
            self.view = v;
        }
    }

    fn go_back(&mut self) {
        if self.view_hist_pos > 0 {
            self.view_hist_pos -= 1;
            self.view = self.view_hist[self.view_hist_pos];
        }
    }

    fn go_forward(&mut self) {
        if self.view_hist_pos + 1 < self.view_hist.len() {
            self.view_hist_pos += 1;
            self.view = self.view_hist[self.view_hist_pos];
        }
    }

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
    fn show_playlist(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.add_space(20.0);
            if ui
                .add_sized(
                    egui::vec2(56.0, 56.0),
                    egui::Button::new(
                        egui::RichText::new("\u{25B6}").size(22.0).color(egui::Color32::BLACK),
                    )
                    .fill(SPOT_GREEN)
                    .corner_radius(egui::CornerRadius::same(28)),
                )
                .on_hover_text("Putar")
                .clicked()
            {
                self.toggle_play();
            }
            ui.add_space(12.0);
            ui.vertical(|ui| {
                ui.label(egui::RichText::new("Playlist").size(13.0).color(SPOT_GRAY));
                ui.label(
                    egui::RichText::new("Koleksi Lagu")
                        .size(40.0)
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
        ui.add_space(12.0);
        // Header tabel
        ui.horizontal(|ui| {
            ui.add_space(20.0);
            ui.add_sized(
                egui::vec2(36.0, 0.0),
                egui::Label::new(egui::RichText::new("#").size(12.0).color(SPOT_DIM)),
            );
            let tw = (ui.available_width() - 340.0).max(120.0);
            ui.add_sized(
                egui::vec2(tw, 0.0),
                egui::Label::new(egui::RichText::new("Judul").size(12.0).color(SPOT_DIM)),
            );
            ui.add_sized(
                egui::vec2(220.0, 0.0),
                egui::Label::new(egui::RichText::new("Album").size(12.0).color(SPOT_DIM)),
            );
            ui.label(egui::RichText::new("\u{23F1}").size(12.0).color(SPOT_DIM));
        });
        ui.separator();
        // Baris lagu
        let t_now = ui.ctx().input(|i| i.time);
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (num, idx) in self.filtered_indices().iter().enumerate() {
                let idx = *idx;
                let is_current = self.current == Some(idx);
                let (title, artist, album, dur_s) = {
                    let t = &self.tracks[idx];
                    (t.title.clone(), t.artist.clone(), t.album.clone(), t.duration_secs)
                };
                let (rect, resp) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 56.0),
                    egui::Sense::click(),
                );
                if resp.hovered() || is_current {
                    ui.painter().rect_filled(rect, 4.0, SPOT_HOVER);
                }
                if resp.clicked() {
                    self.play_index(idx);
                }
                let mut cui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(rect)
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                );
                cui.horizontal(|ui| {
                    ui.add_space(20.0);
                    if is_current {
                        eq_bars(ui, t_now);
                        ui.add_space(20.0);
                    } else {
                        ui.add_sized(
                            egui::vec2(36.0, 0.0),
                            egui::Label::new(
                                egui::RichText::new(format!("{}", num + 1))
                                    .size(14.0)
                                    .color(SPOT_GRAY),
                            ),
                        );
                    }
                    let title_col = if is_current { SPOT_GREEN } else { SPOT_WHITE };
                    let tw = (ui.available_width() - 340.0).max(120.0);
                    ui.allocate_ui_with_layout(
                        egui::vec2(tw, 56.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(&title).size(15.0).color(title_col),
                                )
                                .truncate(),
                            );
                            let artist_txt =
                                if artist.is_empty() { "Unknown Artist".to_string() } else { artist };
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(artist_txt).size(13.0).color(SPOT_GRAY),
                                )
                                .truncate(),
                            );
                        },
                    );
                    ui.add_space(8.0);
                    ui.add_sized(
                        egui::vec2(220.0, 0.0),
                        egui::Label::new(
                            egui::RichText::new(album).size(13.0).color(SPOT_GRAY),
                        )
                        .truncate(),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(16.0);
                        ui.label(
                            egui::RichText::new(fmt_time(dur_s)).size(13.0).color(SPOT_GRAY),
                        );
                    });
                });
            }
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
                    let dur = self.player.duration();
                    let mut active_center_y: Option<f32> = None;
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
                                    ui.add_space(120.0);
                                });
                            });
                        });
                    if let Some(cy) = active_center_y {
                        let target = (cy - output.inner_rect.height() / 2.0).max(0.0);
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

        // ---- Sidebar kiri (ala Spotify) ----
        egui::Panel::left("sidebar")
            .resizable(true)
            .default_size(300.0)
            .frame(egui::Frame::NONE.fill(SPOT_BLACK).inner_margin(16.0))
            .show(ui, |ui| {
                // Footer menempel di bawah
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    ui.small(
                        egui::RichText::new(self.status_msg.clone())
                            .size(11.0)
                            .color(SPOT_DIM),
                    );
                    ui.horizontal(|ui| {
                        if ui.small_button("\u{1F4C1}").on_hover_text("Folder Musik").clicked() {
                            if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                                self.set_music_dir(dir);
                            }
                        }
                        if ui.small_button("\u{1F504}").on_hover_text("Pindai Ulang").clicked() {
                            if let Some(dir) = self.music_dir.clone() {
                                self.set_music_dir(dir);
                            }
                        }
                        if ui.small_button("\u{1F9F2}").on_hover_text("Mini player").clicked() {
                            let ctx = ui.ctx().clone();
                            self.set_mini(&ctx, true);
                        }
                    });
                    ui.separator();
                });
                ui.label(
                    egui::RichText::new("\u{1F3B5} Joni Music")
                        .size(20.0)
                        .strong()
                        .color(SPOT_WHITE),
                );
                ui.add_space(14.0);
                for (icon, label, v) in
                    [("\u{1F3E0}", "Home", View::Playlist), ("\u{1F3A4}", "Lirik", View::Lyrics)]
                {
                    let active = self.view == v;
                    let col = if active { SPOT_WHITE } else { SPOT_GRAY };
                    if ui
                        .add_sized(
                            egui::vec2(ui.available_width(), 38.0),
                            egui::Button::new(
                                egui::RichText::new(format!("{icon}  {label}"))
                                    .size(15.0)
                                    .strong()
                                    .color(col),
                            )
                            .frame(false),
                        )
                        .clicked()
                    {
                        self.goto_view(v);
                    }
                }
                ui.add_space(6.0);
                ui.separator();
                ui.add_space(10.0);
                ui.label(
                    egui::RichText::new("Your Library")
                        .size(15.0)
                        .strong()
                        .color(SPOT_GRAY),
                );
                ui.add_space(6.0);
                ui.text_edit_singleline(&mut self.search);
                ui.add_space(6.0);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for idx in self.filtered_indices() {
                        let is_current = self.current == Some(idx);
                        let display = self.tracks[idx].display();
                        let (txt, col) = if is_current {
                            (format!("\u{25B6} {display}"), SPOT_GREEN)
                        } else {
                            (display, SPOT_GRAY)
                        };
                        let resp = ui.add(
                            egui::Label::new(egui::RichText::new(txt).size(13.0).color(col))
                                .sense(egui::Sense::click())
                                .truncate(),
                        );
                        if resp.clicked() {
                            self.play_index(idx);
                        }
                        ui.add_space(2.0);
                    }
                });
            });
        // ---- Konten utama (ala Spotify) ----
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(SPOT_BG).inner_margin(egui::Margin::ZERO))
            .show(ui, |ui| {
                // Top bar
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    if ui.small_button("\u{2039}").on_hover_text("Kembali").clicked() {
                        self.go_back();
                    }
                    if ui.small_button("\u{203A}").on_hover_text("Maju").clicked() {
                        self.go_forward();
                    }
                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            ui.add_space(16.0);
                            ui.small(
                                egui::RichText::new(format!(
                                    "v{}",
                                    env!("CARGO_PKG_VERSION")
                                ))
                                .color(SPOT_DIM),
                            );
                        },
                    );
                });
                ui.add_space(4.0);
                match self.view {
                    View::Playlist => self.show_playlist(ui),
                    View::Lyrics => self.show_lyrics_view(ui),
                }
            });
        // ---- Bar Now Playing (ala Spotify) ----
        egui::Panel::bottom("nowplaying")
            .resizable(false)
            .default_size(96.0)
            .frame(egui::Frame::NONE.fill(SPOT_BLACK).inner_margin(12.0))
            .show(ui, |ui| {
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    // Kiri: info lagu
                    ui.allocate_ui_with_layout(
                        egui::vec2(300.0, 72.0),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            let (ar, _) =
                                ui.allocate_exact_size(egui::vec2(56.0, 56.0), egui::Sense::hover());
                            ui.painter().rect_filled(ar, 6.0, SPOT_CARD);
                            ui.painter().text(
                                ar.center(),
                                egui::Align2::CENTER_CENTER,
                                "\u{266A}",
                                egui::FontId::proportional(26.0),
                                SPOT_GRAY,
                            );
                            ui.add_space(10.0);
                            match self.current.and_then(|i| self.tracks.get(i)) {
                                Some(t) => {
                                    ui.vertical(|ui| {
                                        ui.add(
                                            egui::Label::new(
                                                egui::RichText::new(&t.title)
                                                    .size(14.0)
                                                    .color(SPOT_WHITE),
                                            )
                                            .truncate(),
                                        );
                                        ui.add(
                                            egui::Label::new(
                                                egui::RichText::new(if t.artist.is_empty() {
                                                    "Unknown Artist"
                                                } else {
                                                    t.artist.as_str()
                                                })
                                                .size(12.0)
                                                .color(SPOT_GRAY),
                                            )
                                            .truncate(),
                                        );
                                    });
                                }
                                None => {
                                    ui.small(egui::RichText::new("Belum ada lagu").color(SPOT_GRAY));
                                }
                            }
                        },
                    );
                    // Tengah: kontrol + progress
                    let center_w = (ui.available_width() - 280.0).max(220.0);
                    ui.allocate_ui_with_layout(
                        egui::vec2(center_w, 72.0),
                        egui::Layout::top_down(egui::Align::Center),
                        |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 18.0;
                                let dim = SPOT_DIM;
                                let sh_col = if self.shuffle { SPOT_GREEN } else { dim };
                                if ui
                                    .button(egui::RichText::new("\u{1F500}").size(16.0).color(sh_col))
                                    .on_hover_text("Acak")
                                    .clicked()
                                {
                                    self.shuffle = !self.shuffle;
                                    if self.shuffle {
                                        self.ensure_shuffle_order();
                                    }
                                }
                                if ui
                                    .button(egui::RichText::new("\u{23EE}").size(16.0).color(SPOT_GRAY))
                                    .clicked()
                                {
                                    self.prev();
                                }
                                let play_label = if self.player.is_playing() { "\u{23F8}" } else { "\u{25B6}" };
                                if ui
                                    .add_sized(
                                        egui::vec2(40.0, 40.0),
                                        egui::Button::new(
                                            egui::RichText::new(play_label)
                                                .size(18.0)
                                                .color(egui::Color32::BLACK),
                                        )
                                        .fill(SPOT_WHITE)
                                        .corner_radius(egui::CornerRadius::same(20)),
                                    )
                                    .clicked()
                                {
                                    self.toggle_play();
                                }
                                if ui
                                    .button(egui::RichText::new("\u{23ED}").size(16.0).color(SPOT_GRAY))
                                    .clicked()
                                {
                                    self.next();
                                }
                                let (rep_icon, rep_col) = match self.repeat {
                                    RepeatMode::Off => ("\u{1F501}", dim),
                                    RepeatMode::All => ("\u{1F501}", SPOT_GREEN),
                                    RepeatMode::One => ("\u{1F502}", SPOT_GREEN),
                                };
                                if ui
                                    .button(egui::RichText::new(rep_icon).size(16.0).color(rep_col))
                                    .on_hover_text("Ulangi: mati / semua / satu")
                                    .clicked()
                                {
                                    self.repeat = match self.repeat {
                                        RepeatMode::Off => RepeatMode::All,
                                        RepeatMode::All => RepeatMode::One,
                                        RepeatMode::One => RepeatMode::Off,
                                    };
                                }
                            });
                            ui.add_space(2.0);
                            ui.horizontal(|ui| {
                                let dur = self.player.duration().max(0.01);
                                let mut pos = self.player.position();
                                ui.label(
                                    egui::RichText::new(fmt_time(pos)).size(11.0).color(SPOT_GRAY),
                                );
                                let sw = (ui.available_width() - 80.0).max(80.0);
                                if ui
                                    .add_sized(
                                        egui::vec2(sw, 0.0),
                                        egui::Slider::new(&mut pos, 0.0..=dur)
                                            .show_value(false)
                                            .trailing_fill(true),
                                    )
                                    .drag_stopped()
                                {
                                    self.player.seek(pos);
                                }
                                ui.label(
                                    egui::RichText::new(fmt_time(dur)).size(11.0).color(SPOT_GRAY),
                                );
                            });
                        },
                    );
                    // Kanan: lirik, mini, volume
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let mut vol = self.volume;
                        if ui
                            .add_sized(
                                egui::vec2(90.0, 0.0),
                                egui::Slider::new(&mut vol, 0.0..=100.0).show_value(false),
                            )
                            .changed()
                        {
                            self.volume = vol;
                            self.player.set_volume(vol / 100.0);
                        }
                        ui.label(egui::RichText::new("\u{1F50A}").size(14.0));
                        if ui
                            .button(egui::RichText::new("\u{1F9F2}").size(16.0))
                            .on_hover_text("Mini player")
                            .clicked()
                        {
                            let ctx = ui.ctx().clone();
                            self.set_mini(&ctx, true);
                        }
                        let mic_col = if self.view == View::Lyrics { SPOT_GREEN } else { SPOT_GRAY };
                        if ui
                            .button(egui::RichText::new("\u{1F3A4}").size(16.0).color(mic_col))
                            .on_hover_text("Lirik")
                            .clicked()
                        {
                            self.goto_view(if self.view == View::Lyrics {
                                View::Playlist
                            } else {
                                View::Lyrics
                            });
                        }
                    });
                });
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
