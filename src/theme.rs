//! Tema visual (warna ala Spotify), gaya egui global, ikon vektor, dan
//! widget kecil yang digambar manual (slider tipis, tombol ikon, gradien).
//!
//! Ikon digambar dengan painter (bukan emoji) agar tajam di semua ukuran
//! dan konsisten di semua mesin Windows.

use eframe::egui::{
    self, pos2, vec2, Color32, CornerRadius, FontFamily, FontId, Painter, Pos2, Rect, Response,
    Sense, Shape, Stroke, Ui,
};
use std::sync::Arc;

// ---- Palet ----
pub const BG: Color32 = Color32::from_rgb(0x12, 0x12, 0x12);
pub const BLACK: Color32 = Color32::from_rgb(0x00, 0x00, 0x00);
pub const ELEVATED: Color32 = Color32::from_rgb(0x1f, 0x1f, 0x1f);
pub const HOVER: Color32 = Color32::from_rgb(0x1f, 0x1f, 0x1f);
pub const ROW_HOVER: Color32 = Color32::from_rgb(0x2a, 0x2a, 0x2a);
pub const SELECTED: Color32 = Color32::from_rgb(0x2a, 0x2a, 0x2a);
pub const BORDER: Color32 = Color32::from_rgb(0x2a, 0x2a, 0x2a);
pub const GREEN: Color32 = Color32::from_rgb(0x1e, 0xd7, 0x60);
pub const GREEN_HOVER: Color32 = Color32::from_rgb(0x3b, 0xe4, 0x77);
pub const WHITE: Color32 = Color32::WHITE;
pub const GRAY: Color32 = Color32::from_rgb(0xb3, 0xb3, 0xb3);
pub const MUTED: Color32 = Color32::from_rgb(0x7a, 0x7a, 0x7a);

/// Palet warna kartu/playlist (diambil dari kategori Spotify).
const TILE_COLORS: [Color32; 10] = [
    Color32::from_rgb(0x50, 0x9b, 0xf5),
    Color32::from_rgb(0xe8, 0x11, 0x5b),
    Color32::from_rgb(0x8d, 0x67, 0xab),
    Color32::from_rgb(0x1e, 0x32, 0x64),
    Color32::from_rgb(0xe1, 0x33, 0x00),
    Color32::from_rgb(0x47, 0x7d, 0x95),
    Color32::from_rgb(0xba, 0x5d, 0x07),
    Color32::from_rgb(0x14, 0x8a, 0x08),
    Color32::from_rgb(0xaf, 0x28, 0x96),
    Color32::from_rgb(0x27, 0x85, 0x6a),
];

pub fn bold_family() -> FontFamily {
    FontFamily::Name("bold".into())
}

pub fn bold(size: f32) -> FontId {
    FontId::new(size, bold_family())
}

pub fn regular(size: f32) -> FontId {
    FontId::proportional(size)
}

/// Warna deterministik dari teks (nama playlist / judul lagu).
pub fn color_for(seed: &str) -> Color32 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in seed.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    TILE_COLORS[(h % TILE_COLORS.len() as u64) as usize]
}

pub fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()), l(a.a(), b.a()))
}

pub fn darken(c: Color32, f: f32) -> Color32 {
    lerp_color(c, BLACK, 1.0 - f.clamp(0.0, 1.0))
}

/// Normalisasi warna rata-rata cover menjadi aksen yang cukup gelap
/// untuk latar teks putih, tapi tetap berwarna.
pub fn accent_from_rgb(r: f32, g: f32, b: f32) -> Color32 {
    let max = r.max(g).max(b).max(1.0);
    let min = r.min(g).min(b);
    // Tambah saturasi sedikit: jauhkan channel dari rata-rata.
    let mean = (r + g + b) / 3.0;
    let sat = if max - min < 12.0 { 1.0 } else { 1.35 };
    let adj = |c: f32| (mean + (c - mean) * sat).max(0.0);
    let (r, g, b) = (adj(r), adj(g), adj(b));
    let max = r.max(g).max(b).max(1.0);
    let k = 150.0 / max;
    Color32::from_rgb(
        (r * k).clamp(0.0, 255.0) as u8,
        (g * k).clamp(0.0, 255.0) as u8,
        (b * k).clamp(0.0, 255.0) as u8,
    )
}

