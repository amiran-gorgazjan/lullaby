use super::theme::{CLAY, Canvas, INK, LINE, MUTED, SAGE};
use egui::epaint::{Mesh, Vertex, WHITE_UV};
use egui::{Align2, Color32, Pos2, Rect, Response, Sense, Stroke, TextureHandle, Vec2};
use nice_plug::prelude::*;
use std::f32::consts::{PI, TAU};

pub struct Knob<'a> {
    pub param: &'a FloatParam,
    pub center: [f32; 2],
    pub label: &'a str,
    pub limits: [&'a str; 2],
    pub value: String,
    pub percent: bool,
    pub clay: bool,
    pub large: bool,
}

/// The parameter gesture and all interaction modes are shared by every rotary control.
pub fn show(
    ui: &mut egui::Ui,
    c: Canvas,
    grain: &TextureHandle,
    setter: &ParamSetter,
    knob: Knob<'_>,
) -> Response {
    let center = c.p(knob.center[0], knob.center[1]);
    let radius = if knob.large { 126.0 } else { 52.0 } * c.scale;
    let hit = Rect::from_center_size(center, Vec2::splat(radius * 2.35));
    // Stable identities also keep egui's debug ID source cache bounded across
    // editor lifetimes; parameter heap addresses change between instances.
    let id = ui.id().with(("knob", knob.param.name()));
    let mut response = ui.interact(hit, id, Sense::click_and_drag());
    response.widget_info(|| egui::WidgetInfo::slider(true, knob.param.value() as f64, knob.label));
    let drag_id = id.with("drag");
    let mut drag = ui.data(|d| d.get_temp::<Drag>(drag_id));
    let press = ui
        .input(|i| {
            i.events.iter().find_map(|event| match event {
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    ..
                } if hit.contains(*pos) && ui.clip_rect().contains(*pos) => Some(*pos),
                _ => None,
            })
        })
        .filter(|pos| ui.ctx().layer_id_at(*pos) == Some(ui.layer_id()));
    if drag.is_none() && (response.is_pointer_button_down_on() || press.is_some()) {
        setter.begin_set_parameter(knob.param);
        response.request_focus();
        drag = press
            .or_else(|| response.interact_pointer_pos())
            .map(|position| Drag {
                value: knob.param.unmodulated_normalized_value(),
                position,
            });
    }
    if let Some(active) = &mut drag {
        // Include the release position: native hosts can coalesce the last motion
        // with mouse-up, when Response::dragged() is already false.
        if let Some(position) = ui.input(|i| i.pointer.interact_pos()) {
            let delta = position - active.position;
            let precision = if ui.input(|i| i.modifiers.shift) {
                0.12
            } else {
                1.0
            };
            active.value = (active.value + (delta.x - delta.y) / (c.scale * 240.0) * precision)
                .clamp(0.0, 1.0);
            active.position = position;
            if delta != Vec2::ZERO {
                setter.set_parameter_normalized(knob.param, active.value);
                response.mark_changed();
            }
        }
    }
    let normalized = drag.map_or(knob.param.unmodulated_normalized_value(), |d| d.value);
    if drag.is_some() && ui.input(|i| !i.pointer.any_down()) {
        setter.end_set_parameter(knob.param);
        drag = None;
    }
    ui.data_mut(|d| {
        if let Some(drag) = drag {
            d.insert_temp(drag_id, drag);
        } else {
            d.remove::<Drag>(drag_id);
        }
    });
    if response.double_clicked() || (response.clicked() && ui.input(|i| i.modifiers.command)) {
        super::set(setter, knob.param, knob.param.default_plain_value());
        response.mark_changed();
    }
    if response.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 {
            setter.begin_set_parameter(knob.param);
            setter.set_parameter_normalized(
                knob.param,
                (knob.param.unmodulated_normalized_value() + scroll * 0.001).clamp(0.0, 1.0),
            );
            setter.end_set_parameter(knob.param);
            response.mark_changed();
        }
    }
    if response.has_focus() {
        let delta = ui.input(|i| {
            let step = if i.modifiers.shift { 0.001 } else { 0.01 };
            (i.key_pressed(egui::Key::ArrowUp) || i.key_pressed(egui::Key::ArrowRight)) as i32
                as f32
                * step
                - (i.key_pressed(egui::Key::ArrowDown) || i.key_pressed(egui::Key::ArrowLeft))
                    as i32 as f32
                    * step
        });
        if delta != 0.0 {
            setter.begin_set_parameter(knob.param);
            setter.set_parameter_normalized(
                knob.param,
                (knob.param.unmodulated_normalized_value() + delta).clamp(0.0, 1.0),
            );
            setter.end_set_parameter(knob.param);
        }
    }
    let p = ui.painter();
    let value = if response.dragged() {
        normalized
    } else {
        knob.param.modulated_normalized_value()
    };
    let accent = if knob.clay { CLAY } else { SAGE };
    let arc_radius = if knob.large { 164.0 } else { 69.0 } * c.scale;
    let arc_width = if knob.large { 15.0 } else { 9.0 } * c.scale;
    arc(
        p,
        center + Vec2::new(0.0, c.scale),
        arc_radius,
        arc_width + c.scale * 2.0,
        1.0,
        LINE,
    );
    arc(
        p,
        center,
        arc_radius,
        arc_width,
        1.0,
        Color32::from_rgb(210, 199, 179),
    );
    arc(p, center, arc_radius, arc_width, value.max(0.0001), accent);
    if knob.large {
        for i in 0..=14 {
            let angle = angle(i as f32 / 14.0);
            let pos = center + Vec2::angled(angle) * (arc_radius + 22.0 * c.scale);
            p.circle_filled(
                pos + Vec2::new(0.0, c.scale),
                3.2 * c.scale,
                Color32::from_white_alpha(125),
            );
            p.circle_filled(pos, 2.6 * c.scale, LINE);
        }
    }
    // Soft contact shadow, then the dark lower rim and illuminated chamfer.
    for i in (1..=24).rev() {
        let spread = i as f32 * radius * 0.013;
        p.circle_filled(
            center + Vec2::new(-radius * 0.045, radius * 0.18),
            radius + spread,
            Color32::from_black_alpha((8.0 * (1.0 - i as f32 / 27.0)) as u8),
        );
    }
    disc(
        p,
        center + Vec2::new(0.0, radius * 0.027),
        radius * 1.012,
        [108, 95, 74],
        [155, 139, 113],
    );
    disc(p, center, radius, [248, 241, 224], [152, 135, 108]);
    // Fine ribbing catches light around the outside of the knob.
    for i in 0..240 {
        let a = i as f32 / 240.0 * TAU;
        let direction = Vec2::angled(a);
        let alpha = (10.0 + 35.0 * ((a - PI * 0.25).cos() * 0.5 + 0.5)) as u8;
        p.line_segment(
            [
                center + direction * radius * 0.965,
                center + direction * radius,
            ],
            Stroke::new(c.scale * 0.7, Color32::from_black_alpha(alpha)),
        );
    }
    disc(
        p,
        center - Vec2::new(0.0, radius * 0.025),
        radius * 0.953,
        [253, 245, 229],
        [200, 184, 158],
    );
    disc(p, center, radius * 0.914, [233, 223, 204], [218, 204, 180]);
    textured_disc(p, center, radius * 0.908, grain);
    p.circle_stroke(
        center - Vec2::new(0.0, radius * 0.016),
        radius * 0.94,
        Stroke::new(1.5 * c.scale, Color32::from_white_alpha(120)),
    );
    let direction = Vec2::angled(angle(value));
    p.line_segment(
        [
            center + direction * radius * 0.53 + Vec2::splat(c.scale),
            center + direction * radius * 0.91 + Vec2::splat(c.scale),
        ],
        Stroke::new(
            if knob.large { 8.0 } else { 5.0 } * c.scale,
            Color32::from_white_alpha(170),
        ),
    );
    p.line_segment(
        [
            center + direction * radius * 0.53,
            center + direction * radius * 0.91,
        ],
        Stroke::new(if knob.large { 6.0 } else { 3.5 } * c.scale, accent),
    );
    if response.has_focus() {
        p.circle_stroke(
            center,
            arc_radius + 7.0 * c.scale,
            Stroke::new(c.scale, accent.gamma_multiply(0.45)),
        );
    }
    let [x, y] = knob.center;
    let (limit_y, limit_dx) = if knob.large {
        (y + 142.0, 132.0)
    } else {
        (y + 62.0, 66.0)
    };
    c.text(
        p,
        x - limit_dx,
        limit_y,
        if knob.large { 23.0 } else { 18.0 },
        knob.limits[0],
        MUTED,
        Align2::CENTER_CENTER,
    );
    c.text(
        p,
        x + limit_dx,
        limit_y,
        if knob.large { 23.0 } else { 18.0 },
        knob.limits[1],
        MUTED,
        Align2::CENTER_CENTER,
    );
    c.caps(
        p,
        x,
        if knob.large { y + 183.0 } else { y - 96.0 },
        if knob.large { 31.0 } else { 20.0 },
        if knob.large { 4.0 } else { 2.0 },
        knob.label,
        INK,
    );
    let value_rect = c.r(
        x - 92.0,
        if knob.large { y + 208.0 } else { y + 76.0 },
        184.0,
        if knob.large { 46.0 } else { 37.0 },
    );
    let value_response = ui.interact(value_rect, id.with("value"), Sense::click());
    let mut editing = ui
        .data(|d| d.get_temp::<bool>(id.with("editing")))
        .unwrap_or(false);
    let just_opened = value_response.clicked();
    if just_opened {
        editing = true;
        ui.data_mut(|d| {
            d.insert_temp(id.with("editing"), true);
            d.insert_temp(
                id.with("text"),
                if knob.percent {
                    format!("{:.2} %", knob.param.value() * 100.0)
                } else {
                    knob.param.to_string()
                },
            );
        });
    }
    if editing {
        let mut text = ui
            .data(|d| d.get_temp::<String>(id.with("text")))
            .unwrap_or_default();
        let edit = ui.put(
            value_rect,
            egui::TextEdit::singleline(&mut text)
                .horizontal_align(egui::Align::Center)
                .font(egui::FontId::proportional(21.0 * c.scale)),
        );
        if just_opened {
            edit.request_focus();
        }
        let commit = ui.input(|i| i.key_pressed(egui::Key::Enter)) || edit.lost_focus();
        let cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));
        let parsed = if knob.percent {
            text.trim()
                .trim_end_matches('%')
                .trim()
                .parse::<f32>()
                .ok()
                .map(|value| knob.param.preview_normalized(value * 0.01))
        } else {
            knob.param.string_to_normalized_value(&text)
        };
        if commit
            && !cancel
            && let Some(value) = parsed.filter(|value| value.is_finite())
        {
            setter.begin_set_parameter(knob.param);
            setter.set_parameter_normalized(knob.param, value);
            setter.end_set_parameter(knob.param);
        }
        if commit || cancel {
            edit.surrender_focus();
        }
        ui.data_mut(|d| {
            d.insert_temp(id.with("text"), text);
            if commit || cancel {
                d.insert_temp(id.with("editing"), false);
            }
        });
    } else {
        c.text(
            p,
            x,
            if knob.large { y + 230.0 } else { y + 96.0 },
            if knob.large { 34.0 } else { 23.0 },
            &knob.value,
            INK,
            Align2::CENTER_CENTER,
        );
    }
    value_response.on_hover_text("Click to enter a value");
    response.on_hover_text(format!("{} · {}\nDrag up or right to increase · Shift for fine adjustment\nDouble-click to reset · Scroll or use arrow keys", knob.param.name(), knob.param))
}

