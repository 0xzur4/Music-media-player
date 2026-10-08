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

    fn next(&mut self) {
        if self.tracks.is_empty() {
            return;
        }
        let n = match self.current {
            Some(i) => (i + 1) % self.tracks.len(),
            None => 0,
        };
        self.play_index(n);
    }

    fn prev(&mut self) {
        if self.tracks.is_empty() {
            return;
        }
        let n = match self.current {
            Some(0) | None => self.tracks.len() - 1,
            Some(i) => i - 1,
        };
        self.play_index(n);
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

        // Lanjut otomatis ke lagu berikut saat selesai
        if self.player.finished() {
            self.next();
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

        // ---- Kiri: List Lagu (full height) ----
        egui::Panel::left("playlist")
            .resizable(true)
            .default_size(300.0)
            .show(ui, |ui| {
                ui.heading(format!("Playlist ({})", self.tracks.len()));
                ui.text_edit_singleline(&mut self.search);
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for idx in self.filtered_indices() {
                        let is_current = self.current == Some(idx);
                        let (display, dur_s) = {
                            let t = &self.tracks[idx];
                            (t.display(), t.duration_secs)
                        };
                        // Tanpa blok hijau: lagu aktif ditandai ▶ putih tebal
                        let row = if is_current {
                            egui::RichText::new(format!("▶ {display}"))
                                .size(14.0)
                                .strong()
                                .color(egui::Color32::WHITE)
                        } else {
                            egui::RichText::new(display)
                                .size(14.0)
                                .color(egui::Color32::from_gray(175))
                        };
                        let resp = ui.add(
                            egui::Label::new(row)
                                .sense(egui::Sense::click())
                                .truncate(),
                        );
                        if resp.clicked() {
                            self.play_index(idx);
                        }
                        ui.small(
                            egui::RichText::new(fmt_time(dur_s))
                                .color(egui::Color32::from_gray(110)),
                        );
                        ui.add_space(4.0);
                    }
                });
            });

        // ---- Tengah: LIRIK (karaoke, gaya Spotify) ----
        egui::CentralPanel::default().show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Lirik");
                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            ui.small("klik baris untuk lompat ⏩");
                        },
                    );
                });
                ui.separator();
                let pos = self.player.position();
                // Clone agar bisa seek (&mut self) dari dalam closure.
                let state = self.lyric_state.clone();
                match state {
                    LyricState::Loading => {
                        ui.label("⏳ Mengambil lirik…");
                    }
                    LyricState::NotFound => {
                        ui.label("Lirik tidak ditemukan untuk lagu ini.");
                        ui.small("Pastikan online saat pertama kali memutar lagu.");
                    }
                    LyricState::None => {
                        ui.label("Pilih lagu untuk melihat lirik.");
                    }
                    LyricState::Loaded { lyrics, .. } => {
                        if !lyrics.synced {
                            egui::ScrollArea::vertical().show(ui, |ui| {
                                for line in &lyrics.lines {
                                    ui.label(egui::RichText::new(&line.text).size(17.0));
                                    ui.add_space(6.0);
                                }
                            });
                        } else {
                            let active = lyrics.active_index(pos);
                            let dur = self.player.duration();
                            let mut active_center_y: Option<f32> = None;
                            let output = egui::ScrollArea::vertical()
                                .auto_shrink([false, false])
                                .vertical_scroll_offset(self.lyric_scroll)
                                .show(ui, |ui| {
                                    ui.add_space(60.0);
                                    for (i, line) in lyrics.lines.iter().enumerate() {
                                        let is_active = Some(i) == active;
                                        let text = if line.text.is_empty() {
                                            "♪".to_string()
                                        } else {
                                            line.text.clone()
                                        };
                                        let rich = if is_active {
                                            egui::RichText::new(&text)
                                                .size(25.0)
                                                .strong()
                                                .color(egui::Color32::WHITE)
                                        } else {
                                            egui::RichText::new(&text)
                                                .size(18.0)
                                                .color(egui::Color32::from_gray(135))
                                        };
                                        let resp = ui.add(
                                            egui::Label::new(rich)
                                                .sense(egui::Sense::click())
                                                .wrap_mode(egui::TextWrapMode::Wrap),
                                        );
                                        // Klik baris lirik -> lompat ke bagian itu
                                        if resp.clicked() {
                                            self.player.seek(line.time + 0.01);
                                        }
                                        if is_active {
                                            active_center_y = Some(resp.rect.center().y);
                                            // --- Efek karaoke: bar hijau berjalan ---
                                            let next_t = lyrics
                                                .lines
                                                .get(i + 1)
                                                .map(|l| l.time)
                                                .unwrap_or(dur.max(pos + 4.0));
                                            let span = (next_t - line.time).max(0.5);
                                            let p = ((pos - line.time) / span).clamp(0.0, 1.0);
                                            let bar = egui::Rect::from_min_size(
                                                egui::pos2(
                                                    resp.rect.min.x,
                                                    resp.rect.max.y - 2.0,
                                                ),
                                                egui::vec2(
                                                    resp.rect.width() * p as f32,
                                                    3.0,
                                                ),
                                            );
                                            ui.painter().rect_filled(
                                                bar,
                                                1.5,
                                                egui::Color32::from_rgb(29, 185, 84),
                                            );
                                        }
                                        ui.add_space(14.0);
                                    }
                                    ui.add_space(120.0);
                                });
                            // --- Auto-scroll halus ke baris aktif ---
                            if let Some(cy) = active_center_y {
                                let target =
                                    (cy - output.inner_rect.height() / 2.0).max(0.0);
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
            });

        // ---- Bawah: Pengaturan Musik ----
        egui::Panel::bottom("controls")
            .resizable(false)
            .default_size(112.0)
            .show(ui, |ui| {
                ui.add_space(6.0);
                // Baris 1: info lagu + tombol utilitas + status
                ui.horizontal(|ui| {
                    match self.current.and_then(|i| self.tracks.get(i)) {
                        Some(t) => {
                            ui.label(
                                egui::RichText::new(format!(
                                    "{} — {}",
                                    t.title,
                                    if t.artist.is_empty() {
                                        "Unknown Artist"
                                    } else {
                                        t.artist.as_str()
                                    }
                                ))
                                .strong()
                                .size(15.0),
                            );
                        }
                        None => {
                            ui.label("Belum ada lagu diputar");
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.small(
                            egui::RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                                .color(egui::Color32::from_gray(120)),
                        );
                        ui.separator();
                        if ui
                            .button("🧲 Mini")
                            .on_hover_text("Ubah jadi layar kecil transparan")
                            .clicked()
                        {
                            let ctx = ui.ctx().clone();
                            self.set_mini(&ctx, true);
                        }
                        if ui.button("🔄 Pindai Ulang").clicked() {
                            if let Some(dir) = self.music_dir.clone() {
                                self.set_music_dir(dir);
                            }
                        }
                        if ui.button("📁 Folder Musik").clicked() {
                            if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                                self.set_music_dir(dir);
                            }
                        }
                        ui.separator();
                        ui.label(&self.status_msg);
                    });
                });
                ui.add_space(4.0);
                // Baris 2: kontrol pemutaran + progress + volume
                ui.horizontal(|ui| {
                    if ui.button("⏮").clicked() {
                        self.prev();
                    }
                    let play_label = if self.player.is_playing() { "⏸" } else { "▶" };
                    if ui
                        .add_sized(egui::vec2(56.0, 36.0), egui::Button::new(play_label))
                        .clicked()
                    {
                        if self.player.has_track() {
                            self.player.toggle();
                        } else if !self.tracks.is_empty() {
                            self.play_index(0);
                        }
                    }
                    if ui.button("⏭").clicked() {
                        self.next();
                    }
                    ui.add_space(8.0);
                    let dur = self.player.duration().max(0.01);
                    let mut pos = self.player.position();
                    ui.label(fmt_time(pos));
                    // Slider mengisi sisa lebar; sisakan ruang untuk durasi + volume
                    let slider_w = (ui.available_width() - 175.0).max(60.0);
                    let slider = egui::Slider::new(&mut pos, 0.0..=dur)
                        .show_value(false)
                        .trailing_fill(true);
                    if ui
                        .add_sized(egui::vec2(slider_w, 0.0), slider)
                        .drag_stopped()
                    {
                        self.player.seek(pos);
                    }
                    ui.label(fmt_time(dur));
                    ui.add_space(8.0);
                    ui.label("🔊");
                    let mut vol = self.volume;
                    if ui
                        .add_sized(
                            egui::vec2(100.0, 0.0),
                            egui::Slider::new(&mut vol, 0.0..=100.0).show_value(false),
                        )
                        .changed()
                    {
                        self.volume = vol;
                        self.player.set_volume(vol / 100.0);
                    }
                });
                ui.add_space(4.0);
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
            // Tema gelap ala Spotify
            let mut visuals = egui::Visuals::dark();
            visuals.window_fill = egui::Color32::from_rgb(18, 18, 18);
            visuals.panel_fill = egui::Color32::from_rgb(22, 22, 22);
            visuals.faint_bg_color = egui::Color32::from_rgb(30, 30, 30);
            visuals.selection.bg_fill = egui::Color32::from_rgb(29, 185, 84);
            visuals.selection.stroke.color = egui::Color32::from_rgb(29, 185, 84);
            cc.egui_ctx.set_visuals(visuals);
            Ok(Box::new(MusicApp::new()))
        }),
    )
}