/// Pasang tema gelap global ke context egui.
pub fn setup(ctx: &egui::Context) {
    ctx.set_theme(egui::ThemePreference::Dark);
    ctx.style_mut_of(egui::Theme::Dark, |style| {
        use egui::{FontId as F, TextStyle};
        style.text_styles.insert(TextStyle::Body, F::proportional(14.0));
        style.text_styles.insert(TextStyle::Button, F::proportional(14.0));
        style.text_styles.insert(TextStyle::Small, F::proportional(11.5));
        style.text_styles.insert(TextStyle::Heading, F::new(22.0, bold_family()));
        style.text_styles.insert(TextStyle::Monospace, F::monospace(13.0));

        style.spacing.item_spacing = vec2(8.0, 6.0);
        style.spacing.button_padding = vec2(10.0, 5.0);
        style.spacing.interact_size.y = 26.0;
        style.spacing.scroll = egui::style::ScrollStyle::floating();

        let v = &mut style.visuals;
        v.dark_mode = true;
        v.override_text_color = None;
        v.panel_fill = BG;
        v.window_fill = Color32::from_rgb(0x28, 0x28, 0x28);
        v.window_stroke = Stroke::new(1.0, Color32::from_rgb(0x3a, 0x3a, 0x3a));
        v.window_corner_radius = CornerRadius::same(10);
        v.menu_corner_radius = CornerRadius::same(6);
        v.faint_bg_color = Color32::from_rgb(0x1a, 0x1a, 0x1a);
        v.extreme_bg_color = Color32::from_rgb(0x24, 0x24, 0x24);
        v.code_bg_color = Color32::from_rgb(0x24, 0x24, 0x24);
        v.hyperlink_color = GREEN;
        v.selection.bg_fill = Color32::from_rgb(0x1d, 0x6b, 0x3c);
        v.selection.stroke = Stroke::new(1.0, WHITE);
        v.slider_trailing_fill = true;

        let w = &mut v.widgets;
        w.noninteractive.bg_fill = BG;
        w.noninteractive.weak_bg_fill = BG;
        w.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
        w.noninteractive.fg_stroke = Stroke::new(1.0, GRAY);
        w.noninteractive.corner_radius = CornerRadius::same(6);

        w.inactive.bg_fill = Color32::from_rgb(0x2a, 0x2a, 0x2a);
        w.inactive.weak_bg_fill = Color32::from_rgb(0x24, 0x24, 0x24);
        w.inactive.bg_stroke = Stroke::NONE;
        w.inactive.fg_stroke = Stroke::new(1.0, Color32::from_rgb(0xd6, 0xd6, 0xd6));
        w.inactive.corner_radius = CornerRadius::same(6);

        w.hovered.bg_fill = Color32::from_rgb(0x33, 0x33, 0x33);
        w.hovered.weak_bg_fill = Color32::from_rgb(0x33, 0x33, 0x33);
        w.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0x55, 0x55, 0x55));
        w.hovered.fg_stroke = Stroke::new(1.5, WHITE);
        w.hovered.corner_radius = CornerRadius::same(6);

        w.active.bg_fill = Color32::from_rgb(0x3e, 0x3e, 0x3e);
        w.active.weak_bg_fill = Color32::from_rgb(0x3e, 0x3e, 0x3e);
        w.active.bg_stroke = Stroke::new(1.0, GREEN);
        w.active.fg_stroke = Stroke::new(2.0, WHITE);
        w.active.corner_radius = CornerRadius::same(6);

        w.open.bg_fill = Color32::from_rgb(0x2e, 0x2e, 0x2e);
        w.open.weak_bg_fill = Color32::from_rgb(0x2e, 0x2e, 0x2e);
        w.open.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0x55, 0x55, 0x55));
        w.open.fg_stroke = Stroke::new(1.0, WHITE);
        w.open.corner_radius = CornerRadius::same(6);
    });
}

