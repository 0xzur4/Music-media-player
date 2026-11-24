#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod config;
mod library;
mod lyrics;
mod playlist;
mod theme;

use audio::{output_device_names, Player, DEFAULT_DEVICE_LABEL};
use config::{RepeatMode, SearchScope, Settings, SortKey};
use eframe::egui::{self, pos2, vec2, Align2, Color32, CornerRadius, Rect, Sense, Stroke};
use library::Track;
use lyrics::{get_lyrics_with_options, Lyrics};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};
use theme::Icon;

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

struct MusicApp {
    player: Player,
    tracks: Vec<Track>,
    current: Option<usize>,
    music_dir: Option<PathBuf>,
    lyric_state: LyricState,
    lyric_rx: Receiver<(PathBuf, Option<Lyrics>, bool)>,
    lyric_tx: Sender<(PathBuf, Option<Lyrics>, bool)>,
    scan_rx: Receiver<Vec<Track>>,
    scan_tx: Sender<Vec<Track>>,
    search: String,
    search_scope: SearchScope,
    focus_search: bool,
    sort_key: SortKey,
    sort_asc: bool,
    volume: f32,
    muted: bool,
    status_msg: String,
    lyric_scroll: f32,
    lyric_viewport_h: f32,
    last_active: Option<usize>,
    /// Preview posisi seek saat user menyeret progress bar (agar slider tidak melompat balik).
    seek_preview: Option<f64>,
    audio_device: Option<String>,
    device_names: Vec<String>,
    mini_mode: bool,
    view: View,
    shuffle: bool,
    repeat: RepeatMode,
    shuffle_order: Vec<usize>,
    shuffle_pos: usize,
    sidebar_width: f32,
    show_eq: bool,
    show_shortcuts: bool,
    eq_preset: String,
    cover_texture: Option<egui::TextureHandle>,
    cover_accent: Option<Color32>,
    cover_for: Option<PathBuf>,
    playlists: Vec<playlist::Playlist>,
    /// Playlist yang sedang dilihat di tabel.
    active_playlist: Option<usize>,
    /// Playlist konteks pemutaran (dipakai oleh next/prev/shuffle/advance).
    playing_playlist: Option<usize>,
    new_playlist_name: String,
    show_playlist_input: bool,
    focus_playlist_input: bool,
    auto_mini: bool,
    /// Untuk melanjutkan lagu terakhir setelah scan selesai saat startup.
    pending_restore: Option<(PathBuf, f64)>,
    last_pos_save: Instant,
}

impl MusicApp {
    fn new() -> Self {
        let (lyric_tx, lyric_rx) = mpsc::channel();
        let (scan_tx, scan_rx) = mpsc::channel();
        let cfg = config::load();

        let (mut player, audio_warn) = Player::new_graceful(cfg.audio_device.as_deref());
        player.set_volume(cfg.volume);
        player.set_muted(cfg.muted);
        player.set_eq_all(cfg.eq_gains);

        let initial_status = if let Some(w) = audio_warn {
            w
        } else if cfg.music_dir.is_some() {
            String::from("Memindai folder musik…")
        } else {
            String::from("Pilih folder musik untuk mulai.")
        };

        let pending_restore = cfg.last_track.clone().map(|p| (p, cfg.last_pos));
        let active_device = player.device_label().map(|s| s.to_string());

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
            search_scope: cfg.search_scope,
            focus_search: false,
            sort_key: cfg.sort_key,
            sort_asc: cfg.sort_asc,
            volume: cfg.volume,
            muted: cfg.muted,
            status_msg: initial_status,
            lyric_scroll: 0.0,
            lyric_viewport_h: 500.0,
            last_active: None,
            seek_preview: None,
            audio_device: active_device,
            device_names: output_device_names(),
            mini_mode: false,
            view: if cfg.view_lyrics {
                View::Lyrics
            } else {
                View::Playlist
            },
            shuffle: cfg.shuffle,
            repeat: cfg.repeat,
            shuffle_order: Vec::new(),
            shuffle_pos: 0,
            sidebar_width: cfg.sidebar_width.clamp(210.0, 420.0),
            show_eq: false,
            show_shortcuts: false,
            eq_preset: cfg.eq_preset,
            cover_texture: None,
            cover_accent: None,
            cover_for: None,
            playlists: playlist::load(),
            active_playlist: None,
            playing_playlist: None,
            new_playlist_name: String::new(),
            show_playlist_input: false,
            focus_playlist_input: false,
            auto_mini: cfg.auto_mini,
            pending_restore,
            last_pos_save: Instant::now(),
        };

