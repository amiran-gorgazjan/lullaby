use crate::{
    Meters,
    params::{LullabyParams, set},
};
use egui::{Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use lullaby_dsp::{FACTORY_PRESETS, METER_BINS, Settings, factory_preset, gain_to_db};
use nice_plug::{context::gui::GuiContext, prelude::*};
use nice_plug_egui::{
    Frame, NiceEguiApp, baseview, resizable_window::ResizableWindow, widgets::ParamSlider,
};
use std::{
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

const MINT: Color32 = Color32::from_rgb(142, 223, 193);
const GOLD: Color32 = Color32::from_rgb(230, 191, 128);
const MUTED: Color32 = Color32::from_rgb(133, 154, 163);
const PANEL: Color32 = Color32::from_rgb(21, 33, 42);

pub struct LullabyEditor {
    params: Arc<LullabyParams>,
    meters: Arc<Meters>,
    context: Option<GuiContext>,
    selected: usize,
    preset: String,
    preset_path: String,
    file_panel: bool,
    status: String,
    ab: [Settings; 2],
    active_ab: usize,
    undo: Vec<Settings>,
    redo: Vec<Settings>,
    last: Settings,
    restoring: Option<Settings>,
    quality_lock: bool,
}

impl LullabyEditor {
    pub(crate) fn new(params: Arc<LullabyParams>, meters: Arc<Meters>) -> Self {
        let current = params.snapshot();
        Self {
            params,
            meters,
            context: None,
            selected: 0,
            preset: "Custom / session".into(),
            preset_path: String::new(),
            file_panel: false,
            status: String::new(),
            ab: [current; 2],
            active_ab: 0,
            undo: Vec::new(),
            redo: Vec::new(),
            last: current,
            restoring: None,
            quality_lock: true,
        }
    }

    fn load(&mut self, mut settings: Settings, setter: &ParamSetter) {
        if self.quality_lock {
            settings.quality = self.params.snapshot().quality;
        }
        self.params.apply(settings, setter);
    }

    fn draw(&mut self, ui: &mut egui::Ui, setter: &ParamSetter) {
        let params = self.params.clone();
        ui.spacing_mut().item_spacing = Vec2::new(10.0, 8.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("LULLABY").size(27.0).strong().color(MINT));
            ui.add_space(8.0);
            ui.label(
                RichText::new("ADAPTIVE RESONANCE SUPPRESSOR")
                    .size(10.0)
                    .color(MUTED),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                toggle(ui, &params.bypass, setter, "Bypass");
                ui.label(RichText::new(env!("CARGO_PKG_VERSION")).color(MUTED));
            });
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("presets")
                .selected_text(&self.preset)
                .width(180.0)
                .show_ui(ui, |ui| {
                    for (i, name) in FACTORY_PRESETS.iter().enumerate() {
                        if ui.selectable_label(self.preset == *name, *name).clicked() {
                            self.load(factory_preset(i), setter);
                            self.preset = name.to_string();
                        }
                    }
                });
            if ui.button("Preset file…").clicked() {
                self.file_panel = !self.file_panel;
            }
            ui.separator();
            for (i, name) in ["A", "B"].iter().enumerate() {
                if ui.selectable_label(self.active_ab == i, *name).clicked() && self.active_ab != i
                {
                    self.ab[self.active_ab] = params.snapshot();
                    self.active_ab = i;
                    self.load(self.ab[i], setter);
                }
            }
            if ui
                .button("Copy A/B")
                .on_hover_text("Copy current settings to the other comparison slot")
                .clicked()
            {
                self.ab[1 - self.active_ab] = params.snapshot();
            }
            ui.separator();
            if ui
                .add_enabled(!self.undo.is_empty(), egui::Button::new("Undo"))
                .clicked()
            {
                self.redo.push(params.snapshot());
                if let Some(s) = self.undo.pop() {
                    params.apply(s, setter);
                    self.restoring = Some(s);
                }
            }
            if ui
                .add_enabled(!self.redo.is_empty(), egui::Button::new("Redo"))
                .clicked()
            {
                self.undo.push(params.snapshot());
                if let Some(s) = self.redo.pop() {
                    params.apply(s, setter);
                    self.restoring = Some(s);
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                toggle(ui, &params.sidechain_listen, setter, "SC listen");
                toggle(ui, &params.external_sidechain, setter, "External SC");
            });
        });
        if self.file_panel {
            egui::Frame::new()
                .fill(PANEL)
                .inner_margin(10)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Preset path");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.preset_path)
                                .hint_text("Absolute path to a .json preset")
                                .desired_width(510.0),
                        );
                        if ui.button("Save").clicked() {
                            self.status = if !std::path::Path::new(&self.preset_path).is_absolute()
                            {
                                "Enter an absolute file path.".into()
                            } else {
                                serde_json::to_string_pretty(&params.snapshot())
                                    .map_err(|e| e.to_string())
                                    .and_then(|data| {
                                        std::fs::write(&self.preset_path, data)
                                            .map_err(|e| e.to_string())
                                    })
                                    .map(|_| "Preset saved.".to_owned())
                                    .unwrap_or_else(|e| e)
                            };
                        }
                        if ui.button("Load").clicked() {
                            match std::fs::read_to_string(&self.preset_path)
                                .map_err(|e| e.to_string())
                                .and_then(|data| {
                                    serde_json::from_str::<Settings>(&data)
                                        .map_err(|e| e.to_string())
                                }) {
                                Ok(s) => {
                                    self.load(s, setter);
                                    self.preset = "User preset".into();
                                    self.status = "Preset loaded.".into();
                                }
                                Err(e) => self.status = e,
                            }
                        }
                    });
                    if !self.status.is_empty() {
                        ui.label(&self.status);
                    }
                });
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("FREQUENCY WEIGHTING").small().color(MUTED));
            ui.label(
                RichText::new("Boost the curve to suppress more. Double-click to add a band.")
                    .small()
                    .color(MUTED),
            );
        });
        self.graph(ui, setter);
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("BAND {}", self.selected + 1)).color(GOLD));
            let band = &params.bands[self.selected];
            toggle(ui, &band.enabled, setter, "Enabled");
            choice(
                ui,
                &band.shape,
                setter,
                &[
                    "Bell",
                    "Low shelf",
                    "High shelf",
                    "Low cut",
                    "High cut",
                    "Bandpass",
                    "Reject",
                    "Tilt",
                ],
            );
            compact(ui, &band.frequency, setter, 108.0);
            compact(ui, &band.depth_db, setter, 95.0);
            compact(ui, &band.width, setter, 85.0);
            compact(ui, &band.focus, setter, 75.0);
            let listen = params.listen_band.value() == self.selected as i32;
            if ui.selectable_label(listen, "Audition Δ").clicked() {
                set(
                    setter,
                    &params.listen_band,
                    if listen { -1 } else { self.selected as i32 },
                );
            }
            if ui.button("Remove").clicked() {
                set(setter, &band.enabled, false);
                if listen {
                    set(setter, &params.listen_band, -1);
                }
            }
        });
        ui.add_space(8.0);
        ui.columns(3, |cols| {
            card(&mut cols[0], "SUPPRESSION", |ui| {
                ui.horizontal(|ui| {
                    choice(
                        ui,
                        &params.mode,
                        setter,
                        &["Soft · adaptive", "Hard · level dependent"],
                    );
                });
                for p in [&params.depth, &params.detail, &params.max_cut_db] {
                    slider(ui, p, setter);
                }
            });
            card(&mut cols[1], "DYNAMICS", |ui| {
                for p in [&params.attack_ms, &params.release_ms] {
                    slider(ui, p, setter);
                }
                ui.label(
                    RichText::new("Timing follows frequency. Use tilts below to refine it.")
                        .small()
                        .color(MUTED),
                );
            });
            card(&mut cols[2], "STEREO", |ui| {
                choice(
                    ui,
                    &params.stereo_mode,
                    setter,
                    &["Left / right", "Mid / side"],
                );
                for p in [&params.link, &params.focus] {
                    slider(ui, p, setter);
                }
                ui.label(
                    RichText::new("Focus: −1 left/mid  ·  +1 right/side")
                        .small()
                        .color(MUTED),
                );
            });
        });
        ui.add_space(6.0);
        ui.collapsing("Frequency tilts", |ui| {
            ui.columns(3, |cols| {
                for p in [&params.detail_low, &params.detail_high] {
                    slider(&mut cols[0], p, setter);
                }
                for p in [&params.attack_low, &params.attack_high] {
                    slider(&mut cols[1], p, setter);
                }
                for p in [&params.release_low, &params.release_high] {
                    slider(&mut cols[2], p, setter);
                }
            });
        });
        ui.separator();
        ui.horizontal(|ui| {
            toggle(ui, &params.delta, setter, "Delta Δ");
            compact(ui, &params.mix, setter, 140.0);
            compact(ui, &params.wet_trim_db, setter, 140.0);
            compact(ui, &params.output_db, setter, 140.0);
            ui.separator();
            ui.vertical(|ui| {
                choice(
                    ui,
                    &params.quality,
                    setter,
                    &["Normal", "High", "Ultra", "Low latency"],
                );
                ui.checkbox(&mut self.quality_lock, "Lock for presets");
            });
            ui.vertical(|ui| {
                choice(
                    ui,
                    &params.phase,
                    setter,
                    &["Minimum phase", "Linear phase"],
                );
                ui.label(
                    RichText::new(format!(
                        "{} samples latency",
                        self.meters.latency.load(Ordering::Relaxed)
                    ))
                    .small()
                    .color(MUTED),
                );
            });
        });
        ui.add_space(6.0);
        ui.label(
            RichText::new(
                "Independent spectral DSP  ·  Eight bands  ·  Mono / stereo  ·  VST3 + CLAP",
            )
            .small()
            .color(MUTED),
        );
        let current = params.snapshot();
        if let Some(target) = self.restoring {
            // Host parameter events are asynchronous. Do not record a restoration
            // itself as a fresh edit (which would destroy the redo stack).
            if settings_close(current, target) {
                self.last = current;
                self.restoring = None;
            }
        } else if current != self.last && !ui.input(|i| i.pointer.any_down()) {
            self.undo.push(self.last);
            if self.undo.len() > 64 {
                self.undo.remove(0);
            }
            self.redo.clear();
            self.last = current;
        }
    }

    fn graph(&mut self, ui: &mut egui::Ui, setter: &ParamSetter) {
        let (outer, response) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 260.0), Sense::click());
        let painter = ui.painter_at(outer);
        painter.rect_filled(outer, 8.0, Color32::from_rgb(13, 24, 32));
        let rect = outer.shrink2(Vec2::new(40.0, 24.0));
        let x = |frequency: f32| rect.left() + (frequency / 20.0).log10() / 3.0 * rect.width();
        let y = |db: f32| rect.top() + (24.0 - db) / 60.0 * rect.height();
        let pos_to_band = |p: Pos2| {
            (
                20.0 * 1000.0_f32.powf(((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0)),
                (24.0 - (p.y - rect.top()) / rect.height() * 60.0).clamp(-24.0, 24.0),
            )
        };
        for f in [
            20.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10000.0, 20000.0,
        ] {
            painter.line_segment(
                [Pos2::new(x(f), rect.top()), Pos2::new(x(f), rect.bottom())],
                Stroke::new(1.0, PANEL),
            );
            let label = if f >= 1000.0 {
                format!("{}k", f / 1000.0)
            } else {
                format!("{f}")
            };
            painter.text(
                Pos2::new(x(f), rect.bottom() + 13.0),
                egui::Align2::CENTER_CENTER,
                label,
                FontId::proportional(10.0),
                MUTED,
            );
        }
        for db in [24.0, 12.0, 0.0, -12.0, -24.0, -36.0] {
            painter.line_segment(
                [
                    Pos2::new(rect.left(), y(db)),
                    Pos2::new(rect.right(), y(db)),
                ],
                Stroke::new(
                    1.0,
                    if db == 0.0 {
                        MUTED.gamma_multiply(0.45)
                    } else {
                        PANEL
                    },
                ),
            );
            painter.text(
                Pos2::new(rect.left() - 8.0, y(db)),
                egui::Align2::RIGHT_CENTER,
                format!("{db:.0}"),
                FontId::proportional(10.0),
                MUTED,
            );
        }
        let frame = self.meters.read();
        let s = self.params.snapshot();
        let points = |values: &[f32; METER_BINS], transform: &dyn Fn(f32) -> f32| -> Vec<Pos2> {
            values
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    Pos2::new(
                        rect.left() + i as f32 / (METER_BINS - 1) as f32 * rect.width(),
                        y(transform(*v)),
                    )
                })
                .collect()
        };
        painter.add(egui::Shape::line(
            points(&frame.input_db, &|db| {
                ((db + 90.0) / 90.0 * 50.0 - 34.0).clamp(-35.0, 20.0)
            }),
            Stroke::new(1.0, MUTED.gamma_multiply(0.25)),
        ));
        for c in 0..2 {
            painter.add(egui::Shape::line(
                points(&frame.reduction_db[c], &|v| -v),
                Stroke::new(
                    if c == 0 { 2.0 } else { 1.2 },
                    if c == 0 { MINT } else { GOLD },
                ),
            ));
            let curve: Vec<_> = (0..METER_BINS)
                .map(|i| {
                    let f = 20.0 * 1000.0_f32.powf(i as f32 / (METER_BINS - 1) as f32);
                    Pos2::new(x(f), y(gain_to_db(s.weight(f, c)).clamp(-36.0, 24.0)))
                })
                .collect();
            painter.add(egui::Shape::line(
                curve,
                Stroke::new(
                    1.0,
                    if c == 0 {
                        Color32::WHITE.gamma_multiply(0.65)
                    } else {
                        GOLD.gamma_multiply(0.45)
                    },
                ),
            ));
        }
        painter.text(
            Pos2::new(rect.right() - 4.0, rect.top() + 4.0),
            egui::Align2::RIGHT_TOP,
            format!("−{:.1} dB  peak reduction", frame.peak_reduction_db),
            FontId::proportional(13.0),
            MINT,
        );
        let mut hit_node = false;
        for (i, b) in s.bands.iter().enumerate() {
            if !b.enabled {
                continue;
            }
            let p = Pos2::new(x(b.frequency), y(b.depth_db));
            let r = ui.interact(
                Rect::from_center_size(p, Vec2::splat(22.0)),
                ui.id().with(("node", i)),
                Sense::click_and_drag(),
            );
            hit_node |= r.hovered() || r.dragged();
            let param = &self.params.bands[i];
            if r.clicked() || r.drag_started() {
                self.selected = i;
            }
            if r.drag_started() {
                setter.begin_set_parameter(&param.frequency);
                setter.begin_set_parameter(&param.depth_db);
            }
            if r.dragged()
                && let Some(pos) = r.interact_pointer_pos()
            {
                let (frequency, db) = pos_to_band(pos);
                setter.set_parameter(&param.frequency, frequency);
                setter.set_parameter(&param.depth_db, db);
            }
            if r.drag_stopped() {
                setter.end_set_parameter(&param.frequency);
                setter.end_set_parameter(&param.depth_db);
            }
            if r.double_clicked() {
                set(setter, &param.enabled, false);
            }
            if r.hovered() {
                let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                if scroll.abs() > 0.0 {
                    set(
                        setter,
                        &param.width,
                        (b.width + scroll * 0.005).clamp(0.1, 4.0),
                    );
                }
            }
            painter.circle_filled(p, if self.selected == i { 7.0 } else { 5.0 }, GOLD);
            painter.text(
                p + Vec2::new(0.0, -15.0),
                egui::Align2::CENTER_CENTER,
                format!("{}", i + 1),
                FontId::proportional(10.0),
                GOLD,
            );
        }
        if response.double_clicked()
            && !hit_node
            && let Some(pos) = response.interact_pointer_pos()
            && let Some(i) = s.bands.iter().position(|b| !b.enabled)
        {
            let (frequency, db) = pos_to_band(pos);
            self.params.bands[i].apply(lullaby_dsp::Band::bell(frequency, db, 1.0), setter);
            self.selected = i;
        }
        // The selector also exposes disabled bands, so all eight remain editable.
        ui.horizontal(|ui| {
            for i in 0..8 {
                if ui
                    .selectable_label(
                        self.selected == i,
                        format!("{}{}", i + 1, if s.bands[i].enabled { " ●" } else { " ○" }),
                    )
                    .clicked()
                {
                    self.selected = i;
                }
            }
            ui.label(
                RichText::new("white / gold: weighting  ·  mint / gold: reduction")
                    .small()
                    .color(MUTED),
            );
        });
    }
}