/// Font: Segoe UI (bila ada di Windows) sebagai font utama agar tampilan
/// bersih, ditambah Noto Sans CJK sebagai fallback Jepang/Korea/China.
pub fn setup_fonts(ctx: &egui::Context, cjk_sc: &'static [u8], cjk_kr: &'static [u8]) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "noto_cjk_sc".to_owned(),
        Arc::new(egui::FontData::from_static(cjk_sc)),
    );
    fonts.font_data.insert(
        "noto_cjk_kr".to_owned(),
        Arc::new(egui::FontData::from_static(cjk_kr)),
    );

    let windir = std::env::var("WINDIR").unwrap_or_else(|_| String::from("C:\\Windows"));
    let load = |name: &str| std::fs::read(format!("{windir}\\Fonts\\{name}")).ok();
    let regular = load("segoeui.ttf");
    let semibold = load("seguisb.ttf").or_else(|| load("segoeuib.ttf"));

    let mut prop = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let mut bold_list = prop.clone();
    if let Some(bytes) = regular {
        fonts
            .font_data
            .insert("segoe".to_owned(), Arc::new(egui::FontData::from_owned(bytes)));
        prop.insert(0, "segoe".to_owned());
        bold_list.insert(0, "segoe".to_owned());
    }
    if let Some(bytes) = semibold {
        fonts
            .font_data
            .insert("segoe_sb".to_owned(), Arc::new(egui::FontData::from_owned(bytes)));
        bold_list.insert(0, "segoe_sb".to_owned());
    }
    for list in [&mut prop, &mut bold_list] {
        list.push("noto_cjk_sc".to_owned());
        list.push("noto_cjk_kr".to_owned());
    }
    fonts.families.insert(FontFamily::Proportional, prop);
    fonts.families.insert(bold_family(), bold_list);
    if let Some(mono) = fonts.families.get_mut(&FontFamily::Monospace) {
        mono.push("noto_cjk_sc".to_owned());
        mono.push("noto_cjk_kr".to_owned());
    }
    ctx.set_fonts(fonts);
}

/// Gradien vertikal dengan sudut membulat (atas & bawah terpisah).
pub fn gradient_rect(
    painter: &Painter,
    rect: Rect,
    top: Color32,
    bottom: Color32,
    r_top: f32,
    r_bottom: f32,
) {
    if rect.height() <= 0.0 || rect.width() <= 0.0 {
        return;
    }
    let h = rect.height();
    let r_top = r_top.min(h / 2.0).min(rect.width() / 2.0);
    let r_bottom = r_bottom.min(h / 2.0).min(rect.width() / 2.0);
    let inset = |y: f32| -> f32 {
        let dt = y - rect.top();
        let db = rect.bottom() - y;
        if dt < r_top {
            let d = r_top - dt;
            r_top - (r_top * r_top - d * d).max(0.0).sqrt()
        } else if db < r_bottom {
            let d = r_bottom - db;
            r_bottom - (r_bottom * r_bottom - d * d).max(0.0).sqrt()
        } else {
            0.0
        }
    };
    let mut ys: Vec<f32> = Vec::new();
    const STEPS: usize = 8;
    for i in 0..=STEPS {
        ys.push(rect.top() + r_top * i as f32 / STEPS as f32);
    }
    for i in 0..=STEPS {
        ys.push(rect.bottom() - r_bottom * (STEPS - i) as f32 / STEPS as f32);
    }
    ys.dedup_by(|a, b| (*a - *b).abs() < 0.01);

    let mut mesh = egui::Mesh::default();
    for (k, &y) in ys.iter().enumerate() {
        let c = lerp_color(top, bottom, (y - rect.top()) / h);
        let dx = inset(y);
        mesh.colored_vertex(pos2(rect.left() + dx, y), c);
        mesh.colored_vertex(pos2(rect.right() - dx, y), c);
        if k > 0 {
            let i = (k as u32) * 2;
            mesh.add_triangle(i - 2, i - 1, i);
            mesh.add_triangle(i - 1, i + 1, i);
        }
    }
    painter.add(Shape::mesh(mesh));
}