        if let Some(dir) = cfg.music_dir {
            if dir.is_dir() {
                app.start_scan(dir, false);
            }
        }
        app
    }

    fn current_settings(&self) -> Settings {
        let last_track = self
            .current
            .and_then(|i| self.tracks.get(i))
            .map(|t| t.path.clone())
            .or_else(|| self.pending_restore.as_ref().map(|(p, _)| p.clone()));
        let last_pos = if self.current.is_some() {
            self.player.position()
        } else {
            self.pending_restore.as_ref().map(|(_, p)| *p).unwrap_or(0.0)
        };
        Settings {
            music_dir: self.music_dir.clone(),
            audio_device: self.audio_device.clone(),
            auto_mini: self.auto_mini,
            volume: self.volume,
            muted: self.muted,
            eq_gains: self.player.eq_gains_snapshot(),
            eq_preset: self.eq_preset.clone(),
            shuffle: self.shuffle,
            repeat: self.repeat,
            sidebar_width: self.sidebar_width,
            last_track,
            last_pos,
            view_lyrics: self.view == View::Lyrics,
            sort_key: self.sort_key,
            sort_asc: self.sort_asc,
            search_scope: self.search_scope,
        }
    }

    fn save_settings(&self) {
        config::save(&self.current_settings());
    }

    fn start_scan(&mut self, dir: PathBuf, stop_playback: bool) {
        self.music_dir = Some(dir.clone());
        if stop_playback {
            self.tracks.clear();
            self.current = None;
            self.player.stop();
            self.lyric_state = LyricState::None;
            self.pending_restore = None;
        }
        self.save_settings();
        self.status_msg = String::from("Memindai folder musik…");
        let tx = self.scan_tx.clone();
        std::thread::spawn(move || {
            let tracks = library::scan(&dir);
            let _ = tx.send(tracks);
        });
    }

    fn set_audio_device(&mut self, device: Option<String>) {
        let label = device.as_deref();
        match self.player.set_output_device(label) {
            Ok(()) => {
                self.audio_device = device;
                self.save_settings();
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

    fn fetch_lyrics_for(&mut self, track: Track, force_online: bool) {
        self.lyric_state = LyricState::Loading;
        let tx = self.lyric_tx.clone();
        let path = track.path.clone();
        std::thread::spawn(move || {
            let (lyrics, from_cache) = get_lyrics_with_options(&track, force_online);
            let _ = tx.send((path, lyrics, from_cache));
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
                self.pending_restore = None;
                self.seek_preview = None;
                if self.shuffle {
                    self.ensure_shuffle_order();
                }
                self.lyric_scroll = 0.0;
                self.last_active = None;
                self.status_msg = format!("Memutar: {}", track.display());
                self.save_settings();
                self.fetch_lyrics_for(track, false);
            }
            Err(e) => {
                self.status_msg = format!("Gagal memutar: {e}");
            }
        }
    }

    /// Mulai putar dari konteks tampilan saat ini (playlist aktif atau semua lagu).
    fn play_from_view(&mut self, idx: usize) {
        if self.playing_playlist != self.active_playlist {
            self.playing_playlist = self.active_playlist;
            self.shuffle_order.clear();
        }
        self.play_index(idx);
    }

    fn toggle_play(&mut self) {
        if self.player.has_track() {
            self.player.toggle();
            if !self.player.is_playing() {
                self.save_settings();
            }
        } else {
            let pool = self.filtered_indices();
            if let Some(&first) = pool.first() {
                self.play_from_view(first);
            } else if !self.tracks.is_empty() {
                self.playing_playlist = None;
                self.play_index(0);
            }
        }
    }

    /// Daftar index lagu yang menjadi pool navigasi (hormati playlist konteks pemutaran).
    fn playback_pool(&self) -> Vec<usize> {
        if let Some(pi) = self.playing_playlist {
            if let Some(pl) = self.playlists.get(pi) {
                let set: HashSet<&PathBuf> = pl.paths.iter().collect();
                let pool: Vec<usize> = self
                    .tracks
                    .iter()
                    .enumerate()
                    .filter(|(_, t)| set.contains(&t.path))
                    .map(|(i, _)| i)
                    .collect();
                if !pool.is_empty() {
                    return pool;
                }
            }
        }
        (0..self.tracks.len()).collect()
    }

    fn next(&mut self) {
        let pool = self.playback_pool();
        if pool.is_empty() {
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
            let next_idx = match self
                .current
                .and_then(|c| pool.iter().position(|&i| i == c))
            {
                Some(p) => pool[(p + 1) % pool.len()],
                None => pool[0],
            };
            self.play_index(next_idx);
        }
    }

    fn prev(&mut self) {
        // Ala Spotify: bila lagu sudah berjalan > 3 detik, ulangi dari awal.
        if self.player.has_track() && self.player.position() > 3.0 {
            self.player.seek(0.0);
            self.lyric_scroll = 0.0;
            self.last_active = None;
            return;
        }
        let pool = self.playback_pool();
        if pool.is_empty() {
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
            let prev_idx = match self
                .current
                .and_then(|c| pool.iter().position(|&i| i == c))
            {
                Some(0) | None => *pool.last().unwrap(),
                Some(p) => pool[p - 1],
            };
            self.play_index(prev_idx);
        }
    }

    /// Dipanggil saat lagu selesai: hormati mode shuffle, repeat, dan playlist aktif.
    fn advance(&mut self) {
        let pool = self.playback_pool();
        if pool.is_empty() {
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
                if let Some(&idx) = self.shuffle_order.first() {
                    self.play_index(idx);
                }
            } else {
                self.player.pause();
            }
        } else {
            match self
                .current
                .and_then(|c| pool.iter().position(|&i| i == c))
            {
                Some(p) if p + 1 < pool.len() => self.play_index(pool[p + 1]),
                Some(_) if self.repeat == RepeatMode::All => self.play_index(pool[0]),
                None => self.play_index(pool[0]),
                _ => self.player.pause(),
            }
        }
    }

    /// Pastikan urutan acak sinkron dengan pool pemutaran saat ini.
    fn ensure_shuffle_order(&mut self) {
        let pool = self.playback_pool();
        let pool_set: HashSet<usize> = pool.iter().copied().collect();
        let valid = self.shuffle_order.len() == pool.len()
            && self.shuffle_order.iter().all(|i| pool_set.contains(i));
        if !valid {
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

    /// Urutan acak baru (Fisher-Yates + xorshift) dari pool pemutaran saat ini.
    fn new_shuffle_order(&mut self) {
        let mut order = self.playback_pool();
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
        // Letakkan lagu saat ini di awal urutan agar tidak langsung terulang.
        if let Some(cur) = self.current {
            if let Some(p) = order.iter().position(|&x| x == cur) {
                order.swap(0, p);
            }
        }
        self.shuffle_order = order;
        self.shuffle_pos = 0;
    }

    fn set_volume_and_save(&mut self, v: f32) {
        self.volume = v.clamp(0.0, 1.0);
        if self.volume > 0.001 {
            self.muted = false;
        }
        self.player.set_volume(self.volume);
        self.save_settings();
    }

    fn toggle_mute(&mut self) {
        self.muted = !self.muted;
        if !self.muted && self.volume <= 0.001 {
            self.volume = 0.5;
            self.player.set_volume(self.volume);
        }
        self.player.set_muted(self.muted);
        self.save_settings();
    }

    fn toggle_shuffle(&mut self) {
        self.shuffle = !self.shuffle;
        if self.shuffle {
            self.new_shuffle_order();
        }
        self.save_settings();
    }

    fn cycle_repeat(&mut self) {
        self.repeat = self.repeat.cycle();
        self.save_settings();
    }

    fn set_view(&mut self, view: View) {
        if self.view != view {
            self.view = view;
            self.save_settings();
        }
    }

    fn set_sort(&mut self, key: SortKey) {
        if self.sort_key == key {
            self.sort_asc = !self.sort_asc;
        } else {
            self.sort_key = key;
            self.sort_asc = true;
        }
        self.save_settings();
    }

    fn delete_playlist(&mut self, idx: usize) {
        if idx >= self.playlists.len() {
            return;
        }
        self.playlists.remove(idx);
        playlist::save(&self.playlists);
        self.active_playlist = match self.active_playlist {
            Some(a) if a == idx => None,
            Some(a) if a > idx => Some(a - 1),
            other => other,
        };
        self.playing_playlist = match self.playing_playlist {
            Some(p) if p == idx => None,
            Some(p) if p > idx => Some(p - 1),
            other => other,
        };
    }

    /// Masuk/keluar mode mini player: jendela kecil borderless transparan
    /// always-on-top.
    fn set_mini(&mut self, ctx: &egui::Context, mini: bool) {
        self.mini_mode = mini;
        if mini {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Transparent(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                egui::WindowLevel::AlwaysOnTop,
            ));
            ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(vec2(170.0, 420.0)));
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(vec2(210.0, 500.0)));
        } else {
            ctx.send_viewport_cmd(egui::ViewportCommand::Transparent(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                egui::WindowLevel::Normal,
            ));
            ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(vec2(760.0, 500.0)));
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(vec2(1140.0, 720.0)));
        }
    }

    /// Pintasan keyboard global.
    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        // Ctrl+F: fokus kolom pencarian (berlaku kapan saja).
        let ctrl_f = ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::F));
        if ctrl_f {
            if self.mini_mode {
                self.set_mini(ctx, false);
            }
            self.set_view(View::Playlist);
            self.focus_search = true;
            return;
        }

        // Escape: tutup popup / bersihkan pencarian.
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            if self.show_playlist_input {
                self.show_playlist_input = false;
                self.new_playlist_name.clear();
            } else if self.show_eq {
                self.show_eq = false;
            } else if self.show_shortcuts {
                self.show_shortcuts = false;
            } else if !self.search.is_empty() {
                self.search.clear();
            }
            return;
        }

        if ctx.egui_wants_keyboard_input() {
            return;
        }

        let (
            space,
            key_k,
            key_m,
            key_s,
            key_r,
            key_l,
            key_e,
            key_n,
            key_p,
            key_f1,
            ctrl_left,
            ctrl_right,
            shift_left,
            shift_right,
            left,
            right,
            up,
            down,
        ) = ctx.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, egui::Key::Space),
                i.consume_key(egui::Modifiers::NONE, egui::Key::K),
                i.consume_key(egui::Modifiers::NONE, egui::Key::M),
                i.consume_key(egui::Modifiers::NONE, egui::Key::S),
                i.consume_key(egui::Modifiers::NONE, egui::Key::R),
                i.consume_key(egui::Modifiers::NONE, egui::Key::L),
                i.consume_key(egui::Modifiers::NONE, egui::Key::E),
                i.consume_key(egui::Modifiers::NONE, egui::Key::N),
                i.consume_key(egui::Modifiers::NONE, egui::Key::P),
                i.consume_key(egui::Modifiers::NONE, egui::Key::F1),
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::ArrowLeft),
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::ArrowRight),
                i.consume_key(egui::Modifiers::SHIFT, egui::Key::ArrowLeft),
                i.consume_key(egui::Modifiers::SHIFT, egui::Key::ArrowRight),
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft),
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight),
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
            )
        });

        if space || key_k {
            self.toggle_play();
        }
        if ctrl_left || key_p {
            self.prev();
        }
        if ctrl_right || key_n {
            self.next();
        }
        if shift_left {
            self.player.seek(self.player.position() - 15.0);
        } else if left {
            self.player.seek(self.player.position() - 5.0);
        }
        if shift_right {
            self.player.seek(self.player.position() + 15.0);
        } else if right {
            self.player.seek(self.player.position() + 5.0);
        }
        if up {
            self.set_volume_and_save(self.volume + 0.05);
        }
        if down {
            self.set_volume_and_save(self.volume - 0.05);
        }
        if key_m {
            self.toggle_mute();
        }
        if key_s {
            self.toggle_shuffle();
        }
        if key_r {
            self.cycle_repeat();
        }
        if key_l && !self.mini_mode {
            let next_v = if self.view == View::Lyrics {
                View::Playlist
            } else {
                View::Lyrics
            };
            self.set_view(next_v);
        }
        if key_e && !self.mini_mode {
            self.show_eq = !self.show_eq;
        }
        if key_f1 && !self.mini_mode {
            self.show_shortcuts = !self.show_shortcuts;
        }
    }

    // -----------------------------------------------------------------------
    // Mini Player
    // -----------------------------------------------------------------------

    fn show_mini(&mut self, ui: &mut egui::Ui) {
        let bg = ui.max_rect();
        let accent = self.cover_accent.unwrap_or(theme::ELEVATED);
        let top_col = theme::lerp_color(theme::darken(accent, 0.45), theme::BLACK, 0.25);
        let top_col = Color32::from_rgba_unmultiplied(top_col.r(), top_col.g(), top_col.b(), 238);
        let bot_col = Color32::from_rgba_unmultiplied(12, 12, 14, 244);
        theme::gradient_rect(ui.painter(), bg, top_col, bot_col, 16.0, 16.0);
        ui.painter().rect_stroke(
            bg,
            16.0,
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 24)),
            egui::StrokeKind::Inside,
        );

        let pad = 14.0;
        let content_r = bg.shrink(pad);
        let mut cui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(content_r)
                .layout(egui::Layout::top_down(egui::Align::Center)),
        );

        let w = content_r.width();
        let bs = (w / 240.0).clamp(0.75, 1.05);
        let ts = (w / 240.0).clamp(0.75, 1.05);
        let cover_sz = (w * 0.72).clamp(90.0, 210.0);

        // Baris atas kecil: drag bar + expand + close
        cui.horizontal(|ui| {
            let drag_w = (w - 58.0).max(20.0);
            let (drag_r, drag_resp) =
                ui.allocate_exact_size(vec2(drag_w, 20.0), Sense::click_and_drag());
            // Indikator grip kecil
            let grip = Rect::from_center_size(pos2(drag_r.center().x, drag_r.center().y), vec2(28.0, 3.0));
            ui.painter()
                .rect_filled(grip, 1.5, Color32::from_rgba_unmultiplied(255, 255, 255, 55));
            if drag_resp.drag_started() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
            if drag_resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if theme::icon_button(ui, Icon::Close, 22.0, theme::GRAY)
                    .on_hover_text("Tutup Joni Music")
                    .clicked()
                {
                    self.save_settings();
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
                if theme::icon_button(ui, Icon::Expand, 22.0, theme::GRAY)
                    .on_hover_text("Kembali ke tampilan penuh")
                    .clicked()
                {
                    let ctx = ui.ctx().clone();
                    self.set_mini(&ctx, false);
                }
            });
        });

        cui.add_space(6.0);

        // Cover art (area drag)
        let (cover_rect, cover_resp) =
            cui.allocate_exact_size(vec2(cover_sz, cover_sz), Sense::click_and_drag());
        self.paint_cover(cui.painter(), cover_rect, 10.0, None);
        if cover_resp.drag_started() {
            cui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        if cover_resp.hovered() {
            cui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }

        cui.add_space(12.0);

        // Judul + artis (terpotong rapi, area drag)
        let title_size = 16.0 * ts;
        let artist_size = 12.5 * ts;
        let (text_rect, text_resp) = cui.allocate_exact_size(
            vec2(w, title_size + artist_size + 10.0),
            Sense::click_and_drag(),
        );
        {
            let p = cui.painter();
            if let Some(t) = self.current.and_then(|i| self.tracks.get(i)) {
                theme::text_trunc(
                    p,
                    pos2(text_rect.center().x, text_rect.min.y),
                    Align2::CENTER_TOP,
                    &t.title,
                    theme::bold(title_size),
                    theme::WHITE,
                    w,
                );
                let artist = if t.artist.is_empty() {
                    "Unknown Artist"
                } else {
                    &t.artist
                };
                theme::text_trunc(
                    p,
                    pos2(text_rect.center().x, text_rect.min.y + title_size + 4.0),
                    Align2::CENTER_TOP,
                    artist,
                    theme::regular(artist_size),
                    theme::GRAY,
                    w,
                );
            } else {
                theme::text_trunc(
                    p,
                    pos2(text_rect.center().x, text_rect.min.y + 4.0),
                    Align2::CENTER_TOP,
                    "Joni Music",
                    theme::bold(title_size),
                    theme::WHITE,
                    w,
                );
            }
        }
        if text_resp.drag_started() {
            cui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        if text_resp.hovered() {
            cui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }

        cui.add_space(6.0);

        // Baris lirik aktif
        let pos = self.seek_preview.unwrap_or_else(|| self.player.position());
        let lyric_text = if let LyricState::Loaded { lyrics, .. } = &self.lyric_state {
            if lyrics.synced {
                lyrics
                    .active_index(pos)
                    .and_then(|i| lyrics.lines.get(i))
                    .map(|l| {
                        if l.text.is_empty() {
                            "♪".to_string()
                        } else {
                            l.text.clone()
                        }
                    })
            } else {
                None
            }
        } else {
            None
        };
        let (lr, _) = cui.allocate_exact_size(vec2(w, 22.0), Sense::hover());
        if let Some(lt) = lyric_text {
            theme::text_trunc(
                cui.painter(),
                lr.center(),
                Align2::CENTER_CENTER,
                &lt,
                theme::bold(13.0 * ts),
                theme::GREEN,
                w,
            );
        }

        cui.add_space(6.0);

        // Progress bar tipis seekable
        let dur = self.player.duration().max(0.01);
        let frac = (pos / dur).clamp(0.0, 1.0) as f32;
        let (bar_r, _) = cui.allocate_exact_size(vec2(w, 16.0), Sense::hover());
        let (sresp, new_f) = theme::thin_slider(&mut cui, bar_r, "mini_seek", frac);
        if let Some(f) = new_f {
            let target = f as f64 * dur;
            if sresp.dragged() || sresp.is_pointer_button_down_on() {
                self.seek_preview = Some(target);
            }
            if sresp.drag_stopped() || sresp.clicked() {
                self.player.seek(target);
                self.seek_preview = None;
            }
        } else if !sresp.dragged() && !sresp.is_pointer_button_down_on() {
            self.seek_preview = None;
        }

        cui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(fmt_time(pos))
                    .size(10.5)
                    .color(theme::MUTED),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(fmt_time(self.player.duration()))
                        .size(10.5)
                        .color(theme::MUTED),
                );
            });
        });

        cui.add_space(8.0);

        // Kontrol utama: Prev | Play/Pause | Next
        let play_sz = 48.0 * bs;
        let pn_sz = 34.0 * bs;
        let gap = 14.0 * bs;
        let row_w = pn_sz * 2.0 + play_sz + gap * 2.0;
        let (ctrl_r, _) = cui.allocate_exact_size(vec2(row_w, play_sz), Sense::hover());
        let cy = ctrl_r.center().y;
        let prev_r = Rect::from_center_size(pos2(ctrl_r.left() + pn_sz / 2.0, cy), vec2(pn_sz, pn_sz));
        let play_r = Rect::from_center_size(ctrl_r.center(), vec2(play_sz, play_sz));
        let next_r = Rect::from_center_size(pos2(ctrl_r.right() - pn_sz / 2.0, cy), vec2(pn_sz, pn_sz));

        if theme::icon_hit(&mut cui, prev_r, "mini_prev", Icon::Prev, theme::GRAY)
            .on_hover_text("Sebelumnya")
            .clicked()
        {
            self.prev();
        }
        let play_icon = if self.player.is_playing() {
            Icon::Pause
        } else {
            Icon::Play
        };
        if theme::circle_hit(&mut cui, play_r, "mini_play", play_icon, theme::GREEN, theme::BLACK)
            .on_hover_text("Putar / Jeda")
            .clicked()
        {
            self.toggle_play();
        }
        if theme::icon_hit(&mut cui, next_r, "mini_next", Icon::Next, theme::GRAY)
            .on_hover_text("Berikutnya")
            .clicked()
        {
            self.next();
        }

        cui.add_space(10.0);

        // Volume bar inline
        cui.horizontal(|ui| {
            let eff = if self.muted { 0.0 } else { self.volume };
            if theme::icon_button(ui, Icon::Volume(eff), 24.0, theme::GRAY)
                .on_hover_text(if self.muted { "Bunyikan (M)" } else { "Bisukan (M)" })
                .clicked()
            {
                self.toggle_mute();
            }
            let sw = (ui.available_width() - 4.0).max(40.0);
            let (vr, _) = ui.allocate_exact_size(vec2(sw, 20.0), Sense::hover());
            let (_, new_v) = theme::thin_slider(ui, vr, "mini_vol", eff);
            if let Some(v) = new_v {
                self.set_volume_and_save(v);
            }
        });

        // Resize handle tepi kanan
        let rw = 8.0;
        let rrect = Rect::from_min_max(
            pos2(bg.max.x - rw, bg.min.y + 30.0),
            pos2(bg.max.x, bg.max.y - 30.0),
        );
        let rresp = ui.interact(
            rrect,
            egui::Id::new("mini_resize_w"),
            Sense::click_and_drag(),
        );
        if rresp.hovered() || rresp.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        if rresp.dragged() {
            let dx = rresp.drag_delta().x;
            if dx.abs() > f32::EPSILON {
                let (cur_w, cur_h) = ui.ctx().input(|i| {
                    i.viewport()
                        .inner_rect
                        .map(|r| (r.width(), r.height()))
                        .unwrap_or((210.0, 500.0))
                });
                let new_w = (cur_w + dx).clamp(170.0, 420.0);
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::InnerSize(vec2(new_w, cur_h)));
            }
        }
    }

    // -----------------------------------------------------------------------
    // Sidebar Kiri
    // -----------------------------------------------------------------------

    fn show_sidebar(&mut self, ui: &mut egui::Ui) {
        let r = ui.available_rect_before_wrap();
        let footer_h = 68.0;
        let content_r = Rect::from_min_max(r.min, pos2(r.max.x, (r.max.y - footer_h).max(r.min.y)));
        let footer_r = Rect::from_min_max(pos2(r.min.x, content_r.max.y), r.max);

        {
            let mut cui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(content_r)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            );

            // Brand header
            cui.horizontal(|ui| {
                let (ir, _) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::hover());
                ui.painter().circle_filled(ir.center(), 13.0, theme::GREEN);
                theme::paint_icon(
                    ui.painter(),
                    Rect::from_center_size(ir.center(), vec2(15.0, 15.0)),
                    Icon::Note,
                    theme::BLACK,
                );
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new("Joni Music")
                        .font(theme::bold(17.0))
                        .color(theme::WHITE),
                );
            });

            cui.add_space(14.0);

            // Navigasi utama
            for (icon, label, target) in [
                (Icon::Home, "Koleksi", View::Playlist),
                (Icon::Lyrics, "Lirik", View::Lyrics),
            ] {
                let active = self.view == target;
                let (rect, resp) = cui.allocate_exact_size(
                    vec2(cui.available_width(), 38.0),
                    Sense::click(),
                );
                if active {
                    cui.painter().rect_filled(rect, 8.0, theme::SELECTED);
                } else if resp.hovered() {
                    cui.painter().rect_filled(rect, 8.0, theme::HOVER);
                }
                let col = if active || resp.hovered() {
                    theme::WHITE
                } else {
                    theme::GRAY
                };
                let icon_r = Rect::from_center_size(pos2(rect.left() + 20.0, rect.center().y), vec2(18.0, 18.0));
                theme::paint_icon(cui.painter(), icon_r, icon, if active { theme::GREEN } else { col });
                cui.painter().text(
                    pos2(rect.left() + 40.0, rect.center().y),
                    Align2::LEFT_CENTER,
                    label,
                    if active { theme::bold(14.0) } else { theme::regular(14.0) },
                    col,
                );
                if resp.hovered() {
                    cui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if resp.clicked() {
                    self.set_view(target);
                }
                cui.add_space(2.0);
            }

            cui.add_space(10.0);
            cui.painter().line_segment(
                [
                    pos2(cui.cursor().left(), cui.cursor().top()),
                    pos2(cui.max_rect().right(), cui.cursor().top()),
                ],
                Stroke::new(1.0, theme::BORDER),
            );
            cui.add_space(10.0);

            // Header Playlist + tombol tambah
            cui.horizontal(|ui| {
                let (ir, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                theme::paint_icon(ui.painter(), ir, Icon::Library, theme::GRAY);
                ui.label(
                    egui::RichText::new("PLAYLIST")
                        .font(theme::bold(11.5))
                        .color(theme::GRAY),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if theme::icon_button(ui, Icon::Plus, 24.0, theme::GRAY)
                        .on_hover_text("Buat playlist baru")
                        .clicked()
                    {
                        self.show_playlist_input = !self.show_playlist_input;
                        if self.show_playlist_input {
                            self.focus_playlist_input = true;
                        }
                    }
                });
            });

            if self.show_playlist_input {
                cui.add_space(6.0);
                cui.horizontal(|ui| {
                    let input_w = (ui.available_width() - 56.0).max(80.0);
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut self.new_playlist_name)
                            .hint_text("Nama playlist…")
                            .desired_width(input_w),
                    );
                    if self.focus_playlist_input {
                        resp.request_focus();
                        self.focus_playlist_input = false;
                    }
                    let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    let clicked = ui.button("Buat").clicked();
                    if (enter || clicked) && !self.new_playlist_name.trim().is_empty() {
                        let name = self.new_playlist_name.trim().to_string();
                        self.playlists.push(playlist::Playlist::new(&name));
                        playlist::save(&self.playlists);
                        self.active_playlist = Some(self.playlists.len() - 1);
                        self.set_view(View::Playlist);
                        self.new_playlist_name.clear();
                        self.show_playlist_input = false;
                    }
                });
            }

            cui.add_space(8.0);

            // Daftar "Semua Lagu" + Playlist pengguna
            egui::ScrollArea::vertical()
                .id_salt("sidebar_playlists")
                .auto_shrink([false, false])
                .show(&mut cui, |ui| {
                    let mut select_pl: Option<Option<usize>> = None;
                    let mut delete_pl: Option<usize> = None;

                    // Item "Semua Lagu"
                    if self.sidebar_playlist_row(
                        ui,
                        "Semua Lagu",
                        &format!("{} lagu", self.tracks.len()),
                        theme::GREEN,
                        Icon::Library,
                        self.active_playlist.is_none(),
                        self.playing_playlist.is_none() && self.player.is_playing(),
                        false,
                    ).0 {
                        select_pl = Some(None);
                    }

                    // Item tiap playlist
                    for i in 0..self.playlists.len() {
                        let name = self.playlists[i].name.clone();
                        let count = self.playlists[i].paths.len();
                        let is_active = self.active_playlist == Some(i);
                        let is_playing_this =
                            self.playing_playlist == Some(i) && self.player.is_playing();
                        let (clicked, del) = self.sidebar_playlist_row(
                            ui,
                            &name,
                            &format!("{count} lagu"),
                            theme::color_for(&name),
                            Icon::Note,
                            is_active,
                            is_playing_this,
                            true,
                        );
                        if clicked {
                            select_pl = Some(Some(i));
                        }
                        if del {
                            delete_pl = Some(i);
                        }
                    }

                    // PERBAIKAN BUG: mengganti playlist TIDAK menghentikan lagu yang sedang diputar!
                    if let Some(sel) = select_pl {
                        self.active_playlist = sel;
                        self.set_view(View::Playlist);
                    }
                    if let Some(i) = delete_pl {
                        self.delete_playlist(i);
                    }
                });
        }

        // Garis pemisah + footer sidebar
        ui.painter().line_segment(
            [footer_r.left_top(), footer_r.right_top()],
            Stroke::new(1.0, theme::BORDER),
        );
        {
            let mut fui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(footer_r)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            );
            fui.add_space(8.0);
            fui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                if theme::icon_button(ui, Icon::Folder, 28.0, theme::GRAY)
                    .on_hover_text("Pilih Folder Musik")
                    .clicked()
                {
                    if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                        self.start_scan(dir, true);
                    }
                }
                if theme::icon_button(ui, Icon::Refresh, 28.0, theme::GRAY)
                    .on_hover_text("Pindai Ulang Folder")
                    .clicked()
                {
                    if let Some(dir) = self.music_dir.clone() {
                        self.start_scan(dir, false);
                    }
                }
                let set_resp = theme::icon_button(ui, Icon::Gear, 28.0, theme::GRAY)
                    .on_hover_text("Pengaturan");
                egui::Popup::from_toggle_button_response(&set_resp)
                    .align(egui::RectAlign::TOP_START)
                    .gap(8.0)
                    .show(|ui| {
                        ui.set_min_width(230.0);
                        ui.label(
                            egui::RichText::new("Pengaturan")
                                .font(theme::bold(13.5))
                                .color(theme::WHITE),
                        );
                        ui.add_space(4.0);
                        let mut am = self.auto_mini;
                        if ui
                            .checkbox(&mut am, "Otomatis mini player saat minimize")
                            .changed()
                        {
                            self.auto_mini = am;
                            self.save_settings();
                        }
                        ui.add_space(4.0);
                        ui.separator();
                        if ui.button("Pintasan Keyboard (F1)").clicked() {
                            self.show_shortcuts = true;
                        }
                    });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                            .size(11.0)
                            .color(theme::MUTED),
                    );
                });
            });
            fui.add_space(4.0);
            let (sr, _) = fui.allocate_exact_size(vec2(fui.available_width(), 16.0), Sense::hover());
            theme::text_trunc(
                fui.painter(),
                pos2(sr.left(), sr.center().y),
                Align2::LEFT_CENTER,
                &self.status_msg,
                theme::regular(11.0),
                theme::MUTED,
                sr.width(),
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn sidebar_playlist_row(
        &self,
        ui: &mut egui::Ui,
        title: &str,
        subtitle: &str,
        tile_color: Color32,
        tile_icon: Icon,
        active: bool,
        playing_here: bool,
        deletable: bool,
    ) -> (bool, bool) {
        let w = ui.available_width();
        let row_h = 50.0;
        let (rect, resp) = ui.allocate_exact_size(vec2(w, row_h), Sense::click());
        let hovered = resp.hovered();
        if active {
            ui.painter().rect_filled(rect, 8.0, theme::SELECTED);
        } else if hovered {
            ui.painter().rect_filled(rect, 8.0, theme::HOVER);
        }

        // Tile persegi berwarna
        let tile_r = Rect::from_min_size(pos2(rect.left() + 8.0, rect.center().y - 18.0), vec2(36.0, 36.0));
        ui.painter().rect_filled(tile_r, 6.0, theme::darken(tile_color, 0.85));
        theme::paint_icon(
            ui.painter(),
            Rect::from_center_size(tile_r.center(), vec2(17.0, 17.0)),
            tile_icon,
            theme::WHITE,
        );

        let right_pad = if deletable && (hovered || active) { 30.0 } else { 10.0 };
        let text_x = tile_r.right() + 10.0;
        let text_w = (rect.right() - text_x - right_pad).max(30.0);

        theme::text_trunc(
            ui.painter(),
            pos2(text_x, rect.center().y - 9.0),
            Align2::LEFT_CENTER,
            title,
            if active { theme::bold(13.5) } else { theme::regular(13.5) },
            if playing_here { theme::GREEN } else { theme::WHITE },
            text_w,
        );
        theme::text_trunc(
            ui.painter(),
            pos2(text_x, rect.center().y + 9.0),
            Align2::LEFT_CENTER,
            subtitle,
            theme::regular(11.5),
            theme::GRAY,
            text_w,
        );

        let mut deleted = false;
        if deletable && (hovered || active) {
            let del_r = Rect::from_center_size(pos2(rect.right() - 16.0, rect.center().y), vec2(20.0, 20.0));
            if theme::icon_hit(ui, del_r, ("del_pl", title), Icon::Close, theme::MUTED)
                .on_hover_text("Hapus playlist")
                .clicked()
            {
                deleted = true;
            }
        }
        if deletable {
            resp.context_menu(|ui| {
                if ui.button("Hapus playlist").clicked() {
                    deleted = true;
                    ui.close();
                }
            });
        }
        if hovered {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        (resp.clicked() && !deleted, deleted)
    }

    // -----------------------------------------------------------------------
    // Halaman Playlist / Koleksi (Konten Utama)
    // -----------------------------------------------------------------------

    fn show_playlist_page(&mut self, ui: &mut egui::Ui) {
        if self.tracks.is_empty() {
            let rect = ui.available_rect_before_wrap();
            theme::gradient_rect(
                ui.painter(),
                rect,
                theme::darken(theme::GREEN, 0.16),
                theme::BG,
                0.0,
                0.0,
            );
            ui.centered_and_justified(|ui| {
                ui.vertical_centered(|ui| {
                    let (ir, _) = ui.allocate_exact_size(vec2(64.0, 64.0), Sense::hover());
                    ui.painter().circle_filled(ir.center(), 32.0, theme::ELEVATED);
                    theme::paint_icon(
                        ui.painter(),
                        Rect::from_center_size(ir.center(), vec2(30.0, 30.0)),
                        Icon::Note,
                        theme::GREEN,
                    );
                    ui.add_space(16.0);
                    ui.label(
                        egui::RichText::new("Belum ada lagu di koleksi")
                            .font(theme::bold(22.0))
                            .color(theme::WHITE),
                    );
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new("Pilih folder yang berisi file MP3, FLAC, WAV, OGG, atau M4A.")
                            .size(14.0)
                            .color(theme::GRAY),
                    );
                    ui.add_space(18.0);
                    if theme::pill_button(ui, "Pilih Folder Musik").clicked() {
                        if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                            self.start_scan(dir, true);
                        }
                    }
                });
            });
            return;
        }

        let indices = self.filtered_indices();
        let total_secs: f64 = indices.iter().map(|&i| self.tracks[i].duration_secs).sum();
        let (header_kind, header_title, accent_col) = match self.active_playlist {
            Some(pi) if pi < self.playlists.len() => {
                let name = self.playlists[pi].name.clone();
                let col = theme::color_for(&name);
                ("PLAYLIST", name, col)
            }
            _ => (
                "KOLEKSI MUSIK",
                String::from("Semua Lagu"),
                self.cover_accent.unwrap_or(Color32::from_rgb(0x1e, 0x6b, 0x45)),
            ),
        };

        // Header dengan gradien Spotify
        let avail_r = ui.available_rect_before_wrap();
        let header_h = 152.0;
        let header_r = Rect::from_min_size(avail_r.min, vec2(avail_r.width(), header_h + 64.0));
        theme::gradient_rect(
            ui.painter(),
            header_r,
            theme::darken(accent_col, 0.55),
            theme::BG,
            0.0,
            0.0,
        );

        ui.add_space(18.0);
        ui.horizontal(|ui| {
            ui.add_space(24.0);
            // Artwork header 104x104
            let (art_r, _) = ui.allocate_exact_size(vec2(104.0, 104.0), Sense::hover());
            if self.active_playlist.is_none() && self.cover_texture.is_some() {
                self.paint_cover(ui.painter(), art_r, 8.0, None);
            } else {
                ui.painter().rect_filled(art_r, 8.0, theme::darken(accent_col, 0.85));
                theme::paint_icon(
                    ui.painter(),
                    Rect::from_center_size(art_r.center(), vec2(44.0, 44.0)),
                    if self.active_playlist.is_some() {
                        Icon::Note
                    } else {
                        Icon::Library
                    },
                    theme::WHITE,
                );
            }
            ui.add_space(18.0);
            ui.vertical(|ui| {
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(header_kind)
                        .font(theme::bold(11.0))
                        .color(theme::GRAY),
                );
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new(&header_title)
                        .font(theme::bold(32.0))
                        .color(theme::WHITE),
                );
                ui.add_space(6.0);
                let meta = format!("{} lagu  •  {}", indices.len(), fmt_duration_long(total_secs));
                ui.label(egui::RichText::new(meta).size(13.0).color(theme::GRAY));
            });
        });

        ui.add_space(16.0);

        // Action bar: Play besar + Shuffle + Search + Scope + Sort
        ui.horizontal(|ui| {
            ui.add_space(24.0);
            // Tombol play lingkaran hijau 48px
            let (play_r, _) = ui.allocate_exact_size(vec2(48.0, 48.0), Sense::hover());
            let playing_this_view =
                self.player.is_playing() && self.playing_playlist == self.active_playlist;
            let p_icon = if playing_this_view {
                Icon::Pause
            } else {
                Icon::Play
            };
            if theme::circle_hit(ui, play_r, "header_play", p_icon, theme::GREEN, theme::BLACK)
                .on_hover_text(if playing_this_view { "Jeda" } else { "Putar koleksi ini" })
                .clicked()
            {
                if self.playing_playlist == self.active_playlist && self.player.has_track() {
                    self.player.toggle();
                } else if let Some(&first) = indices.first() {
                    self.play_from_view(first);
                }
            }

            ui.add_space(8.0);
            let sh_col = if self.shuffle { theme::GREEN } else { theme::GRAY };
            if theme::icon_button(ui, Icon::Shuffle, 32.0, sh_col)
                .on_hover_text("Acak (S)")
                .clicked()
            {
                self.toggle_shuffle();
            }

            // Kanan: Search + Scope + Sort
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(20.0);

                // Arah sort (Asc/Desc)
                let sort_icon = if self.sort_asc {
                    Icon::SortAsc
                } else {
                    Icon::SortDesc
                };
                if theme::icon_button(ui, sort_icon, 26.0, theme::GRAY)
                    .on_hover_text(if self.sort_asc {
                        "Urutan: Naik (A-Z)"
                    } else {
                        "Urutan: Turun (Z-A)"
                    })
                    .clicked()
                {
                    self.sort_asc = !self.sort_asc;
                    self.save_settings();
                }

                // Pilihan SortKey
                egui::ComboBox::from_id_salt("sort_key")
                    .selected_text(
                        egui::RichText::new(self.sort_key.label())
                            .size(12.5)
                            .color(theme::GRAY),
                    )
                    .width(105.0)
                    .show_ui(ui, |ui| {
                        for k in SortKey::ALL {
                            if ui
                                .selectable_label(self.sort_key == k, k.label())
                                .clicked()
                            {
                                self.sort_key = k;
                                self.save_settings();
                            }
                        }
                    });

                ui.add_space(6.0);

                // Pilihan cakupan pencarian (Semua / Judul / Artis / Album)
                egui::ComboBox::from_id_salt("search_scope")
                    .selected_text(
                        egui::RichText::new(self.search_scope.label())
                            .size(12.5)
                            .color(theme::GRAY),
                    )
                    .width(78.0)
                    .show_ui(ui, |ui| {
                        for sc in SearchScope::ALL {
                            if ui
                                .selectable_label(self.search_scope == sc, sc.label())
                                .clicked()
                            {
                                self.search_scope = sc;
                                self.save_settings();
                            }
                        }
                    });

                // Tombol bersihkan search
                if !self.search.is_empty()
                    && theme::icon_button(ui, Icon::Close, 22.0, theme::GRAY)
                        .on_hover_text("Bersihkan pencarian (Esc)")
                        .clicked()
                {
                    self.search.clear();
                }

                // Kotak pencarian + ikon search
                let search_w = (ui.available_width() - 48.0).clamp(110.0, 220.0);
                let sresp = ui.add(
                    egui::TextEdit::singleline(&mut self.search)
                        .hint_text("Cari lagu… (Ctrl+F)")
                        .desired_width(search_w),
                );
                let (sir, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                theme::paint_icon(ui.painter(), sir, Icon::Search, theme::MUTED);
                if self.focus_search {
                    sresp.request_focus();
                    self.focus_search = false;
                }
            });
        });

        ui.add_space(10.0);

        // Tabel lagu (dengan header kolom sortable + virtualized ScrollArea::show_rows)
        self.show_track_table(ui, &indices);
    }

    fn show_track_table(&mut self, ui: &mut egui::Ui, indices: &[usize]) {
        let pad_x = 24.0;
        let avail_w = (ui.available_width() - pad_x * 2.0).max(240.0);
        let show_album = avail_w >= 460.0;
        let num_w = 38.0;
        let dur_w = 56.0;
        let album_w = if show_album { (avail_w * 0.28).clamp(120.0, 240.0) } else { 0.0 };
        let title_w = (avail_w - num_w - dur_w - album_w - 24.0).max(120.0);

        // Header kolom
        ui.horizontal(|ui| {
            ui.add_space(pad_x);
            let (hr, _) = ui.allocate_exact_size(vec2(avail_w, 26.0), Sense::hover());
            let cy = hr.center().y;

            ui.painter().text(
                pos2(hr.left() + 14.0, cy),
                Align2::CENTER_CENTER,
                "#",
                theme::bold(11.5),
                theme::MUTED,
            );

            // Header Judul (clickable untuk sort)
            let tx = hr.left() + num_w + 6.0;
            let title_hr = Rect::from_min_size(pos2(tx, hr.top()), vec2(title_w, hr.height()));
            if self.sortable_col_header(ui, title_hr, "JUDUL", SortKey::Title, Align2::LEFT_CENTER)
            {
                self.set_sort(SortKey::Title);
            }

            if show_album {
                let ax = tx + title_w + 10.0;
                let album_hr = Rect::from_min_size(pos2(ax, hr.top()), vec2(album_w, hr.height()));
                if self.sortable_col_header(
                    ui,
                    album_hr,
                    "ALBUM",
                    SortKey::Album,
                    Align2::LEFT_CENTER,
                ) {
                    self.set_sort(SortKey::Album);
                }
            }

            // Header Durasi (ikon jam, clickable untuk sort)
            let dur_hr = Rect::from_min_size(
                pos2(hr.right() - dur_w, hr.top()),
                vec2(dur_w, hr.height()),
            );
            let dresp = ui.interact(dur_hr, ui.id().with("col_dur"), Sense::click());
            let dcol = if self.sort_key == SortKey::Duration {
                theme::GREEN
            } else if dresp.hovered() {
                theme::WHITE
            } else {
                theme::MUTED
            };
            theme::paint_icon(
                ui.painter(),
                Rect::from_center_size(pos2(dur_hr.right() - 16.0, cy), vec2(14.0, 14.0)),
                Icon::Clock,
                dcol,
            );
            if self.sort_key == SortKey::Duration {
                theme::paint_icon(
                    ui.painter(),
                    Rect::from_center_size(pos2(dur_hr.right() - 34.0, cy), vec2(10.0, 10.0)),
                    if self.sort_asc {
                        Icon::SortAsc
                    } else {
                        Icon::SortDesc
                    },
                    theme::GREEN,
                );
            }
            if dresp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if dresp.clicked() {
                self.set_sort(SortKey::Duration);
            }
        });

        // Garis bawah header
        let line_y = ui.cursor().top();
        ui.painter().line_segment(
            [
                pos2(ui.max_rect().left() + pad_x, line_y),
                pos2(ui.max_rect().right() - pad_x, line_y),
            ],
            Stroke::new(1.0, theme::BORDER),
        );
        ui.add_space(6.0);

        if indices.is_empty() {
            ui.add_space(36.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    egui::RichText::new("Tidak ada lagu yang cocok.")
                        .size(15.0)
                        .color(theme::GRAY),
                );
            });
            return;
        }

        let row_h = 52.0;
        let t_now = ui.ctx().input(|i| i.time);
        let playing = self.player.is_playing();
        let mut clicked_idx: Option<usize> = None;
        let mut add_action: Option<(usize, usize)> = None;
        let mut remove_action: Option<usize> = None;

        egui::ScrollArea::vertical()
            .id_salt("main_track_table")
            .auto_shrink([false, false])
            .show_rows(ui, row_h, indices.len(), |ui, row_range| {
                for num in row_range {
                    let idx = indices[num];
                    let is_current = self.current == Some(idx);
                    let track = &self.tracks[idx];
                    let title = track.title.clone();
                    let artist = if track.artist.is_empty() {
                        String::from("Unknown Artist")
                    } else {
                        track.artist.clone()
                    };
                    let album = track.album.clone();
                    let dur_s = track.duration_secs;

                    ui.horizontal(|ui| {
                        ui.add_space(pad_x);
                        let (rect, resp) =
                            ui.allocate_exact_size(vec2(avail_w, row_h), Sense::click());
                        let hovered = resp.hovered();
                        if hovered {
                            ui.painter().rect_filled(rect, 6.0, theme::ROW_HOVER);
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        } else if is_current {
                            ui.painter().rect_filled(
                                rect,
                                6.0,
                                Color32::from_rgba_unmultiplied(30, 215, 96, 16),
                            );
                        }
                        if resp.clicked() {
                            clicked_idx = Some(idx);
                        }

                        let cy = rect.center().y;
                        let num_c = pos2(rect.left() + 18.0, cy);
                        if hovered {
                            let ic = if is_current && playing {
                                Icon::Pause
                            } else {
                                Icon::Play
                            };
                            theme::paint_icon(
                                ui.painter(),
                                Rect::from_center_size(num_c, vec2(13.0, 13.0)),
                                ic,
                                if is_current { theme::GREEN } else { theme::WHITE },
                            );
                        } else if is_current {
                            if playing {
                                theme::eq_bars(ui.painter(), num_c, t_now, theme::GREEN);
                            } else {
                                theme::paint_icon(
                                    ui.painter(),
                                    Rect::from_center_size(num_c, vec2(12.0, 12.0)),
                                    Icon::Play,
                                    theme::GREEN,
                                );
                            }
                        } else {
                            ui.painter().text(
                                num_c,
                                Align2::CENTER_CENTER,
                                format!("{}", num + 1),
                                theme::regular(13.0),
                                theme::MUTED,
                            );
                        }

                        // Judul + artis
                        let tx = rect.left() + num_w + 6.0;
                        theme::text_trunc(
                            ui.painter(),
                            pos2(tx, cy - 9.0),
                            Align2::LEFT_CENTER,
                            &title,
                            theme::bold(14.0),
                            if is_current { theme::GREEN } else { theme::WHITE },
                            title_w,
                        );
                        theme::text_trunc(
                            ui.painter(),
                            pos2(tx, cy + 10.0),
                            Align2::LEFT_CENTER,
                            &artist,
                            theme::regular(12.0),
                            if hovered { theme::WHITE } else { theme::GRAY },
                            title_w,
                        );

                        // Album
                        if show_album {
                            let ax = tx + title_w + 10.0;
                            theme::text_trunc(
                                ui.painter(),
                                pos2(ax, cy),
                                Align2::LEFT_CENTER,
                                &album,
                                theme::regular(12.5),
                                if hovered { theme::WHITE } else { theme::GRAY },
                                album_w,
                            );
                        }

                        // Durasi
                        ui.painter().text(
                            pos2(rect.right() - 10.0, cy),
                            Align2::RIGHT_CENTER,
                            fmt_time(dur_s),
                            theme::regular(12.5),
                            theme::GRAY,
                        );

                        // Context menu (klik kanan)
                        resp.context_menu(|ui| {
                            if ui.button("Putar sekarang").clicked() {
                                clicked_idx = Some(idx);
                                ui.close();
                            }
                            if !self.playlists.is_empty() {
                                ui.separator();
                                ui.label(
                                    egui::RichText::new("Tambah ke playlist:")
                                        .size(11.5)
                                        .color(theme::MUTED),
                                );
                                for (pi, pl) in self.playlists.iter().enumerate() {
                                    if ui.button(&pl.name).clicked() {
                                        add_action = Some((pi, idx));
                                        ui.close();
                                    }
                                }
                            }
                            if self.active_playlist.is_some() {
                                ui.separator();
                                if ui.button("Hapus dari playlist ini").clicked() {
                                    remove_action = Some(idx);
                                    ui.close();
                                }
                            }
                        });
                    });
                }
            });

        if let Some(i) = clicked_idx {
            if self.current == Some(i) && self.player.has_track() {
                self.player.toggle();
            } else {
                self.play_from_view(i);
            }
        }
        if let Some((pi, idx)) = add_action {
            let path = self.tracks[idx].path.clone();
            if self.playlists[pi].add(&path) {
                playlist::save(&self.playlists);
                self.status_msg = format!("Ditambahkan ke playlist '{}'", self.playlists[pi].name);
            }
        }
        if let Some(idx) = remove_action {
            if let Some(pi) = self.active_playlist {
                let path = self.tracks[idx].path.clone();
                self.playlists[pi].remove(&path);
                playlist::save(&self.playlists);
            }
        }
    }

    fn sortable_col_header(
        &self,
        ui: &mut egui::Ui,
        rect: Rect,
        label: &str,
        key: SortKey,
        align: Align2,
    ) -> bool {
        let resp = ui.interact(rect, ui.id().with(("col", label)), Sense::click());
        let active = self.sort_key == key;
        let col = if active {
            theme::GREEN
        } else if resp.hovered() {
            theme::WHITE
        } else {
            theme::MUTED
        };
        let g = ui
            .painter()
            .layout_no_wrap(label.to_owned(), theme::bold(11.5), col);
        let text_r = align.anchor_size(pos2(rect.left(), rect.center().y), g.size());
        ui.painter().galley(text_r.min, g, col);
        if active {
            theme::paint_icon(
                ui.painter(),
                Rect::from_center_size(pos2(text_r.right() + 10.0, rect.center().y), vec2(10.0, 10.0)),
                if self.sort_asc {
                    Icon::SortAsc
                } else {
                    Icon::SortDesc
                },
                theme::GREEN,
            );
        }
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        resp.clicked()
    }

    // -----------------------------------------------------------------------
    // Halaman Lirik
    // -----------------------------------------------------------------------

    fn show_lyrics_view(&mut self, ui: &mut egui::Ui) {
        let bg = ui.available_rect_before_wrap();
        let accent = self
            .cover_accent
            .unwrap_or(Color32::from_rgb(0x1f, 0x3b, 0x52));
        theme::gradient_rect(
            ui.painter(),
            bg,
            theme::darken(accent, 0.52),
            theme::BG,
            0.0,
            0.0,
        );

        // Header lagu aktif di atas lirik
        let mut reload_lyrics = false;
        if let Some(track) = self.current.and_then(|i| self.tracks.get(i)).cloned() {
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                ui.add_space(28.0);
                let (cr, _) = ui.allocate_exact_size(vec2(48.0, 48.0), Sense::hover());
                self.paint_cover(ui.painter(), cr, 6.0, None);
                ui.add_space(12.0);
                ui.vertical(|ui| {
                    ui.add_space(2.0);
                    ui.label(
                        egui::RichText::new(&track.title)
                            .font(theme::bold(18.0))
                            .color(theme::WHITE),
                    );
                    let sub = if track.artist.is_empty() {
                        String::from("Unknown Artist")
                    } else {
                        track.artist.clone()
                    };
                    ui.label(egui::RichText::new(sub).size(13.0).color(theme::GRAY));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(24.0);
                    if theme::icon_button(ui, Icon::Refresh, 28.0, theme::GRAY)
                        .on_hover_text("Unduh ulang lirik dari internet")
                        .clicked()
                    {
                        reload_lyrics = true;
                    }
                    let badge = match &self.lyric_state {
                        LyricState::Loaded {
                            lyrics,
                            from_cache,
                        } => {
                            if lyrics.synced {
                                if *from_cache {
                                    Some("Tersinkron • Offline")
                                } else {
                                    Some("Tersinkron • Online")
                                }
                            } else {
                                Some("Teks Biasa")
                            }
                        }
                        _ => None,
                    };
                    if let Some(b) = badge {
                        ui.label(
                            egui::RichText::new(b)
                                .size(11.5)
                                .color(Color32::from_rgba_unmultiplied(255, 255, 255, 160)),
                        );
                    }
                });
            });
            ui.add_space(8.0);
        }
        if reload_lyrics {
            if let Some(t) = self.current.and_then(|i| self.tracks.get(i)).cloned() {
                self.fetch_lyrics_for(t, true);
            }
        }

        let pos = self.seek_preview.unwrap_or_else(|| self.player.position());
        let state = self.lyric_state.clone();
        let avail = ui.available_width();
        let col_w = 680.0f32.min(avail - 48.0).max(220.0);
        let pad = ((avail - col_w) / 2.0).max(0.0);

        match state {
            LyricState::Loading => {
                ui.centered_and_justified(|ui| {
                    ui.label(
                        egui::RichText::new("Mengambil lirik…")
                            .font(theme::bold(20.0))
                            .color(theme::GRAY),
                    );
                });
            }
            LyricState::NotFound => {
                ui.centered_and_justified(|ui| {
                    ui.vertical_centered(|ui| {
                        ui.label(
                            egui::RichText::new("Lirik tidak ditemukan untuk lagu ini.")
                                .font(theme::bold(20.0))
                                .color(theme::WHITE),
                        );
                        ui.add_space(6.0);
                        ui.label(
                            egui::RichText::new(
                                "Pastikan online saat pertama kali memutar lagu, atau letakkan file .lrc di folder yang sama.",
                            )
                            .size(13.0)
                            .color(theme::GRAY),
                        );
                    });
                });
            }
            LyricState::None => {
                ui.centered_and_justified(|ui| {
                    ui.label(
                        egui::RichText::new("Putar lagu untuk melihat lirik.")
                            .font(theme::bold(20.0))
                            .color(theme::GRAY),
                    );
                });
            }
            LyricState::Loaded { lyrics, .. } => {
                if !lyrics.synced {
                    egui::ScrollArea::vertical()
                        .id_salt("lyrics_plain")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.add_space(pad);
                                ui.vertical(|ui| {
                                    ui.add_space(24.0);
                                    for line in &lyrics.lines {
                                        ui.add_sized(
                                            vec2(col_w, 0.0),
                                            egui::Label::new(
                                                egui::RichText::new(&line.text)
                                                    .font(theme::regular(22.0))
                                                    .color(Color32::from_rgba_unmultiplied(
                                                        255, 255, 255, 210,
                                                    )),
                                            )
                                            .wrap_mode(egui::TextWrapMode::Wrap),
                                        );
                                        ui.add_space(12.0);
                                    }
                                    ui.add_space(80.0);
                                });
                            });
                        });
                } else {
                    let active = lyrics.active_index(pos);
                    let jump = match (self.last_active, active) {
                        (Some(a), Some(b)) => (b as isize - a as isize).unsigned_abs(),
                        _ => 0,
                    };
                    self.last_active = active;
                    let snap = jump >= 2;
                    let mut active_center_y: Option<f32> = None;
                    let bottom_pad = self.lyric_viewport_h.max(240.0);

                    let output = egui::ScrollArea::vertical()
                        .id_salt("lyrics_synced")
                        .auto_shrink([false, false])
                        .vertical_scroll_offset(self.lyric_scroll)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.add_space(pad);
                                ui.vertical(|ui| {
                                    ui.add_space(36.0);
                                    for (i, line) in lyrics.lines.iter().enumerate() {
                                        let is_active = Some(i) == active;
                                        let is_past = active.is_some_and(|a| i < a);
                                        let text = if line.text.is_empty() {
                                            "♪".to_string()
                                        } else {
                                            line.text.clone()
                                        };
                                        let (font, base_col) = if is_active {
                                            (theme::bold(30.0), theme::WHITE)
                                        } else if is_past {
                                            (
                                                theme::bold(22.0),
                                                Color32::from_rgba_unmultiplied(255, 255, 255, 130),
                                            )
                                        } else {
                                            (
                                                theme::bold(22.0),
                                                Color32::from_rgba_unmultiplied(255, 255, 255, 88),
                                            )
                                        };
                                        let resp = ui.add_sized(
                                            vec2(col_w, 0.0),
                                            egui::Label::new(
                                                egui::RichText::new(&text).font(font).color(base_col),
                                            )
                                            .sense(Sense::click())
                                            .wrap_mode(egui::TextWrapMode::Wrap),
                                        );
                                        if resp.hovered() && !is_active {
                                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                            ui.painter().rect_filled(
                                                resp.rect.expand2(vec2(8.0, 4.0)),
                                                6.0,
                                                Color32::from_rgba_unmultiplied(255, 255, 255, 16),
                                            );
                                        }
                                        if resp.clicked() {
                                            self.player.seek(line.time + 0.01);
                                        }
                                        if is_active {
                                            active_center_y = Some(resp.rect.center().y);
                                        }
                                        ui.add_space(18.0);
                                    }
                                    ui.add_space(bottom_pad);
                                });
                            });
                        });

                    if let Some(cy) = active_center_y {
                        let vh = output.inner_rect.height();
                        self.lyric_viewport_h = vh;
                        let target =
                            (cy - output.inner_rect.min.y + self.lyric_scroll - vh * 0.45)
                                .max(0.0);
                        let diff = target - self.lyric_scroll;
                        if snap || diff.abs() < 0.5 {
                            self.lyric_scroll = target;
                        } else {
                            let dt = ui.ctx().input(|i| i.stable_dt).clamp(0.001, 0.05);
                            let alpha = 1.0 - (-10.0 * dt).exp();
                            self.lyric_scroll += diff * alpha;
                        }
                    }
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Bottom Player Bar (Full-Width ala Spotify Desktop)
    // -----------------------------------------------------------------------

    fn show_player_bar(&mut self, ui: &mut egui::Ui) {
        let r = ui.max_rect();
        // Garis tipis pemisah atas
        ui.painter()
            .line_segment([r.left_top(), r.right_top()], Stroke::new(1.0, theme::BORDER));

        let pad_x = 16.0;
        let inner = Rect::from_min_max(
            pos2(r.left() + pad_x, r.top() + 8.0),
            pos2(r.right() - pad_x, r.bottom() - 8.0),
        );
        let total_w = inner.width();
        let side_w = (total_w * 0.28).clamp(180.0, 310.0);
        let center_w = (total_w - side_w * 2.0 - 24.0).max(240.0);

        let left_r = Rect::from_min_size(inner.min, vec2(side_w, inner.height()));
        let center_r = Rect::from_center_size(inner.center(), vec2(center_w, inner.height()));
        let right_r = Rect::from_min_size(
            pos2(inner.right() - side_w, inner.top()),
            vec2(side_w, inner.height()),
        );

        // ---- Kiri: Cover Art + Judul & Artis ----
        {
            let mut lui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(left_r)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            let cover_sz = 56.0;
            let (cr, cresp) = lui.allocate_exact_size(vec2(cover_sz, cover_sz), Sense::click());
            self.paint_cover(lui.painter(), cr, 6.0, None);
            if cresp.hovered() {
                lui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if cresp.clicked() && self.current.is_some() {
                let target = if self.view == View::Lyrics {
                    View::Playlist
                } else {
                    View::Lyrics
                };
                self.set_view(target);
            }

            lui.add_space(12.0);
            let tw = (left_r.width() - cover_sz - 16.0).max(40.0);
            let (tr, tresp) = lui.allocate_exact_size(vec2(tw, 42.0), Sense::click());
            if let Some(t) = self.current.and_then(|i| self.tracks.get(i)) {
                theme::text_trunc(
                    lui.painter(),
                    pos2(tr.left(), tr.center().y - 9.0),
                    Align2::LEFT_CENTER,
                    &t.title,
                    theme::bold(13.5),
                    theme::WHITE,
                    tw,
                );
                let artist = if t.artist.is_empty() {
                    "Unknown Artist"
                } else {
                    &t.artist
                };
                theme::text_trunc(
                    lui.painter(),
                    pos2(tr.left(), tr.center().y + 9.0),
                    Align2::LEFT_CENTER,
                    artist,
                    theme::regular(11.5),
                    theme::GRAY,
                    tw,
                );
                if tresp.hovered() {
                    lui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if tresp.clicked() {
                    self.set_view(View::Lyrics);
                }
            } else {
                theme::text_trunc(
                    lui.painter(),
                    pos2(tr.left(), tr.center().y),
                    Align2::LEFT_CENTER,
                    "Tidak ada lagu diputar",
                    theme::regular(12.5),
                    theme::MUTED,
                    tw,
                );
            }
        }

        // ---- Tengah: Transport + Progress Bar ----
        {
            let mut cui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(center_r)
                    .layout(egui::Layout::top_down(egui::Align::Center)),
            );

            // Baris kontrol transport
            let btn_sz = 30.0;
            let play_sz = 36.0;
            let gap = 16.0;
            let row_w = btn_sz * 4.0 + play_sz + gap * 4.0;
            let (tr, _) = cui.allocate_exact_size(vec2(row_w, 40.0), Sense::hover());
            let cy = tr.center().y - 1.0;

            let x0 = tr.left() + btn_sz / 2.0;
            let x1 = x0 + btn_sz + gap;
            let x2 = tr.center().x;
            let x4 = tr.right() - btn_sz / 2.0;
            let x3 = x4 - btn_sz - gap;

            // Shuffle
            let sh_r = Rect::from_center_size(pos2(x0, cy), vec2(btn_sz, btn_sz));
            let sh_col = if self.shuffle { theme::GREEN } else { theme::GRAY };
            if theme::icon_hit(&mut cui, sh_r, "tb_shuffle", Icon::Shuffle, sh_col)
                .on_hover_text("Acak (S)")
                .clicked()
            {
                self.toggle_shuffle();
            }
            if self.shuffle {
                cui.painter().circle_filled(pos2(x0, tr.bottom() - 2.0), 2.0, theme::GREEN);
            }

            // Prev
            let pr_r = Rect::from_center_size(pos2(x1, cy), vec2(btn_sz, btn_sz));
            if theme::icon_hit(&mut cui, pr_r, "tb_prev", Icon::Prev, theme::GRAY)
                .on_hover_text("Sebelumnya (P)")
                .clicked()
            {
                self.prev();
            }

            // Play / Pause putih bulat
            let pl_r = Rect::from_center_size(pos2(x2, cy), vec2(play_sz, play_sz));
            let play_ic = if self.player.is_playing() {
                Icon::Pause
            } else {
                Icon::Play
            };
            if theme::circle_hit(&mut cui, pl_r, "tb_play", play_ic, theme::WHITE, theme::BLACK)
                .on_hover_text("Putar / Jeda (Space)")
                .clicked()
            {
                self.toggle_play();
            }

            // Next
            let nx_r = Rect::from_center_size(pos2(x3, cy), vec2(btn_sz, btn_sz));
            if theme::icon_hit(&mut cui, nx_r, "tb_next", Icon::Next, theme::GRAY)
                .on_hover_text("Berikutnya (N)")
                .clicked()
            {
                self.next();
            }

            // Repeat
            let rp_r = Rect::from_center_size(pos2(x4, cy), vec2(btn_sz, btn_sz));
            let (rp_ic, rp_col) = match self.repeat {
                RepeatMode::Off => (Icon::Repeat, theme::GRAY),
                RepeatMode::All => (Icon::Repeat, theme::GREEN),
                RepeatMode::One => (Icon::RepeatOne, theme::GREEN),
            };
            if theme::icon_hit(&mut cui, rp_r, "tb_repeat", rp_ic, rp_col)
                .on_hover_text(format!("{} (R)", self.repeat.label()))
                .clicked()
            {
                self.cycle_repeat();
            }
            if self.repeat != RepeatMode::Off {
                cui.painter().circle_filled(pos2(x4, tr.bottom() - 2.0), 2.0, theme::GREEN);
            }

            cui.add_space(4.0);

            // Progress bar
            let dur = self.player.duration().max(0.01);
            let pos = self.seek_preview.unwrap_or_else(|| self.player.position());
            let frac = (pos / dur).clamp(0.0, 1.0) as f32;
            let (pr, _) = cui.allocate_exact_size(vec2(center_w, 20.0), Sense::hover());
            let time_w = 42.0;
            cui.painter().text(
                pos2(pr.left() + time_w - 6.0, pr.center().y),
                Align2::RIGHT_CENTER,
                fmt_time(pos),
                theme::regular(11.0),
                theme::GRAY,
            );
            cui.painter().text(
                pos2(pr.right() - time_w + 6.0, pr.center().y),
                Align2::LEFT_CENTER,
                fmt_time(self.player.duration()),
                theme::regular(11.0),
                theme::GRAY,
            );
            let slider_r = Rect::from_min_max(
                pos2(pr.left() + time_w + 4.0, pr.top()),
                pos2(pr.right() - time_w - 4.0, pr.bottom()),
            );
            let (sresp, new_f) = theme::thin_slider(&mut cui, slider_r, "main_seek", frac);
            if let Some(f) = new_f {
                let target = f as f64 * dur;
                if sresp.dragged() || sresp.is_pointer_button_down_on() {
                    self.seek_preview = Some(target);
                }
                if sresp.drag_stopped() || sresp.clicked() {
                    self.player.seek(target);
                    self.seek_preview = None;
                }
            } else if !sresp.dragged() && !sresp.is_pointer_button_down_on() {
                self.seek_preview = None;
            }
        }

        // ---- Kanan: Lirik, EQ, Perangkat Audio, Volume, Mini Player ----
        {
            let mut rui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(right_r)
                    .layout(egui::Layout::right_to_left(egui::Align::Center)),
            );
            rui.spacing_mut().item_spacing.x = 8.0;

            // Mini player
            if theme::icon_button(&mut rui, Icon::Mini, 28.0, theme::GRAY)
                .on_hover_text("Mini player")
                .clicked()
            {
                let ctx = rui.ctx().clone();
                self.set_mini(&ctx, true);
            }

            // Slider volume horizontal
            let vol_w = (right_r.width() * 0.34).clamp(64.0, 100.0);
            let eff = if self.muted { 0.0 } else { self.volume };
            let (vr, _) = rui.allocate_exact_size(vec2(vol_w, 22.0), Sense::hover());
            let (_, new_v) = theme::thin_slider(&mut rui, vr, "main_vol", eff);
            if let Some(v) = new_v {
                self.set_volume_and_save(v);
            }

            // Tombol mute/unmute
            if theme::icon_button(&mut rui, Icon::Volume(eff), 26.0, theme::GRAY)
                .on_hover_text(format!(
                    "Volume {}% (klik untuk {}, M)",
                    (eff * 100.0).round() as u32,
                    if self.muted { "bunyikan" } else { "bisukan" }
                ))
                .clicked()
            {
                self.toggle_mute();
            }

            // Pemilih perangkat output audio (popup)
            let dev_col = if !self.player.is_ready() {
                Color32::from_rgb(0xe8, 0x55, 0x55)
            } else if self.audio_device.is_some() {
                theme::GREEN
            } else {
                theme::GRAY
            };
            let dev_label = self
                .audio_device
                .as_deref()
                .unwrap_or(DEFAULT_DEVICE_LABEL);
            let dev_resp = theme::icon_button(&mut rui, Icon::Device, 28.0, dev_col)
                .on_hover_text(format!("Output audio: {dev_label}"));
            let mut picked_device: Option<Option<String>> = None;
            egui::Popup::from_toggle_button_response(&dev_resp)
                .align(egui::RectAlign::TOP_END)
                .gap(8.0)
                .show(|ui| {
                    ui.set_min_width(220.0);
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("Perangkat Output Audio")
                                .font(theme::bold(13.0))
                                .color(theme::WHITE),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if theme::icon_button(ui, Icon::Refresh, 20.0, theme::GRAY)
                                .on_hover_text("Segarkan daftar")
                                .clicked()
                            {
                                self.device_names = output_device_names();
                            }
                        });
                    });
                    ui.separator();
                    if ui
                        .selectable_label(self.audio_device.is_none(), DEFAULT_DEVICE_LABEL)
                        .clicked()
                    {
                        picked_device = Some(None);
                    }
                    for name in self.device_names.clone() {
                        let sel = self.audio_device.as_deref() == Some(name.as_str());
                        if ui.selectable_label(sel, &name).clicked() {
                            picked_device = Some(Some(name));
                        }
                    }
                });
            if dev_resp.clicked() {
                self.device_names = output_device_names();
            }
            if let Some(d) = picked_device {
                self.set_audio_device(d);
            }

            // Equalizer
            let eq_col = if self.eq_preset == "Flat" && !self.show_eq {
                theme::GRAY
            } else {
                theme::GREEN
            };
            if theme::icon_button(&mut rui, Icon::Eq, 28.0, eq_col)
                .on_hover_text(format!("Equalizer: {} (E)", self.eq_preset))
                .clicked()
            {
                self.show_eq = !self.show_eq;
            }

            // Lirik toggle
            let lyr_col = if self.view == View::Lyrics {
                theme::GREEN
            } else {
                theme::GRAY
            };
            if theme::icon_button(&mut rui, Icon::Lyrics, 28.0, lyr_col)
                .on_hover_text("Tampilkan Lirik (L)")
                .clicked()
            {
                let next_v = if self.view == View::Lyrics {
                    View::Playlist
                } else {
                    View::Lyrics
                };
                self.set_view(next_v);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Window Mengambang: Equalizer & Pintasan Keyboard
    // -----------------------------------------------------------------------

    fn show_eq_window(&mut self, ctx: &egui::Context) {
        if !self.show_eq {
            return;
        }
        let mut open = true;
        let mut changed = false;
        egui::Window::new("Equalizer 5-Band")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 14.0;
                    ui.spacing_mut().slider_width = 100.0;
                    for band in 0..5 {
                        ui.vertical_centered(|ui| {
                            ui.label(
                                egui::RichText::new(audio::EQ_NAMES[band])
                                    .font(theme::bold(11.5))
                                    .color(theme::GRAY),
                            );
                            let mut v = self.player.eq_gains_snapshot()[band];
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
                                changed = true;
                            }
                            ui.label(
                                egui::RichText::new(format!("{:+.0} dB", v))
                                    .size(10.5)
                                    .color(theme::MUTED),
                            );
                        });
                    }
                });
                ui.add_space(10.0);
                ui.separator();
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    for (name, gains) in audio::EQ_PRESETS {
                        let active = self.eq_preset == *name;
                        if ui
                            .selectable_label(
                                active,
                                egui::RichText::new(*name).size(12.5).color(if active {
                                    theme::GREEN
                                } else {
                                    theme::GRAY
                                }),
                            )
                            .clicked()
                        {
                            self.player.set_eq_all(*gains);
                            self.eq_preset = String::from(*name);
                            changed = true;
                        }
                    }
                });
            });
        if changed {
            self.save_settings();
        }
        if !open {
            self.show_eq = false;
        }
    }

    fn show_shortcuts_window(&mut self, ctx: &egui::Context) {
        if !self.show_shortcuts {
            return;
        }
        let mut open = true;
        egui::Window::new("Pintasan Keyboard")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                egui::Grid::new("shortcuts_grid")
                    .num_columns(2)
                    .spacing([24.0, 6.0])
                    .show(ui, |ui| {
                        for (k, desc) in [
                            ("Space / K", "Putar / Jeda"),
                            ("← / →", "Mundur / Maju 5 detik"),
                            ("Shift + ← / →", "Mundur / Maju 15 detik"),
                            ("Ctrl + ← / →  atau  P / N", "Lagu sebelumnya / berikutnya"),
                            ("↑ / ↓", "Volume +5% / -5%"),
                            ("M", "Bisukan / Bunyikan"),
                            ("S", "Acak (Shuffle)"),
                            ("R", "Ulangi (Mati / Semua / Satu)"),
                            ("L", "Tampilkan / Tutup Lirik"),
                            ("E", "Buka / Tutup Equalizer"),
                            ("Ctrl + F", "Fokus kotak pencarian"),
                            ("Esc", "Tutup jendela / bersihkan pencarian"),
                        ] {
                            ui.label(
                                egui::RichText::new(k)
                                    .font(theme::bold(12.5))
                                    .color(theme::GREEN),
                            );
                            ui.label(egui::RichText::new(desc).size(12.5).color(theme::GRAY));
                            ui.end_row();
                        }
                    });
            });
        if !open {
            self.show_shortcuts = false;
        }
    }

    // -----------------------------------------------------------------------
    // Cover Art & Background Channels
    // -----------------------------------------------------------------------

    fn paint_cover(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        radius: f32,
        fallback_col: Option<Color32>,
    ) {
        if let Some(tex) = &self.cover_texture {
            painter.image(
                tex.id(),
                rect,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
            painter.rect_stroke(
                rect,
                CornerRadius::same(radius as u8),
                Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 20)),
                egui::StrokeKind::Inside,
            );
        } else {
            let bg = fallback_col.unwrap_or(theme::ELEVATED);
            painter.rect_filled(rect, radius, bg);
            theme::paint_icon(
                painter,
                Rect::from_center_size(rect.center(), rect.size() * 0.44),
                Icon::Note,
                theme::MUTED,
            );
        }
    }

    fn sync_cover(&mut self, ctx: &egui::Context) {
        let path = self
            .current
            .and_then(|i| self.tracks.get(i))
            .map(|t| t.path.clone());
        if path == self.cover_for {
            return;
        }
        self.cover_for = path.clone();
        self.cover_accent = None;
        self.cover_texture = path
            .as_ref()
            .and_then(|p| library::cover_art(p))
            .and_then(|bytes| image::load_from_memory(&bytes).ok())
            .map(|img| {
                let rgba = img.to_rgba8();
                let (w, h) = (rgba.width() as usize, rgba.height() as usize);
                // Hitung warna rata-rata untuk gradien tema (sampling tiap 8 piksel).
                let (mut sr, mut sg, mut sb, mut n) = (0.0f64, 0.0f64, 0.0f64, 0u64);
                let mut pixels = Vec::with_capacity(w * h);
                for (i, p) in rgba.pixels().enumerate() {
                    if i % 8 == 0 {
                        sr += p[0] as f64;
                        sg += p[1] as f64;
                        sb += p[2] as f64;
                        n += 1;
                    }
                    pixels.push(Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3]));
                }
                if n > 0 {
                    self.cover_accent = Some(theme::accent_from_rgb(
                        (sr / n as f64) as f32,
                        (sg / n as f64) as f32,
                        (sb / n as f64) as f32,
                    ));
                }
                let cimg = egui::ColorImage::new([w, h], pixels);
                ctx.load_texture("cover-art", cimg, egui::TextureOptions::LINEAR)
            });
    }

    fn poll_channels(&mut self) {
        if let Ok(tracks) = self.scan_rx.try_recv() {
            let prev_path = self
                .current
                .and_then(|i| self.tracks.get(i))
                .map(|t| t.path.clone());
            let n = tracks.len();
            self.tracks = tracks;

            // Pertahankan lagu yang sedang diputar saat rescan.
            if let Some(p) = prev_path {
                self.current = self.tracks.iter().position(|t| t.path == p);
            } else if let Some((restore_path, restore_pos)) = self.pending_restore.take() {
                if let Some(idx) = self.tracks.iter().position(|t| t.path == restore_path) {
                    let track = self.tracks[idx].clone();
                    if self.player.load_paused(&track, restore_pos).is_ok() {
                        self.current = Some(idx);
                        self.fetch_lyrics_for(track, false);
                    }
                }
            }

            if self.shuffle {
                self.ensure_shuffle_order();
            }

            self.status_msg = if n == 0 {
                String::from("Tidak ada file audio di folder ini.")
            } else {
                format!("{n} lagu siap diputar.")
            };
        }

        if let Ok((path, lyrics, from_cache)) = self.lyric_rx.try_recv() {
            let still_current = self
                .current
                .and_then(|i| self.tracks.get(i))
                .is_some_and(|t| t.path == path);
            if still_current {
                match lyrics {
                    Some(l) if l.synced => {
                        self.status_msg = if from_cache {
                            String::from("Lirik tersinkron (offline)")
                        } else {
                            String::from("Lirik tersinkron (diunduh & disimpan)")
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
                        self.status_msg = String::from("Lirik teks biasa ditemukan.");
                    }
                    None => {
                        self.lyric_state = LyricState::NotFound;
                        self.status_msg = String::from("Lirik tidak ditemukan.");
                    }
                }
            }
        }
    }

    fn filtered_indices(&self) -> Vec<usize> {
        filter_and_sort_indices(
            &self.tracks,
            self.active_playlist
                .and_then(|i| self.playlists.get(i))
                .map(|pl| pl.paths.as_slice()),
            &self.search,
            self.search_scope,
            self.sort_key,
            self.sort_asc,
        )
    }
}

