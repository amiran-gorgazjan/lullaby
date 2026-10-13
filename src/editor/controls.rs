use super::theme::{CLAY, Canvas, INK, LINE, MUTED, SAGE};
use egui::{Align2, Color32, Rect, Response, Sense, Stroke, StrokeKind, Vec2};
use nice_plug::prelude::*;

pub fn button(
    ui: &mut egui::Ui,
    c: Canvas,
    rect: Rect,
    id: impl egui::AsIdSalt,
    label: &str,
    selected: bool,
) -> Response {
    let response = ui.interact(rect, ui.id().with(id), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    let p = ui.painter();
    if selected {
        p.rect_filled(rect, 7.0 * c.scale, CLAY);
        p.rect_stroke(
            rect,
            7.0 * c.scale,
            Stroke::new(c.scale, Color32::from_rgb(143, 94, 72)),
            StrokeKind::Inside,
        );
    } else if response.hovered() {
        p.rect_filled(rect, 7.0 * c.scale, Color32::from_black_alpha(8));
    }
    p.text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(21.0 * c.scale),
        if selected {
            Color32::from_rgb(255, 249, 237)
        } else {
            INK
        },
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn switch(
    ui: &mut egui::Ui,
    c: Canvas,
    rect: Rect,
    id: impl egui::AsIdSalt,
    on: bool,
    label: &str,
) -> Response {
    let response = ui.interact(rect.expand(5.0 * c.scale), ui.id().with(id), Sense::click());
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, on, label));
    let p = ui.painter();
    let radius = rect.height() * 0.5;
    p.rect_filled(
        rect,
        radius,
        if on {
            SAGE
        } else {
            Color32::from_rgb(166, 157, 142)
        },
    );
    p.rect_stroke(
        rect,
        radius,
        Stroke::new(
            c.scale,
            if on {
                Color32::from_rgb(87, 100, 76)
            } else {
                Color32::from_rgb(138, 129, 114)
            },
        ),
        StrokeKind::Inside,
    );
    let center = egui::pos2(
        if on {
            rect.right() - radius
        } else {
            rect.left() + radius
        },
        rect.center().y,
    );
    for i in (1..=5).rev() {
        p.circle_filled(
            center + Vec2::new(-c.scale * 1.5, c.scale * 3.0),
            radius * 0.87 + i as f32 * c.scale * 0.5,
            Color32::from_black_alpha(9),
        );
    }
    p.circle_filled(center, radius * 0.87, Color32::from_rgb(244, 237, 223));
    p.circle_stroke(
        center,
        radius * 0.87,
        Stroke::new(c.scale, Color32::from_rgb(128, 119, 102)),
    );
    p.circle_stroke(
        center - Vec2::splat(c.scale * 0.5),
        radius * 0.80,
        Stroke::new(c.scale, Color32::from_white_alpha(160)),
    );
    if response.has_focus() {
        p.rect_stroke(
            rect.expand(4.0 * c.scale),
            radius + 4.0 * c.scale,
            Stroke::new(c.scale, CLAY),
            StrokeKind::Outside,
        );
    }
    response
        .on_hover_text(label)
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn param_switch(
    ui: &mut egui::Ui,
    c: Canvas,
    rect: Rect,
    param: &BoolParam,
    setter: &ParamSetter,
    label: &str,
) {
    if switch(
        ui,
        c,
        rect,
        param as *const BoolParam as usize,
        param.value(),
        label,
    )
    .clicked()
    {
        super::set(setter, param, !param.value());
    }
}

pub fn chevron(
    ui: &mut egui::Ui,
    c: Canvas,
    x: f32,
    y: f32,
    right: bool,
    id: &str,
    label: &str,
) -> Response {
    let response = ui.interact(
        c.r(x - 20.0, y - 22.0, 40.0, 44.0),
        ui.id().with(id),
        Sense::click(),
    );
    let color = if response.hovered() { CLAY } else { INK };
    let d = if right { 1.0 } else { -1.0 };
    c.line(
        ui.painter(),
        [x - 4.0 * d, y - 8.0],
        [x + 4.0 * d, y],
        color,
        2.2,
    );
    c.line(
        ui.painter(),
        [x + 4.0 * d, y],
        [x - 4.0 * d, y + 8.0],
        color,
        2.2,
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    response
        .on_hover_text(label)
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn quiet_button(
    ui: &mut egui::Ui,
    c: Canvas,
    rect: Rect,
    id: &str,
    label: &str,
    active: bool,
) -> Response {
    let response = ui.interact(rect, ui.id().with(id), Sense::click());
    let p = ui.painter();
    p.text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(13.0 * c.scale),
        if active || response.hovered() {
            INK
        } else {
            MUTED
        },
    );
    if active {
        p.line_segment(
            [rect.left_bottom(), rect.right_bottom()],
            Stroke::new(c.scale, LINE),
        );
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