/// Layout teks satu baris yang dipotong dengan "…" bila melebihi `max_w`.
pub fn trunc_galley(
    painter: &Painter,
    text: &str,
    font: FontId,
    color: Color32,
    max_w: f32,
) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::single_section(
        text.to_owned(),
        egui::TextFormat::simple(font, color),
    );
    job.wrap = egui::text::TextWrapping {
        max_width: max_w.max(1.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
        ..Default::default()
    };
    painter.layout_job(job)
}

/// Gambar teks terpotong. `anchor` menentukan perataan terhadap `pos`.
pub fn text_trunc(
    painter: &Painter,
    pos: Pos2,
    anchor: egui::Align2,
    text: &str,
    font: FontId,
    color: Color32,
    max_w: f32,
) -> Rect {
    let g = trunc_galley(painter, text, font, color, max_w);
    let rect = anchor.anchor_size(pos, g.size());
    painter.galley(rect.min, g, color);
    rect
}

// ---------------------------------------------------------------------------
// Ikon vektor
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Icon {
    Play,
    Pause,
    Prev,
    Next,
    Shuffle,
    Repeat,
    RepeatOne,
    /// Level volume 0..1 (0 = bisu).
    Volume(f32),
    Lyrics,
    Mini,
    Expand,
    Close,
    Eq,
    Device,
    Home,
    Search,
    Folder,
    Refresh,
    Plus,
    Gear,
    Note,
    Library,
    Clock,
    SortAsc,
    SortDesc,
}

