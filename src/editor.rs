mod controls;
mod knob;
mod panels;
mod spectrum;
#[cfg(test)]
mod tests;
mod theme;

use crate::{
    Meters,
    params::{LullabyParams, set},
};
use egui::{Align2, Color32, FontFamily, FontId, Sense, Stroke, StrokeKind, TextureHandle, Vec2};
use lullaby_dsp::{FACTORY_PRESETS, METER_BINS, Settings, factory_preset};
use nice_plug::{context::gui::GuiContext, prelude::*};
use nice_plug_egui::{Frame, NiceEguiApp, baseview, resizable_window::ResizableWindow};
use std::{sync::Arc, time::Duration};
use theme::{Canvas, INK, LINE, MUTED};

pub struct LullabyEditor {
    params: Arc<LullabyParams>,
    meters: Arc<Meters>,
    context: Option<GuiContext>,
    grain: Option<TextureHandle>,
    selected: usize,
    preset: String,
    preset_path: String,
    file_panel: bool,
    bands_panel: bool,
    settings_panel: bool,
    status: String,
    ab: [Settings; 2],
    active_ab: usize,
    undo: Vec<Settings>,
    redo: Vec<Settings>,
    last: Settings,
    restoring: Option<Settings>,
    quality_lock: bool,
    graph_floor: f32,
    channel_curves: bool,
    display_input: [f32; METER_BINS],
    display_reduction: [f32; METER_BINS],
}

impl LullabyEditor {
    pub(crate) fn new(params: Arc<LullabyParams>, meters: Arc<Meters>) -> Self {
        let current = params.snapshot();
        let preset = FACTORY_PRESETS
            .iter()
            .enumerate()
            .find(|(i, _)| preset_matches(current, factory_preset(*i)))
            .map_or("Custom / session", |(_, name)| *name)
            .to_owned();
        Self {
            params,
            meters,
            context: None,
            grain: None,
            selected: 0,
            preset,
            preset_path: String::new(),
            file_panel: false,
            bands_panel: false,
            settings_panel: false,
            status: String::new(),
            ab: [current; 2],
            active_ab: 0,
            undo: Vec::new(),
            redo: Vec::new(),
            last: current,
            restoring: None,
            quality_lock: true,
            graph_floor: 36.0,
            channel_curves: false,
            display_input: [-120.0; METER_BINS],
            display_reduction: [0.0; METER_BINS],
        }
    }

    fn load(&mut self, mut settings: Settings, setter: &ParamSetter) {
        if self.quality_lock {
            settings.quality = self.params.snapshot().quality;
        }
        self.params.apply(settings, setter);
    }

    fn select_preset(&mut self, index: usize, setter: &ParamSetter) {
        self.load(factory_preset(index), setter);
        self.preset = FACTORY_PRESETS[index].to_owned();
    }

    fn step_preset(&mut self, direction: isize, setter: &ParamSetter) {
        let current = FACTORY_PRESETS
            .iter()
            .position(|name| *name == self.preset)
            .unwrap_or(0);
        let next =
            (current as isize + direction).rem_euclid(FACTORY_PRESETS.len() as isize) as usize;
        self.select_preset(next, setter);
    }

