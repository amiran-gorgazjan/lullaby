use egui::epaint::{
    Mesh, Vertex, WHITE_UV,
    text::{FontTweak, LayoutJob, TextFormat, VariationCoords},
};
use egui::{
    Align2, Color32, FontData, FontDefinitions, FontFamily, FontId, Painter, Pos2, Rect, Stroke,
    TextureHandle, Vec2, pos2,
};
use std::sync::Arc;

pub const SIZE: Vec2 = Vec2::new(1536.0, 1024.0);
pub const INK: Color32 = Color32::from_rgb(48, 42, 34);
pub const MUTED: Color32 = Color32::from_rgb(105, 95, 80);
pub const LINE: Color32 = Color32::from_rgb(177, 160, 138);
pub const PAPER: Color32 = Color32::from_rgb(232, 223, 208);
pub const SAGE: Color32 = Color32::from_rgb(104, 116, 91);
pub const CLAY: Color32 = Color32::from_rgb(181, 130, 103);

/// All geometry lives in the reference's 1536×1024 coordinate system.
/// Fitting the canvas preserves spacing and circular controls on every host size.
#[derive(Clone, Copy)]
pub struct Canvas {
    pub origin: Pos2,
    pub scale: f32,
}
impl Canvas {
    pub fn fit(rect: Rect) -> Self {
        let scale = (rect.width() / SIZE.x).min(rect.height() / SIZE.y);
        Self {
            origin: rect.center() - SIZE * scale * 0.5,
            scale,
        }
    }
    pub fn p(self, x: f32, y: f32) -> Pos2 {
        self.origin + Vec2::new(x, y) * self.scale
    }
    pub fn r(self, x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::from_min_size(self.p(x, y), Vec2::new(w, h) * self.scale)
    }
    #[allow(clippy::too_many_arguments)] // Geometry and typography stay explicit at each draw site.
    pub fn text(
        self,
        p: &Painter,
        x: f32,
        y: f32,
        size: f32,
        text: &str,
        color: Color32,
        anchor: Align2,
    ) {
        p.text(
            self.p(x, y),
            anchor,
            text,
            FontId::proportional(size * self.scale),
            color,
        );
    }
    #[allow(clippy::too_many_arguments)]
    pub fn caps(
        self,
        p: &Painter,
        x: f32,
        y: f32,
        size: f32,
        spacing: f32,
        text: &str,
        color: Color32,
    ) {
        let job = LayoutJob::single_section(
            text.to_owned(),
            TextFormat {
                font_id: FontId::proportional(size * self.scale),
                color,
                extra_letter_spacing: spacing * self.scale,
                ..Default::default()
            },
        );
        let galley = p.layout_job(job);
        p.galley(self.p(x, y) - galley.size() * 0.5, galley, color);
    }
    pub fn line(self, p: &Painter, a: [f32; 2], b: [f32; 2], color: Color32, width: f32) {
        p.line_segment(
            [self.p(a[0], a[1]), self.p(b[0], b[1])],
            Stroke::new(width * self.scale, color),
        );
    }
}

pub fn install(ctx: &egui::Context) -> TextureHandle {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "Lato".into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../../assets/fonts/Lato-Regular.ttf"
        ))),
    );
    fonts.font_data.insert(
        "Cormorant".into(),
        Arc::new(
            FontData::from_static(include_bytes!("../../assets/fonts/CormorantGaramond.ttf"))
                .tweak(FontTweak {
                    coords: VariationCoords::new([(*b"wght", 650.0)]),
                    ..Default::default()
                }),
        ),
    );
    fonts
        .families
        .get_mut(&FontFamily::Proportional)
        .unwrap()
        .insert(0, "Lato".into());
    fonts.families.insert(
        FontFamily::Name("wordmark".into()),
        vec!["Cormorant".into()],
    );
    ctx.set_fonts(fonts);
    ctx.set_theme(egui::Theme::Light);
    let mut style = (*ctx.style_of(egui::Theme::Light)).clone();
    style.visuals = egui::Visuals::light();
    style.visuals.panel_fill = PAPER;
    style.visuals.window_fill = Color32::from_rgb(239, 232, 219);
    style.visuals.override_text_color = Some(INK);
    style.visuals.selection.bg_fill = CLAY.gamma_multiply(0.25);
    style.visuals.selection.stroke = Stroke::new(1.0, CLAY);
    style.visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(222, 211, 194);
    style.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(214, 200, 179);
    style.visuals.widgets.active.weak_bg_fill = Color32::from_rgb(201, 184, 159);
    style.spacing.item_spacing = Vec2::new(10.0, 10.0);
    style.spacing.button_padding = Vec2::new(10.0, 6.0);
    ctx.set_style_of(egui::Theme::Light, style);
    // Deterministic, seamless paper grain. Generated once, never on the audio thread.
    let mut seed = 0x43e19ab7_u32;
    let pixels = (0..256 * 256)
        .map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            if seed & 1 == 0 {
                Color32::from_black_alpha((seed % 15) as u8)
            } else {
                Color32::from_white_alpha((seed % 29) as u8)
            }
        })
        .collect();
    ctx.load_texture(
        "Lullaby paper grain",
        egui::ColorImage::new([256, 256], pixels),
        egui::TextureOptions {
            wrap_mode: egui::TextureWrapMode::Repeat,
            ..egui::TextureOptions::LINEAR
        },
    )
}

pub fn background(p: &Painter, canvas: Canvas, grain: &TextureHandle, rect: Rect) {
    p.rect_filled(rect, 0.0, PAPER);
    let mut mesh = Mesh::default();
    for (pos, color) in [
        (rect.left_top(), Color32::from_rgb(236, 229, 216)),
        (rect.right_top(), Color32::from_rgb(238, 231, 218)),
        (rect.right_bottom(), Color32::from_rgb(223, 212, 194)),
        (rect.left_bottom(), Color32::from_rgb(229, 218, 201)),
    ] {
        mesh.vertices.push(Vertex {
            pos,
            uv: WHITE_UV,
            color,
        });
    }
    mesh.indices.extend([0, 1, 2, 0, 2, 3]);
    p.add(mesh);
    p.image(
        grain.id(),
        rect,
        Rect::from_min_max(
            Pos2::ZERO,
            pos2(
                rect.width() / (256.0 * canvas.scale),
                rect.height() / (256.0 * canvas.scale),
            ),
        ),
        Color32::WHITE,
    );
}

pub fn logo(p: &Painter, c: Canvas, x: f32, y: f32, scale: f32, monochrome: bool) {
    // Three interwoven ribbons, drawn as filled strips rather than a baked image.
    for (row, color) in [
        (0.0, SAGE),
        (17.0, CLAY),
        (31.0, Color32::from_rgb(166, 147, 119)),
    ] {
        let mut mesh = Mesh::default();
        for i in 0..=64 {
            let t = i as f32 / 64.0;
            let wave = (t * std::f32::consts::TAU + row * 0.075).sin();
            let middle = y + (row + wave * 14.0) * scale;
            let width = (std::f32::consts::PI * t).sin() * 21.0 * scale;
            let color = if monochrome {
                Color32::from_rgb(177, 157, 129)
            } else {
                color
            };
            for offset in [-width * 0.5, width * 0.5] {
                mesh.vertices.push(Vertex {
                    pos: c.p(x + t * 120.0 * scale, middle + offset),
                    uv: WHITE_UV,
                    color: color.gamma_multiply(0.8),
                });
            }
            if i < 64 {
                let v = i * 2;
                mesh.indices.extend([v, v + 1, v + 2, v + 1, v + 3, v + 2]);
            }
        }
        p.add(mesh);
    }
}