/// Gambar ikon di dalam `rect` (grid desain 24x24).
pub fn paint_icon(painter: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let s = rect.width().min(rect.height());
    let u = s / 24.0;
    let c = rect.center();
    let p = |x: f32, y: f32| pos2(c.x + (x - 12.0) * u, c.y + (y - 12.0) * u);
    let sw = (2.0 * u).max(1.2);
    let stroke = Stroke::new(sw, color);
    let fill_poly = |pts: Vec<Pos2>| {
        painter.add(Shape::convex_polygon(pts, color, Stroke::NONE));
    };
    let line = |pts: Vec<Pos2>| {
        painter.add(Shape::line(pts, stroke));
    };
    let arc = |cx: f32, cy: f32, r: f32, a0: f32, a1: f32| -> Vec<Pos2> {
        let n = 16;
        (0..=n)
            .map(|i| {
                let a = (a0 + (a1 - a0) * i as f32 / n as f32).to_radians();
                p(cx + r * a.cos(), cy + r * a.sin())
            })
            .collect()
    };
    match icon {
        Icon::Play => fill_poly(vec![p(7.0, 4.0), p(20.0, 12.0), p(7.0, 20.0)]),
        Icon::Pause => {
            painter.rect_filled(Rect::from_min_max(p(6.0, 4.5), p(10.0, 19.5)), u, color);
            painter.rect_filled(Rect::from_min_max(p(14.0, 4.5), p(18.0, 19.5)), u, color);
        }
        Icon::Prev => {
            painter.rect_filled(Rect::from_min_max(p(5.0, 5.0), p(7.5, 19.0)), u * 0.6, color);
            fill_poly(vec![p(19.0, 5.0), p(19.0, 19.0), p(8.5, 12.0)]);
        }
        Icon::Next => {
            painter.rect_filled(Rect::from_min_max(p(16.5, 5.0), p(19.0, 19.0)), u * 0.6, color);
            fill_poly(vec![p(5.0, 5.0), p(15.5, 12.0), p(5.0, 19.0)]);
        }
        Icon::Shuffle => {
            line(vec![p(3.0, 7.0), p(7.5, 7.0), p(14.5, 17.0), p(19.0, 17.0)]);
            line(vec![p(3.0, 17.0), p(7.5, 17.0), p(14.5, 7.0), p(19.0, 7.0)]);
            fill_poly(vec![p(22.0, 7.0), p(18.0, 3.8), p(18.0, 10.2)]);
            fill_poly(vec![p(22.0, 17.0), p(18.0, 13.8), p(18.0, 20.2)]);
        }
        Icon::Repeat | Icon::RepeatOne => {
            painter.rect_stroke(
                Rect::from_min_max(p(4.0, 7.0), p(20.0, 18.0)),
                3.5 * u,
                stroke,
                egui::StrokeKind::Middle,
            );
            fill_poly(vec![p(16.0, 7.0), p(12.0, 3.8), p(12.0, 10.2)]);
            if icon == Icon::RepeatOne {
                painter.circle_filled(p(12.0, 12.5), 4.6 * u, color);
                painter.text(
                    p(12.0, 12.6),
                    egui::Align2::CENTER_CENTER,
                    "1",
                    bold(7.5 * u),
                    BLACK,
                );
            }
        }
        Icon::Volume(level) => {
            painter.rect_filled(Rect::from_min_max(p(3.0, 9.0), p(7.5, 15.0)), u * 0.5, color);
            fill_poly(vec![p(7.0, 9.0), p(12.5, 4.5), p(12.5, 19.5), p(7.0, 15.0)]);
            if level <= 0.001 {
                line(vec![p(15.5, 9.0), p(21.0, 15.0)]);
                line(vec![p(21.0, 9.0), p(15.5, 15.0)]);
            } else {
                line(arc(12.5, 12.0, 4.0, -45.0, 45.0));
                if level > 0.33 {
                    line(arc(12.5, 12.0, 7.5, -50.0, 50.0));
                }
                if level > 0.75 {
                    line(arc(12.5, 12.0, 11.0, -50.0, 50.0));
                }
            }
        }
        Icon::Lyrics => {
            painter.rect_stroke(
                Rect::from_min_max(p(9.0, 2.5), p(15.0, 14.0)),
                3.0 * u,
                stroke,
                egui::StrokeKind::Middle,
            );
            line(arc(12.0, 10.5, 6.5, 0.0, 180.0));
            line(vec![p(12.0, 17.0), p(12.0, 21.5)]);
            line(vec![p(8.5, 21.5), p(15.5, 21.5)]);
        }
        Icon::Mini => {
            painter.rect_stroke(
                Rect::from_min_max(p(2.5, 4.5), p(21.5, 19.5)),
                2.0 * u,
                stroke,
                egui::StrokeKind::Middle,
            );
            painter.rect_filled(Rect::from_min_max(p(12.0, 11.5), p(19.0, 17.0)), u, color);
        }
        Icon::Expand => {
            line(vec![p(4.0, 10.0), p(4.0, 4.0), p(10.0, 4.0)]);
            line(vec![p(14.0, 4.0), p(20.0, 4.0), p(20.0, 10.0)]);
            line(vec![p(20.0, 14.0), p(20.0, 20.0), p(14.0, 20.0)]);
            line(vec![p(10.0, 20.0), p(4.0, 20.0), p(4.0, 14.0)]);
        }
        Icon::Close => {
            line(vec![p(6.0, 6.0), p(18.0, 18.0)]);
            line(vec![p(18.0, 6.0), p(6.0, 18.0)]);
        }
        Icon::Eq => {
            for (x, knob) in [(6.0, 14.0), (12.0, 8.0), (18.0, 16.0)] {
                line(vec![p(x, 3.5), p(x, 20.5)]);
                painter.circle_filled(p(x, knob), 2.8 * u, color);
            }
        }
        Icon::Device => {
            painter.rect_stroke(
                Rect::from_min_max(p(5.5, 2.5), p(18.5, 21.5)),
                2.5 * u,
                stroke,
                egui::StrokeKind::Middle,
            );
            painter.circle_stroke(p(12.0, 14.5), 3.6 * u, stroke);
            painter.circle_filled(p(12.0, 7.0), 1.4 * u, color);
        }
        Icon::Home => {
            painter.add(Shape::closed_line(
                vec![p(4.0, 11.0), p(12.0, 4.0), p(20.0, 11.0), p(20.0, 20.5), p(4.0, 20.5)],
                stroke,
            ));
            painter.rect_filled(Rect::from_min_max(p(10.0, 14.0), p(14.0, 20.5)), 0.0, color);
        }
        Icon::Search => {
            painter.circle_stroke(p(10.5, 10.5), 6.5 * u, stroke);
            line(vec![p(15.5, 15.5), p(21.0, 21.0)]);
        }
        Icon::Folder => {
            painter.add(Shape::closed_line(
                vec![
                    p(2.5, 5.0),
                    p(9.0, 5.0),
                    p(11.0, 7.5),
                    p(21.5, 7.5),
                    p(21.5, 19.5),
                    p(2.5, 19.5),
                ],
                stroke,
            ));
        }
        Icon::Refresh => {
            line(arc(12.0, 12.0, 7.5, -60.0, 250.0));
            let a = (-60.0f32).to_radians();
            let tip = (12.0 + 7.5 * a.cos(), 12.0 + 7.5 * a.sin());
            fill_poly(vec![
                p(tip.0 + 3.6, tip.1 + 0.6),
                p(tip.0 - 1.6, tip.1 - 3.2),
                p(tip.0 - 1.2, tip.1 + 3.6),
            ]);
        }
        Icon::Plus => {
            line(vec![p(12.0, 4.0), p(12.0, 20.0)]);
            line(vec![p(4.0, 12.0), p(20.0, 12.0)]);
        }
        Icon::Gear => {
            painter.circle_stroke(p(12.0, 12.0), 6.0 * u, stroke);
            painter.circle_stroke(p(12.0, 12.0), 2.4 * u, stroke);
            for k in 0..8 {
                let a = (k as f32 * 45.0).to_radians();
                line(vec![
                    p(12.0 + 6.0 * a.cos(), 12.0 + 6.0 * a.sin()),
                    p(12.0 + 9.5 * a.cos(), 12.0 + 9.5 * a.sin()),
                ]);
            }
        }
        Icon::Note => {
            painter.circle_filled(p(8.0, 17.5), 3.4 * u, color);
            line(vec![p(11.2, 17.5), p(11.2, 3.5), p(18.5, 6.0), p(18.5, 10.0)]);
        }
        Icon::Library => {
            line(vec![p(5.0, 3.5), p(5.0, 20.5)]);
            line(vec![p(10.5, 3.5), p(10.5, 20.5)]);
            line(vec![p(14.5, 4.5), p(20.0, 20.0)]);
        }
        Icon::Clock => {
            painter.circle_stroke(p(12.0, 12.0), 8.5 * u, stroke);
            line(vec![p(12.0, 7.0), p(12.0, 12.0), p(15.5, 14.0)]);
        }
        Icon::SortAsc => fill_poly(vec![p(12.0, 7.0), p(18.0, 16.0), p(6.0, 16.0)]),
        Icon::SortDesc => fill_poly(vec![p(6.0, 8.0), p(18.0, 8.0), p(12.0, 17.0)]),
    }
}

