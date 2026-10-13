use super::{LullabyEditor, set, theme::Canvas};
use egui::{RichText, Vec2};
use lullaby_dsp::Settings;
use nice_plug::prelude::*;
use nice_plug_egui::widgets::ParamSlider;
use std::sync::atomic::Ordering;

impl LullabyEditor {
    pub(super) fn panels(&mut self, ctx: &egui::Context, c: Canvas, setter: &ParamSetter) {
        if self.file_panel {
            let mut open = self.file_panel;
            egui::Window::new("Presets & history")
                .open(&mut open)
                .resizable(false)
                .default_pos(c.p(570.0, 160.0))
                .default_width(440.0)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(!self.undo.is_empty(), egui::Button::new("Undo"))
                            .clicked()
                        {
                            self.undo(setter);
                        }
                        if ui
                            .add_enabled(!self.redo.is_empty(), egui::Button::new("Redo"))
                            .clicked()
                        {
                            self.redo(setter);
                        }
                        if ui
                            .button("Copy A/B")
                            .on_hover_text("Copy current settings to the other slot")
                            .clicked()
                        {
                            self.ab[1 - self.active_ab] = self.params.snapshot();
                        }
                    });
                    ui.separator();
                    ui.label("Preset file");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.preset_path)
                            .hint_text("Absolute path to a .json preset")
                            .desired_width(f32::INFINITY),
                    );
                    ui.horizontal(|ui| {
                        if ui.button("Save preset").clicked() {
                            self.status = if !std::path::Path::new(&self.preset_path).is_absolute()
                            {
                                "Enter an absolute file path.".into()
                            } else {
                                serde_json::to_string_pretty(&self.params.snapshot())
                                    .map_err(|e| e.to_string())
                                    .and_then(|data| {
                                        std::fs::write(&self.preset_path, data)
                                            .map_err(|e| e.to_string())
                                    })
                                    .map(|_| "Preset saved.".to_owned())
                                    .unwrap_or_else(|e| e)
                            };
                        }
                        if ui.button("Load preset").clicked() {
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
                    ui.checkbox(
                        &mut self.quality_lock,
                        "Keep current quality when loading presets",
                    );
                });
            self.file_panel = open;
        }
        if self.settings_panel {
            let mut open = self.settings_panel;
            let params = self.params.clone();
            egui::Window::new("Processing")
                .open(&mut open)
                .default_pos(c.p(770.0, 330.0))
                .default_width(360.0)
                .resizable(false)
                .vscroll(true)
                .max_height(ctx.content_rect().height() - 40.0)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Mode");
                        choice(
                            ui,
                            &params.mode,
                            setter,
                            &["Soft · adaptive", "Hard · level dependent"],
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("Channels");
                        choice(
                            ui,
                            &params.stereo_mode,
                            setter,
                            &["Left / right", "Mid / side"],
                        );
                    });
                    slider(ui, &params.link, setter);
                    slider(ui, &params.focus, setter);
                    slider(ui, &params.wet_trim_db, setter);
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label("Quality");
                        choice(
                            ui,
                            &params.quality,
                            setter,
                            &["Normal", "High", "Ultra", "Low latency"],
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("Phase");
                        choice(
                            ui,
                            &params.phase,
                            setter,
                            &["Minimum phase", "Linear phase"],
                        );
                    });
                    ui.checkbox(&mut self.quality_lock, "Lock quality for presets");
                    ui.label(format!(
                        "{} samples latency",
                        self.meters.latency.load(Ordering::Relaxed)
                    ));
                    ui.separator();
                    toggle(ui, &params.external_sidechain, setter, "External sidechain");
                    toggle(ui, &params.sidechain_listen, setter, "Listen to sidechain");
                    ui.collapsing("Frequency tilts", |ui| {
                        for param in [
                            &params.detail_low,
                            &params.detail_high,
                            &params.attack_low,
                            &params.attack_high,
                            &params.release_low,
                            &params.release_high,
                        ] {
                            slider(ui, param, setter);
                        }
                    });
                    ui.separator();
                    ui.label(
                        RichText::new(format!(
                            "Lullaby {} · Independent resonance suppression",
                            env!("CARGO_PKG_VERSION")
                        ))
                        .small(),
                    );
                });
            self.settings_panel = open;
        }
        if self.bands_panel {
            let mut open = self.bands_panel;
            let params = self.params.clone();
            egui::Window::new("Spectral bands")
                .open(&mut open)
                .default_pos(c.p(470.0, 642.0))
                .default_width(590.0)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        for i in 0..8 {
                            let mark = if params.bands[i].enabled.value() { "●" } else { "○" };
                            if ui.selectable_label(self.selected == i, format!("{} {mark}", i + 1)).clicked() {
                                self.selected = i;
                            }
                        }
                    });
                    let band = &params.bands[self.selected];
                    ui.horizontal(|ui| {
                        toggle(ui, &band.enabled, setter, "Enabled");
                        choice(ui, &band.shape, setter, &[
                            "Bell", "Low shelf", "High shelf", "Low cut", "High cut", "Bandpass", "Reject", "Tilt",
                        ]);
                        let listen = params.listen_band.value() == self.selected as i32;
                        if ui.selectable_label(listen, "Audition Δ").clicked() {
                            set(setter, &params.listen_band, if listen { -1 } else { self.selected as i32 });
                        }
                        if ui.button("Remove").clicked() {
                            set(setter, &band.enabled, false);
                            if listen { set(setter, &params.listen_band, -1); }
                        }
                    });
                    ui.columns(2, |cols| {
                        for p in [&band.frequency, &band.depth_db] {
                            slider(&mut cols[0], p, setter);
                        }
                        for p in [&band.width, &band.focus] {
                            slider(&mut cols[1], p, setter);
                        }
                    });
                    ui.label(RichText::new("Double-click the graph to add a band. Drag a node to shape it; scroll to change width.").small());
                });
            self.bands_panel = open;
        }
    }
}

fn toggle(ui: &mut egui::Ui, param: &BoolParam, setter: &ParamSetter, label: &str) {
    let mut value = param.value();
    if ui.checkbox(&mut value, label).changed() {
        set(setter, param, value);
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
fn slider(ui: &mut egui::Ui, param: &FloatParam, setter: &ParamSetter) {
    ui.horizontal(|ui| {
        ui.add_sized(
            Vec2::new(86.0, 22.0),
            egui::Label::new(RichText::new(param.name()).size(12.0)),
        );
        ui.add(
            ParamSlider::for_param(param, setter)
                .with_width((ui.available_width() - 78.0).max(35.0)),
        );
    });
}