/// Filter + urutkan daftar lagu (dipisah agar mudah diuji unit-test).
fn filter_and_sort_indices(
    tracks: &[Track],
    playlist_paths: Option<&[PathBuf]>,
    search: &str,
    scope: SearchScope,
    sort_key: SortKey,
    sort_asc: bool,
) -> Vec<usize> {
    let q = search.trim().to_lowercase();
    let in_playlist: Option<HashSet<&PathBuf>> =
        playlist_paths.map(|paths| paths.iter().collect());

    let mut out: Vec<usize> = tracks
        .iter()
        .enumerate()
        .filter(|(_, t)| {
            in_playlist
                .as_ref()
                .map(|set| set.contains(&t.path))
                .unwrap_or(true)
        })
        .filter(|(_, t)| {
            if q.is_empty() {
                return true;
            }
            match scope {
                SearchScope::All => {
                    t.title.to_lowercase().contains(&q)
                        || t.artist.to_lowercase().contains(&q)
                        || t.album.to_lowercase().contains(&q)
                }
                SearchScope::Title => t.title.to_lowercase().contains(&q),
                SearchScope::Artist => t.artist.to_lowercase().contains(&q),
                SearchScope::Album => t.album.to_lowercase().contains(&q),
            }
        })
        .map(|(i, _)| i)
        .collect();

    match sort_key {
        SortKey::Default => {
            if !sort_asc {
                out.reverse();
            }
        }
        SortKey::Title => out.sort_by(|&a, &b| {
            let ord = tracks[a]
                .title
                .to_lowercase()
                .cmp(&tracks[b].title.to_lowercase());
            if sort_asc { ord } else { ord.reverse() }
        }),
        SortKey::Artist => out.sort_by(|&a, &b| {
            let ord = tracks[a]
                .artist
                .to_lowercase()
                .cmp(&tracks[b].artist.to_lowercase())
                .then_with(|| {
                    tracks[a]
                        .title
                        .to_lowercase()
                        .cmp(&tracks[b].title.to_lowercase())
                });
            if sort_asc { ord } else { ord.reverse() }
        }),
        SortKey::Album => out.sort_by(|&a, &b| {
            let ord = tracks[a]
                .album
                .to_lowercase()
                .cmp(&tracks[b].album.to_lowercase())
                .then_with(|| {
                    tracks[a]
                        .title
                        .to_lowercase()
                        .cmp(&tracks[b].title.to_lowercase())
                });
            if sort_asc { ord } else { ord.reverse() }
        }),
        SortKey::Duration => out.sort_by(|&a, &b| {
            let ord = tracks[a].duration_secs.total_cmp(&tracks[b].duration_secs);
            if sort_asc { ord } else { ord.reverse() }
        }),
    }
    out
}