#[derive(Clone, Copy)]
struct Drag {
    value: f32,
    position: Pos2,
}

fn angle(value: f32) -> f32 {
    (135.0 + value.clamp(0.0, 1.0) * 270.0).to_radians()
}

fn arc(p: &egui::Painter, center: Pos2, radius: f32, width: f32, value: f32, color: Color32) {
    let segments = (120.0 * value).ceil().max(1.0) as u32;
    let mut mesh = Mesh::default();
    for i in 0..=segments {
        let direction = Vec2::angled(angle(i as f32 / segments as f32 * value));
        for r in [radius - width * 0.5, radius + width * 0.5] {
            mesh.vertices.push(Vertex {
                pos: center + direction * r,
                uv: WHITE_UV,
                color,
            });
        }
        if i < segments {
            let v = i * 2;
            mesh.indices.extend([v, v + 1, v + 2, v + 1, v + 3, v + 2]);
        }
    }
    p.add(mesh);
}

fn disc(p: &egui::Painter, center: Pos2, radius: f32, light: [u8; 3], dark: [u8; 3]) {
    let mut mesh = Mesh::default();
    mesh.vertices.push(Vertex {
        pos: center,
        uv: WHITE_UV,
        color: Color32::from_rgb(light[0], light[1], light[2]),
    });
    for i in 0..=128 {
        let a = i as f32 / 128.0 * TAU;
        let shade = ((a - PI * 1.25).cos() * 0.5 + 0.5).powf(0.8);
        let color = std::array::from_fn::<_, 3, _>(|c| {
            (dark[c] as f32 + (light[c] as f32 - dark[c] as f32) * shade) as u8
        });
        mesh.vertices.push(Vertex {
            pos: center + Vec2::angled(a) * radius,
            uv: WHITE_UV,
            color: Color32::from_rgb(color[0], color[1], color[2]),
        });
        if i < 128 {
            mesh.indices.extend([0, i + 1, i + 2]);
        }
    }
    p.add(mesh);
}

fn textured_disc(p: &egui::Painter, center: Pos2, radius: f32, grain: &TextureHandle) {
    let mut mesh = Mesh::with_texture(grain.id());
    mesh.vertices.push(Vertex {
        pos: center,
        uv: Pos2::new(0.5, 0.5),
        color: Color32::WHITE,
    });
    for i in 0..=96 {
        let d = Vec2::angled(i as f32 / 96.0 * TAU);
        mesh.vertices.push(Vertex {
            pos: center + d * radius,
            uv: Pos2::new(0.5, 0.5) + d * 0.5,
            color: Color32::WHITE,
        });
        if i < 96 {
            mesh.indices.extend([0, i + 1, i + 2]);
        }
    }
    p.add(mesh);
}