impl NiceEguiApp for LullabyEditor {
    fn build(
        &mut self,
        ctx: egui::Context,
        gui: GuiContext,
        _frame: &mut Frame,
    ) -> Result<(), baseview::HandlerError> {
        ctx.set_theme(egui::Theme::Dark);
        let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
        style.visuals = egui::Visuals::dark();
        style.visuals.panel_fill = Color32::from_rgb(16, 26, 34);
        style.visuals.window_fill = PANEL;
        style.visuals.selection.bg_fill = MINT.gamma_multiply(0.25);
        style.visuals.selection.stroke = Stroke::new(1.0, MINT);
        style.visuals.widgets.active.bg_fill = Color32::from_rgb(48, 94, 85);
        style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(41, 62, 68);
        ctx.set_style_of(egui::Theme::Dark, style);
        self.context = Some(gui);
        Ok(())
    }

    fn editor_closed(&mut self) {
        self.context = None;
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut Frame) {
        let Some(context) = self.context.clone() else {
            return;
        };
        let setter = context.param_setter();
        ResizableWindow::new("lullaby_window")
            .min_size(Vec2::new(960.0, 700.0))
            .show(ui, |ui| {
                egui::Frame::new().inner_margin(18).show(ui, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| self.draw(ui, &setter));
                });
            });
        ui.ctx().request_repaint_after(Duration::from_millis(33));
    }
}