fn brighten(c: Color32) -> Color32 {
    if c == GREEN {
        GREEN_HOVER
    } else {
        WHITE
    }
}

fn paint_icon_for(ui: &Ui, rect: Rect, resp: &Response, icon: Icon, color: Color32) {
    let hovered = resp.hovered();
    let col = if hovered { brighten(color) } else { color };
    let scale = if resp.is_pointer_button_down_on() {
        0.92
    } else if hovered {
        1.05
    } else {
        1.0
    };
    let ir = Rect::from_center_size(rect.center(), rect.size() * 0.64 * scale);
    paint_icon(ui.painter(), ir, icon, col);
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
}

/// Tombol ikon tanpa bingkai di posisi absolut.
pub fn icon_hit(
    ui: &mut Ui,
    rect: Rect,
    id: impl std::hash::Hash + std::fmt::Debug,
    icon: Icon,
    color: Color32,
) -> Response {
    let resp = ui.interact(rect, ui.id().with(id), Sense::click());
    paint_icon_for(ui, rect, &resp, icon, color);
    resp
}

/// Tombol ikon tanpa bingkai yang ikut layout.
pub fn icon_button(ui: &mut Ui, icon: Icon, size: f32, color: Color32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    paint_icon_for(ui, rect, &resp, icon, color);
    resp
}