fn fmt_time(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    format!("{:02}:{:02}", s / 60, s % 60)
}

fn fmt_duration_long(secs: f64) -> String {
    let total = secs.max(0.0) as u64;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    if h > 0 {
        format!("{h} jam {m} mnt")
    } else {
        format!("{m} mnt")
    }
}

impl eframe::App for MusicApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll_channels();
        self.sync_cover(&ctx);
        self.handle_shortcuts(&ctx);

        // Simpan posisi terakhir tiap 5 detik saat lagu berjalan & saat jendela ditutup.
        if self.player.is_playing() && self.last_pos_save.elapsed() >= Duration::from_secs(5) {
            self.save_settings();
            self.last_pos_save = Instant::now();
        }
        if ctx.input(|i| i.viewport().close_requested()) {
            self.save_settings();
        }

        // Lanjut otomatis ke lagu berikut saat selesai.
        if self.player.finished() {
            self.advance();
        }

        // Minimize -> ubah jadi mini player (bila aktif di pengaturan).
        let minimized = ctx.input(|i| i.viewport().minimized);
        if minimized == Some(true) && !self.mini_mode && self.auto_mini {
            self.set_mini(&ctx, true);
        }

        // ---- Mode mini player ----
        if self.mini_mode {
            egui::CentralPanel::no_frame().show(ui, |ui| {
                self.show_mini(ui);
            });
            ctx.request_repaint_after(Duration::from_millis(120));
            return;
        }

        // ---- Player bar penuh di bawah (ala Spotify desktop) ----
        egui::Panel::bottom("player_bar")
            .resizable(false)
            .exact_size(86.0)
            .frame(egui::Frame::NONE.fill(theme::BLACK))
            .show(ui, |ui| {
                self.show_player_bar(ui);
            });

        // ---- Sidebar kiri ----
        egui::Panel::left("sidebar")
            .resizable(false)
            .exact_size(self.sidebar_width)
            .frame(egui::Frame::NONE.fill(theme::BLACK).inner_margin(14.0))
            .show(ui, |ui| {
                self.show_sidebar(ui);
            });

        // ---- Konten utama ----
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(theme::BG).inner_margin(egui::Margin::ZERO))
            .show(ui, |ui| match self.view {
                View::Playlist => self.show_playlist_page(ui),
                View::Lyrics => self.show_lyrics_view(ui),
            });

        // ---- Drag handle tepi kanan sidebar ----
        {
            let win_r = ui.max_rect();
            let bar_top = win_r.max.y - 86.0;
            let x = self.sidebar_width;
            let strip = Rect::from_min_max(pos2(x - 5.0, 0.0), pos2(x + 5.0, bar_top));
            let resp = ui.interact(strip, egui::Id::new("sidebar_resize"), Sense::drag());
            if resp.dragged() {
                if let Some(p) = resp.interact_pointer_pos() {
                    let max_w = (win_r.width() - 360.0).max(210.0);
                    self.sidebar_width = p.x.clamp(210.0, 420.0).min(max_w);
                }
            }
            if resp.drag_stopped() {
                self.save_settings();
            }
            if resp.hovered() || resp.dragged() {
                ctx.set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
            let line_col = if resp.dragged() {
                theme::GREEN
            } else if resp.hovered() {
                theme::GREEN_HOVER
            } else {
                theme::BORDER
            };
            let lw = if resp.hovered() || resp.dragged() { 2.0 } else { 1.0 };
            ui.painter()
                .line_segment([pos2(x, 0.0), pos2(x, bar_top)], Stroke::new(lw, line_col));
        }

        // Window mengambang
        self.show_eq_window(&ctx);
        self.show_shortcuts_window(&ctx);

        if self.player.is_playing() {
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
    }
}