fn toggle(ui: &mut egui::Ui, param: &BoolParam, setter: &ParamSetter, label: &str) {
    if ui.selectable_label(param.value(), label).clicked() {
        set(setter, param, !param.value());
    }
}

fn choice(ui: &mut egui::Ui, param: &IntParam, setter: &ParamSetter, options: &[&str]) {
    egui::ComboBox::from_id_salt(param as *const _ as usize)
        .selected_text(options[param.value() as usize])
        .show_ui(ui, |ui| {
            for (i, option) in options.iter().enumerate() {
                if ui
                    .selectable_label(param.value() == i as i32, *option)
                    .clicked()
                {
                    set(setter, param, i as i32);
                }
            }
        });
}

fn compact(ui: &mut egui::Ui, param: &FloatParam, setter: &ParamSetter, width: f32) {
    ui.vertical(|ui| {
        ui.label(RichText::new(param.name()).size(10.0).color(MUTED));
        ui.add(ParamSlider::for_param(param, setter).with_width((width - 60.0).max(35.0)));
    });
}

fn slider(ui: &mut egui::Ui, param: &FloatParam, setter: &ParamSetter) {
    ui.horizontal(|ui| {
        ui.add_sized(
            [83.0, 20.0],
            egui::Label::new(RichText::new(param.name()).size(12.0)),
        );
        ui.add(
            ParamSlider::for_param(param, setter)
                .with_width((ui.available_width() - 76.0).max(35.0)),
        );
    });
}