/// Tombol lingkaran (mis. play putih / hijau) di posisi absolut.
pub fn circle_hit(
    ui: &mut Ui,
    rect: Rect,
    id: impl std::hash::Hash + std::fmt::Debug,
    icon: Icon,
    bg: Color32,
    fg: Color32,
) -> Response {
    let resp = ui.interact(rect, ui.id().with(id), Sense::click());
    let hovered = resp.hovered();
    let scale = if resp.is_pointer_button_down_on() {
        0.95
    } else if hovered {
        1.05
    } else {
        1.0
    };
    let r = rect.width().min(rect.height()) / 2.0 * scale;
    let bg = if hovered && bg == GREEN { GREEN_HOVER } else { bg };
    ui.painter().circle_filled(rect.center(), r, bg);
    // Segitiga play digeser sedikit ke kanan agar terlihat di tengah.
    let off = if icon == Icon::Play { vec2(r * 0.06, 0.0) } else { vec2(0.0, 0.0) };
    let ir = Rect::from_center_size(rect.center() + off, vec2(r, r) * 1.0);
    paint_icon(ui.painter(), ir, icon, fg);
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// Slider tipis ala Spotify (progress / volume). Mengembalikan nilai baru
/// (0..1) selama pointer menekan/menyeret.
pub fn thin_slider(
    ui: &mut Ui,
    rect: Rect,
    id: impl std::hash::Hash + std::fmt::Debug,
    frac: f32,
) -> (Response, Option<f32>) {
    let resp = ui.interact(rect, ui.id().with(id), Sense::click_and_drag());
    let pressing = resp.is_pointer_button_down_on() || resp.dragged();
    let active = resp.hovered() || pressing;
    let h = 4.0;
    let cy = rect.center().y;
    let track = Rect::from_min_max(pos2(rect.left(), cy - h / 2.0), pos2(rect.right(), cy + h / 2.0));
    let mut f = if frac.is_finite() { frac.clamp(0.0, 1.0) } else { 0.0 };
    let mut new = None;
    if pressing || resp.clicked() || resp.drag_stopped() {
        if let Some(p) = resp.interact_pointer_pos() {
            f = ((p.x - track.left()) / track.width().max(1.0)).clamp(0.0, 1.0);
            new = Some(f);
        }
    }
    let painter = ui.painter();
    painter.rect_filled(track, 2.0, Color32::from_gray(0x4d));
    let fill = Rect::from_min_size(track.min, vec2(track.width() * f, h));
    painter.rect_filled(fill, 2.0, if active { GREEN } else { WHITE });
    if active {
        painter.circle_filled(pos2(fill.right(), cy), 6.0, WHITE);
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    (resp, new)
}

/// Ikon equalizer animasi (3 batang) untuk lagu yang sedang diputar.
pub fn eq_bars(painter: &Painter, center: Pos2, t: f64, color: Color32) {
    for (i, base) in [12.0f64, 16.0, 9.0].iter().enumerate() {
        let h = (base * (0.45 + 0.55 * (t * 4.2 + i as f64 * 1.9).sin().abs())) as f32;
        let x = center.x - 5.0 + i as f32 * 5.0;
        let bottom = center.y + 8.0;
        painter.rect_filled(
            Rect::from_min_max(pos2(x - 1.5, bottom - h), pos2(x + 1.5, bottom)),
            1.0,
            color,
        );
    }
}

/// Tombol pil hijau (call-to-action).
pub fn pill_button(ui: &mut Ui, text: &str) -> Response {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), bold(15.0), BLACK);
    let size = vec2(galley.size().x + 40.0, 44.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let scale = if resp.hovered() { 1.03 } else { 1.0 };
    let r = Rect::from_center_size(rect.center(), rect.size() * scale);
    ui.painter().rect_filled(
        r,
        22.0,
        if resp.hovered() { GREEN_HOVER } else { GREEN },
    );
    ui.painter().galley(
        r.center() - galley.size() / 2.0,
        galley,
        BLACK,
    );
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}