    fn history(&mut self, ui: &egui::Ui) {
        let current = self.params.snapshot();
        if let Some(target) = self.restoring {
            // Host writes can arrive asynchronously; restoration must not erase redo.
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

    fn undo(&mut self, setter: &ParamSetter) {
        if let Some(settings) = self.undo.pop() {
            self.redo.push(self.params.snapshot());
            self.params.apply(settings, setter);
            self.restoring = Some(settings);
        }
    }
    fn redo(&mut self, setter: &ParamSetter) {
        if let Some(settings) = self.redo.pop() {
            self.undo.push(self.params.snapshot());
            self.params.apply(settings, setter);
            self.restoring = Some(settings);
        }
    }

    fn draw(&mut self, ui: &mut egui::Ui, setter: &ParamSetter) {
        let c = Canvas::fit(ui.clip_rect());
        let Some(grain) = self.grain.clone() else {
            return;
        };
        theme::background(ui.painter(), c, &grain, ui.clip_rect());
        self.header(ui, c, setter);
        self.spectrum(ui, c, setter);
        let params = self.params.clone();
        knob::show(
            ui,
            c,
            &grain,
            setter,
            knob::Knob {
                param: &params.depth,
                center: [227.0, 370.0],
                label: "DEPTH",
                limits: ["0", "24"],
                value: format!("{:.2}", params.depth.value()),
                percent: false,
                clay: true,
                large: true,
            },
        );
        for (param, x, label, limits, value, clay, percent) in [
            (
                &params.detail,
                148.0,
                "SHARPNESS",
                ["0", "100"],
                format!("{:.0} %", params.detail.value() * 100.0),
                false,
                true,
            ),
            (
                &params.max_cut_db,
                390.0,
                "MAXIMUM CUT",
                ["0", "36 dB"],
                format!("{:.1} dB", params.max_cut_db.value()),
                true,
                false,
            ),
            (
                &params.attack_ms,
                671.0,
                "ATTACK",
                ["0.1 ms", "500"],
                format!("{} ms", time_value(params.attack_ms.value())),
                false,
                false,
            ),
            (
                &params.release_ms,
                899.0,
                "RELEASE",
                ["5 ms", "2 s"],
                format!("{} ms", time_value(params.release_ms.value())),
                false,
                false,
            ),
            (
                &params.mix,
                1136.0,
                "MIX",
                ["0", "100"],
                format!("{:.0} %", params.mix.value() * 100.0),
                false,
                true,
            ),
            (
                &params.output_db,
                1383.0,
                "OUTPUT",
                ["−24", "+24"],
                format!("{:.1} dB", params.output_db.value()),
                false,
                false,
            ),
        ] {
            knob::show(
                ui,
                c,
                &grain,
                setter,
                knob::Knob {
                    param,
                    center: [x, 781.0],
                    label,
                    limits,
                    value,
                    percent,
                    clay,
                    large: false,
                },
            );
        }
        for x in [532.0, 1017.0] {
            c.line(ui.painter(), [x, 677.0], [x, 884.0], LINE, 1.0);
        }
        self.footer(ui, c, setter);
        self.panels(ui.ctx(), c, setter);
        self.history(ui);
    }

    fn header(&mut self, ui: &mut egui::Ui, c: Canvas, setter: &ParamSetter) {
        let p = ui.painter().clone();
        theme::logo(&p, c, 35.0, 73.0, 1.0, false);
        p.text(
            c.p(182.0, 64.0),
            Align2::LEFT_CENTER,
            "Lullaby",
            FontId::new(85.0 * c.scale, FontFamily::Name("wordmark".into())),
            INK,
        );
        c.caps(
            &p,
            370.0,
            124.0,
            13.0,
            4.7,
            "DYNAMIC RESONANCE SUPPRESSOR",
            INK,
        );
        let preset_rect = c.r(620.0, 60.0, 390.0, 51.0);
        p.rect_filled(preset_rect, 8.0 * c.scale, Color32::from_black_alpha(6));
        p.rect_stroke(
            preset_rect,
            8.0 * c.scale,
            Stroke::new(c.scale, LINE),
            StrokeKind::Inside,
        );
        p.rect_stroke(
            preset_rect.shrink(c.scale),
            7.0 * c.scale,
            Stroke::new(c.scale, Color32::from_white_alpha(80)),
            StrokeKind::Inside,
        );
        if controls::chevron(ui, c, 647.0, 86.0, false, "prev_preset", "Previous preset").clicked()
        {
            self.step_preset(-1, setter);
        }
        if controls::chevron(ui, c, 984.0, 86.0, true, "next_preset", "Next preset").clicked() {
            self.step_preset(1, setter);
        }
        let preset_response = controls::button(
            ui,
            c,
            c.r(679.0, 62.0, 273.0, 47.0),
            "preset_menu",
            &self.preset,
            false,
        );
        egui::Popup::menu(&preset_response).show(|ui| {
            ui.set_min_width(240.0);
            for (i, name) in FACTORY_PRESETS.iter().enumerate() {
                if ui.selectable_label(self.preset == *name, *name).clicked() {
                    self.select_preset(i, setter);
                    ui.close();
                }
            }
            ui.separator();
            if ui.button("Preset files & history…").clicked() {
                self.file_panel = true;
                ui.close();
            }
        });
        let save_response = ui.interact(
            c.r(1020.0, 62.0, 47.0, 48.0),
            ui.id().with("preset_file"),
            Sense::click(),
        );
        let color = if save_response.hovered() {
            theme::CLAY
        } else {
            MUTED
        };
        p.rect_stroke(
            c.r(1034.0, 77.0, 21.0, 20.0),
            0.0,
            Stroke::new(2.0 * c.scale, color),
            StrokeKind::Inside,
        );
        p.rect_stroke(
            c.r(1038.0, 77.0, 13.0, 9.0),
            0.0,
            Stroke::new(1.7 * c.scale, color),
            StrokeKind::Inside,
        );
        c.line(&p, [1038.0, 93.0], [1051.0, 93.0], color, 1.5);
        if save_response
            .on_hover_text("Preset files, copy A/B, Undo and Redo")
            .clicked()
        {
            self.file_panel = !self.file_panel;
        }
        c.line(&p, [1080.0, 70.0], [1080.0, 102.0], LINE, 1.0);
        let ab_rect = c.r(1104.0, 61.0, 147.0, 50.0);
        p.rect_filled(ab_rect, 9.0 * c.scale, Color32::from_black_alpha(5));
        p.rect_stroke(
            ab_rect,
            9.0 * c.scale,
            Stroke::new(c.scale, LINE),
            StrokeKind::Inside,
        );
        for (i, (x, name)) in [(1105.0, "A"), (1178.0, "B")].iter().enumerate() {
            let response = controls::button(
                ui,
                c,
                c.r(*x, 62.0, 72.0, 48.0),
                ("ab", i),
                name,
                self.active_ab == i,
            );
            if response.clicked() && self.active_ab != i {
                self.ab[self.active_ab] = self.params.snapshot();
                self.active_ab = i;
                self.load(self.ab[i], setter);
            }
            response.context_menu(|ui| {
                if ui.button("Copy current settings to other slot").clicked() {
                    self.ab[1 - self.active_ab] = self.params.snapshot();
                    ui.close();
                }
            });
        }
        c.line(&p, [1276.0, 59.0], [1276.0, 113.0], LINE, 1.0);
        controls::param_switch(
            ui,
            c,
            c.r(1321.0, 68.0, 69.0, 38.0),
            &self.params.bypass,
            setter,
            "Bypass",
        );
        c.caps(&p, 1450.0, 86.0, 20.0, 2.2, "BYPASS", INK);
    }

    fn footer(&mut self, ui: &mut egui::Ui, c: Canvas, setter: &ParamSetter) {
        let p = ui.painter().clone();
        c.line(&p, [43.0, 920.0], [1494.0, 920.0], LINE, 1.0);
        c.caps(&p, 136.0, 958.0, 16.0, 1.8, "DELTA LISTEN", MUTED);
        controls::param_switch(
            ui,
            c,
            c.r(214.0, 939.0, 69.0, 38.0),
            &self.params.delta,
            setter,
            "Listen to removed signal",
        );
        c.line(&p, [519.0, 965.0], [711.0, 965.0], LINE, 0.8);
        c.line(&p, [825.0, 965.0], [1017.0, 965.0], LINE, 0.8);
        theme::logo(&p, c, 730.0, 960.0, 0.62, true);
        if controls::quiet_button(
            ui,
            c,
            c.r(521.0, 936.0, 190.0, 24.0),
            "bands_panel",
            "SPECTRAL BANDS",
            self.bands_panel,
        )
        .clicked()
        {
            self.bands_panel = !self.bands_panel;
        }
        if controls::quiet_button(
            ui,
            c,
            c.r(825.0, 936.0, 192.0, 24.0),
            "settings_panel",
            "PROCESSING",
            self.settings_panel,
        )
        .clicked()
        {
            self.settings_panel = !self.settings_panel;
        }
        c.line(&p, [1227.0, 940.0], [1227.0, 979.0], LINE, 1.0);
        c.caps(&p, 1320.0, 958.0, 16.0, 1.8, "STEREO LINK", MUTED);
        if controls::switch(
            ui,
            c,
            c.r(1398.0, 939.0, 69.0, 38.0),
            "stereo_link",
            self.params.link.value() > 0.0,
            "Stereo link · continuous amount in Processing",
        )
        .clicked()
        {
            set(
                setter,
                &self.params.link,
                if self.params.link.value() > 0.0 {
                    0.0
                } else {
                    1.0
                },
            );
        }
    }
}

impl NiceEguiApp for LullabyEditor {
    fn build(
        &mut self,
        ctx: egui::Context,
        gui: GuiContext,
        _frame: &mut Frame,
    ) -> Result<(), baseview::HandlerError> {
        self.grain = Some(theme::install(&ctx));
        self.context = Some(gui);
        Ok(())
    }
    fn editor_closed(&mut self) {
        self.context = None;
        self.grain = None;
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut Frame) {
        let Some(context) = self.context.clone() else {
            return;
        };
        let setter = context.param_setter();
        ResizableWindow::new("lullaby_window")
            .min_size(Vec2::new(960.0, 640.0))
            .show(ui, |ui| self.draw(ui, &setter));
        ui.ctx().request_repaint_after(Duration::from_millis(33));
    }
}

fn time_value(value: f32) -> String {
    if value < 10.0 {
        format!("{value:.1}")
    } else {
        format!("{value:.0}")
    }
}

fn settings_close(a: Settings, b: Settings) -> bool {
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

fn preset_matches(mut a: Settings, mut b: Settings) -> bool {
    // Disabled bands have no sonic effect; their initial editor frequencies differ
    // from the DSP defaults and must not make Gentle start appear as Custom.
    for (a, b) in a.bands.iter_mut().zip(&mut b.bands) {
        if !a.enabled && !b.enabled {
            *a = lullaby_dsp::Band::default();
            *b = lullaby_dsp::Band::default();
        }
    }
    settings_close(a, b)
}