fn main() -> eframe::Result<()> {
    let ver = env!("CARGO_PKG_VERSION");
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1140.0, 720.0])
            .with_min_inner_size([760.0, 500.0])
            .with_title(format!("Joni Music v{ver}")),
        ..Default::default()
    };
    eframe::run_native(
        &format!("Joni Music v{ver}"),
        options,
        Box::new(|cc| {
            theme::setup(&cc.egui_ctx);
            theme::setup_fonts(
                &cc.egui_ctx,
                include_bytes!("../assets/fonts/NotoSansCJK-sc.ttf"),
                include_bytes!("../assets/fonts/NotoSansCJK-kr.ttf"),
            );
            // Monitor thread: bangunkan UI tiap 500ms agar auto-advance tetap berjalan
            // walau jendela di-minimize di Windows.
            let rep_ctx = cc.egui_ctx.clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(Duration::from_millis(500));
                rep_ctx.request_repaint();
            });
            Ok(Box::new(MusicApp::new()))
        }),
    )
}

#[cfg(test)]
mod filter_sort_tests {
    use super::*;

    fn mk(title: &str, artist: &str, album: &str, dur: f64) -> Track {
        Track {
            path: PathBuf::from(format!("{artist}-{title}.mp3")),
            title: title.to_string(),
            artist: artist.to_string(),
            album: album.to_string(),
            duration_secs: dur,
        }
    }

