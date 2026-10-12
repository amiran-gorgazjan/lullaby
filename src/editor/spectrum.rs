use super::{
    LullabyEditor, set,
    theme::{CLAY, Canvas, INK, LINE, MUTED, SAGE},
};
use egui::epaint::{Mesh, Vertex, WHITE_UV};
use egui::{Align2, Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use lullaby_dsp::{METER_BINS, gain_to_db};
use nice_plug::prelude::*;

impl LullabyEditor {
    pub(super) fn spectrum(&mut self, ui: &mut egui::Ui, c: Canvas, setter: &ParamSetter) {
        let outer = c.r(470.0, 170.0, 1035.0, 466.0);
        let plot = c.r(470.0, 231.0, 972.0, 351.0);
        let p = ui.painter_at(outer);
        p.rect_filled(outer, 12.0 * c.scale, Color32::from_black_alpha(3));
        p.rect_stroke(
            outer,
            12.0 * c.scale,
            Stroke::new(c.scale, LINE),
            StrokeKind::Inside,
        );
        p.rect_stroke(
            outer.shrink(c.scale),
            11.0 * c.scale,
            Stroke::new(c.scale, Color32::from_white_alpha(100)),
            StrokeKind::Inside,
        );
        let settings = self.params.snapshot();
        for (x, color, label) in [
            (
                514.0,
                Color32::from_rgb(157, 153, 140),
                if settings.external_sidechain {
                    "Detector"
                } else {
                    "Input"
                },
            ),
            (714.0, CLAY, "Suppression"),
            (937.0, SAGE, "Filtered"),
        ] {
            if label == "Suppression" {
                p.rect_filled(
                    c.r(x, 196.0, 42.0, 13.0),
                    6.0 * c.scale,
                    color.gamma_multiply(0.8),
                );
            } else {
                c.line(&p, [x, 202.0], [x + 47.0, 202.0], color, 2.2);
            }
            c.text(&p, x + 61.0, 202.0, 20.0, label, MUTED, Align2::LEFT_CENTER);
            ui.interact(
                c.r(x, 180.0, 190.0, 44.0),
                ui.id().with(("legend", label)),
                Sense::hover(),
            )
            .on_hover_text(match label {
                "Filtered" => "Estimated detector spectrum after displayed suppression. Mix is reflected; wet trim, output gain, bypass and monitoring are not. With external sidechain enabled, this shows the filtered key signal.",
                "Suppression" => "Average gain reduction across both processing channels.",
                _ => "Detector spectrum. This follows the external key when sidechain detection is enabled.",
            });
        }
        let scale_response = ui.interact(
            c.r(1365.0, 181.0, 126.0, 40.0),
            ui.id().with("graph_scale"),
            Sense::click(),
        );
        c.text(
            &p,
            1442.0,
            202.0,
            19.0,
            &format!("−{} dB", self.graph_floor as i32),
            MUTED,
            Align2::RIGHT_CENTER,
        );
        c.line(&p, [1455.0, 199.0], [1461.0, 205.0], MUTED, 1.5);
        c.line(&p, [1461.0, 205.0], [1467.0, 199.0], MUTED, 1.5);
        egui::Popup::menu(&scale_response).show(|ui| {
            ui.label("Spectrum floor");
            for floor in [36.0, 60.0, 84.0] {
                if ui
                    .selectable_label(self.graph_floor == floor, format!("−{floor:.0} dB"))
                    .clicked()
                {
                    self.graph_floor = floor;
                    ui.close();
                }
            }
        });
        let x = |f: f32| plot.left() + (f / 20.0).log10() / 3.0 * plot.width();
        let y = |db: f32| {
            plot.top() + ((12.0 - db) / (self.graph_floor + 12.0)).clamp(0.0, 1.0) * plot.height()
        };
        for decade in [10.0, 100.0, 1000.0, 10000.0] {
            for multiple in 1..10 {
                let f = decade * multiple as f32;
                if !(20.0..=20000.0).contains(&f) {
                    continue;
                }
                p.line_segment(
                    [Pos2::new(x(f), plot.top()), Pos2::new(x(f), plot.bottom())],
                    Stroke::new(
                        c.scale * 0.65,
                        Color32::from_black_alpha(if multiple == 1 { 30 } else { 15 }),
                    ),
                );
            }
        }
        let step = (self.graph_floor + 12.0) / 4.0;
        for i in 0..=4 {
            let db = 12.0 - i as f32 * step;
            p.line_segment(
                [
                    Pos2::new(plot.left(), y(db)),
                    Pos2::new(plot.right() + 8.0 * c.scale, y(db)),
                ],
                Stroke::new(c.scale * 0.7, Color32::from_black_alpha(35)),
            );
            p.text(
                Pos2::new(plot.right() + 18.0 * c.scale, y(db)),
                Align2::LEFT_CENTER,
                if db > 0.0 {
                    format!("+{db:.0}")
                } else {
                    format!("{db:.0}")
                },
                egui::FontId::proportional(17.0 * c.scale),
                MUTED,
            );
        }
        for (f, label) in [
            (25.0, "20"),
            (100.0, "100"),
            (1000.0, "1k"),
            (10000.0, "10k"),
            (20000.0, "20k"),
        ] {
            p.text(
                Pos2::new(x(f), plot.bottom() + 24.0 * c.scale),
                Align2::CENTER_CENTER,
                label,
                egui::FontId::proportional(20.0 * c.scale),
                MUTED,
            );
        }
        let frame = self.meters.read();
        c.text(
            &p,
            1232.0,
            202.0,
            16.0,
            &format!("PEAK −{:.1} dB", frame.peak_reduction_db),
            MUTED,
            Align2::CENTER_CENTER,
        );
        // Display actual detector magnitude and its estimated post-reduction spectrum.
        // No decorative signal is drawn when the host is silent.
        let smooth = 0.22;
        for i in 0..METER_BINS {
            self.display_input[i] += (frame.input_db[i] - self.display_input[i]) * smooth;
            let reduction = (frame.reduction_db[0][i] + frame.reduction_db[1][i]) * 0.5;
            self.display_reduction[i] += (reduction - self.display_reduction[i]) * smooth;
        }
        let input: Vec<_> = (0..METER_BINS)
            .map(|i| {
                Pos2::new(
                    plot.left() + i as f32 / (METER_BINS - 1) as f32 * plot.width(),
                    y(self.display_input[i]),
                )
            })
            .collect();
        let output: Vec<_> = (0..METER_BINS)
            .map(|i| {
                Pos2::new(
                    input[i].x,
                    y(self.display_input[i] - self.display_reduction[i]),
                )
            })
            .collect();
        let clipped = p.with_clip_rect(plot);
        fill_between(
            &clipped,
            &input,
            &vec![Pos2::new(0.0, plot.bottom()); METER_BINS],
            Color32::from_rgba_unmultiplied(146, 142, 125, 35),
        );
        fill_between(
            &clipped,
            &output,
            &vec![Pos2::new(0.0, plot.bottom()); METER_BINS],
            Color32::from_rgba_unmultiplied(113, 123, 96, 38),
        );
        fill_between(
            &clipped,
            &input,
            &output,
            Color32::from_rgba_unmultiplied(181, 130, 103, 135),
        );
        clipped.add(egui::Shape::line(
            input,
            Stroke::new(c.scale, Color32::from_rgb(163, 158, 144)),
        ));
        clipped.add(egui::Shape::line(output, Stroke::new(c.scale * 2.0, SAGE)));
        let response = ui.interact(plot, ui.id().with("spectrum"), Sense::click());
        response.context_menu(|ui| {
            ui.checkbox(&mut self.channel_curves, "Show channel reduction curves");
            ui.label("Sage: left/mid · clay: right/side");
            ui.separator();
            if ui.button("Edit spectral bands…").clicked() {
                self.bands_panel = true;
                ui.close();
            }
            if ui.button("Processing settings…").clicked() {
                self.settings_panel = true;
                ui.close();
            }
        });
        if self.channel_curves {
            for (channel, color) in [(0, SAGE), (1, CLAY)] {
                let points = (0..METER_BINS)
                    .map(|i| {
                        Pos2::new(
                            plot.left() + i as f32 / (METER_BINS - 1) as f32 * plot.width(),
                            y(-frame.reduction_db[channel][i]),
                        )
                    })
                    .collect();
                clipped.add(egui::Shape::line(points, Stroke::new(c.scale * 1.5, color)));
            }
        }
        if self.bands_panel || settings.bands.iter().any(|band| band.enabled) {
            let weight_y = |db: f32| plot.center().y - db / 24.0 * plot.height() * 0.42;
            for (channel, color) in [(0, CLAY), (1, SAGE.gamma_multiply(0.7))] {
                let curve = (0..METER_BINS)
                    .map(|i| {
                        let f = 20.0 * 1000.0_f32.powf(i as f32 / (METER_BINS - 1) as f32);
                        Pos2::new(
                            x(f),
                            weight_y(gain_to_db(settings.weight(f, channel)).clamp(-24.0, 24.0)),
                        )
                    })
                    .collect();
                clipped.add(egui::Shape::line(curve, Stroke::new(c.scale * 1.3, color)));
            }
            let mut hit_node = false;
            for (i, band) in settings.bands.iter().enumerate().filter(|(_, b)| b.enabled) {
                let position = Pos2::new(x(band.frequency), weight_y(band.depth_db));
                let node = ui.interact(
                    Rect::from_center_size(position, Vec2::splat(24.0 * c.scale)),
                    ui.id().with(("band", i)),
                    Sense::click_and_drag(),
                );
                hit_node |= node.hovered() || node.dragged();
                if node.clicked() || node.drag_started() {
                    self.selected = i;
                    self.bands_panel = true;
                }
                let param = &self.params.bands[i];
                if node.drag_started() {
                    setter.begin_set_parameter(&param.frequency);
                    setter.begin_set_parameter(&param.depth_db);
                }
                if node.dragged()
                    && let Some(pos) = node.interact_pointer_pos()
                {
                    setter.set_parameter(&param.frequency, frequency_at(plot, pos.x));
                    setter.set_parameter(
                        &param.depth_db,
                        ((plot.center().y - pos.y) / (plot.height() * 0.42) * 24.0)
                            .clamp(-24.0, 24.0),
                    );
                }
                if node.drag_stopped() {
                    setter.end_set_parameter(&param.frequency);
                    setter.end_set_parameter(&param.depth_db);
                }
                if node.double_clicked() {
                    set(setter, &param.enabled, false);
                    if self.params.listen_band.value() == i as i32 {
                        set(setter, &self.params.listen_band, -1);
                    }
                }
                if node.hovered() {
                    let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                    if scroll != 0.0 {
                        set(
                            setter,
                            &param.width,
                            (band.width + scroll * 0.005).clamp(0.1, 4.0),
                        );
                    }
                }
                clipped.circle_filled(
                    position,
                    if self.selected == i { 7.0 } else { 5.0 } * c.scale,
                    CLAY,
                );
                clipped.text(
                    position + Vec2::new(0.0, -17.0 * c.scale),
                    Align2::CENTER_CENTER,
                    (i + 1).to_string(),
                    egui::FontId::proportional(14.0 * c.scale),
                    INK,
                );
            }
            if response.double_clicked()
                && !hit_node
                && let Some(pos) = response.interact_pointer_pos()
                && let Some(i) = settings.bands.iter().position(|b| !b.enabled)
            {
                let db =
                    ((plot.center().y - pos.y) / (plot.height() * 0.42) * 24.0).clamp(-24.0, 24.0);
                self.params.bands[i].apply(
                    lullaby_dsp::Band::bell(frequency_at(plot, pos.x), db, 1.0),
                    setter,
                );
                self.selected = i;
            }
        } else if response.double_clicked() {
            self.bands_panel = true;
            if let Some(pos) = response.interact_pointer_pos()
                && let Some(i) = settings.bands.iter().position(|b| !b.enabled)
            {
                let db =
                    ((plot.center().y - pos.y) / (plot.height() * 0.42) * 24.0).clamp(-24.0, 24.0);
                self.params.bands[i].apply(
                    lullaby_dsp::Band::bell(frequency_at(plot, pos.x), db, 1.0),
                    setter,
                );
                self.selected = i;
            }
        }
    }
}

fn frequency_at(plot: Rect, x: f32) -> f32 {
    20.0 * 1000.0_f32.powf(((x - plot.left()) / plot.width()).clamp(0.0, 1.0))
}

fn fill_between(p: &egui::Painter, top: &[Pos2], bottom: &[Pos2], color: Color32) {
    let mut mesh = Mesh::default();
    for (i, (a, b)) in top.iter().zip(bottom).enumerate() {
        mesh.vertices.push(Vertex {
            pos: *a,
            uv: WHITE_UV,
            color,
        });
        mesh.vertices.push(Vertex {
            pos: Pos2::new(a.x, b.y),
            uv: WHITE_UV,
            color,
        });
        if i + 1 < top.len() {
            let v = i as u32 * 2;
            mesh.indices.extend([v, v + 1, v + 2, v + 1, v + 3, v + 2]);
        }
    }
    p.add(mesh);
}