fn settings_close(a: Settings, b: Settings) -> bool {
    // UI-only comparison permits the round-off introduced by host-normalized
    // floating-point parameters. Enums, switches and array shapes stay exact.
    fn compare(a: &serde_json::Value, b: &serde_json::Value) -> bool {
        match (a, b) {
            (serde_json::Value::Number(a), serde_json::Value::Number(b)) => {
                let a = a.as_f64().unwrap_or(0.0);
                let b = b.as_f64().unwrap_or(0.0);
                (a - b).abs() <= 0.0001 * b.abs().max(1.0)
            }
            (serde_json::Value::Array(a), serde_json::Value::Array(b)) => {
                a.len() == b.len() && a.iter().zip(b).all(|(a, b)| compare(a, b))
            }
            (serde_json::Value::Object(a), serde_json::Value::Object(b)) => {
                a.len() == b.len()
                    && a.iter()
                        .all(|(k, v)| b.get(k).is_some_and(|b| compare(v, b)))
            }
            _ => a == b,
        }
    }
    compare(
        &serde_json::to_value(a).expect("finite settings"),
        &serde_json::to_value(b).expect("finite settings"),
    )
}

fn card(ui: &mut egui::Ui, title: &str, contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(PANEL)
        .inner_margin(12)
        .corner_radius(7)
        .show(ui, |ui| {
            ui.set_min_height(132.0);
            ui.label(RichText::new(title).color(MINT).size(11.0));
            ui.add_space(4.0);
            contents(ui);
        });
}