    #[test]
    fn search_scopes_work() {
        let tracks = vec![
            mk("Starlight", "Muse", "Black Holes", 240.0),
            mk("Yellow", "Coldplay", "Parachutes", 269.0),
            mk("Blackbird", "Beatles", "White Album", 138.0),
        ];
        // Scope All menemukan "black" di album maupun judul
        let all = filter_and_sort_indices(&tracks, None, "black", SearchScope::All, SortKey::Default, true);
        assert_eq!(all, vec![0, 2]);
        // Scope Album hanya menemukan "Black Holes"
        let alb = filter_and_sort_indices(&tracks, None, "black", SearchScope::Album, SortKey::Default, true);
        assert_eq!(alb, vec![0]);
        // Scope Artist hanya menemukan "Coldplay"
        let art = filter_and_sort_indices(&tracks, None, "cold", SearchScope::Artist, SortKey::Default, true);
        assert_eq!(art, vec![1]);
    }

    #[test]
    fn sorting_by_title_and_duration() {
        let tracks = vec![
            mk("Zebra", "B", "Al2", 200.0),
            mk("Alpha", "C", "Al1", 100.0),
            mk("Mango", "A", "Al3", 300.0),
        ];
        let by_title = filter_and_sort_indices(&tracks, None, "", SearchScope::All, SortKey::Title, true);
        assert_eq!(by_title, vec![1, 2, 0]);
        let by_dur_desc = filter_and_sort_indices(&tracks, None, "", SearchScope::All, SortKey::Duration, false);
        assert_eq!(by_dur_desc, vec![2, 0, 1]);
    }
}

#[cfg(test)]
mod cjk_font_tests {
    const SC: &[u8] = include_bytes!("../assets/fonts/NotoSansCJK-sc.ttf");
    const KR: &[u8] = include_bytes!("../assets/fonts/NotoSansCJK-kr.ttf");

    fn punya_glyph(font: &[u8], ch: char) -> bool {
        ttf_parser::Face::parse(font, 0)
            .map(|f| f.glyph_index(ch).is_some())
            .unwrap_or(false)
    }

    #[test]
    fn font_sc_memuat_kana_dan_kanji() {
        for ch in [
            'あ', 'い', 'う', 'ア', 'イ', 'ウ', 'ー', '。', '、', '「', '」', '・',
        ] {
            assert!(punya_glyph(SC, ch), "glyph hilang: {ch}");
        }
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
        for ch in ['\u{1100}', '\u{1161}', '\u{11A8}'] {
            assert!(punya_glyph(KR, ch), "glyph hilang: U+{:04X}", ch as u32);
        }
    }
}
