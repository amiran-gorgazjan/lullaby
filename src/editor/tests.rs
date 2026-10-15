use super::*;
use egui::{Event, Modifiers, PointerButton, Pos2, RawInput, Rect, pos2};
use std::sync::Mutex;

struct Host {
    // Own the parameters for the lifetime of every ParamPtr used in a callback.
    params: Arc<LullabyParams>,
    events: Mutex<Vec<(&'static str, String, f32)>>,
}
impl GuiContextInner for Host {
    fn plugin_api(&self) -> PluginApi {
        PluginApi::Clap
    }
    unsafe fn raw_begin_set_parameter(&self, param: ParamPtr) {
        self.events
            .lock()
            .unwrap()
            .push(("begin", unsafe { param.name() }.to_owned(), 0.0));
    }
    unsafe fn raw_set_parameter_normalized(&self, param: ParamPtr, value: f32) {
        assert!((0.0..=1.0).contains(&value));
        unsafe {
            param._internal_set_normalized_value(value);
        }
        self.events
            .lock()
            .unwrap()
            .push(("set", unsafe { param.name() }.to_owned(), value));
    }
    unsafe fn raw_end_set_parameter(&self, param: ParamPtr) {
        self.events
            .lock()
            .unwrap()
            .push(("end", unsafe { param.name() }.to_owned(), 0.0));
    }
    fn get_state(&self) -> PluginState {
        panic!("UI tests use the public parameter setter")
    }
    fn set_state(&self, _state: PluginState) {
        panic!("UI tests use the public parameter setter")
    }
    fn request_restart(&self) {}
}

struct Harness {
    editor: LullabyEditor,
    host: Arc<Host>,
    gui: GuiContext,
    ctx: egui::Context,
    time: f64,
    size: Vec2,
}
impl Harness {
    fn new(size: Vec2) -> Self {
        let params = Arc::new(LullabyParams::default());
        let host = Arc::new(Host {
            params: params.clone(),
            events: Mutex::default(),
        });
        let ctx = egui::Context::default();
        let mut editor = LullabyEditor::new(params, Arc::new(Meters::default()));
        editor.grain = Some(theme::install(&ctx));
        let mut test = Self {
            editor,
            gui: GuiContext::new(host.clone()),
            host,
            ctx,
            time: 0.0,
            size,
        };
        test.frame(vec![], Modifiers::NONE);
        test
    }
    fn frame(&mut self, events: Vec<Event>, modifiers: Modifiers) {
        self.time += 0.05;
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, self.size)),
            time: Some(self.time),
            events: std::iter::once(Event::ModifiersChanged(modifiers))
                .chain(events)
                .collect(),
            ..Default::default()
        };
        let setter = self.gui.param_setter();
        let mut output = self.ctx.run_ui(input, |ui| self.editor.draw(ui, &setter));
        assert!(!output.shapes.is_empty());
        let meshes = self
            .ctx
            .tessellate(std::mem::take(&mut output.shapes), output.pixels_per_point);
        assert!(!meshes.is_empty());
        // Headless tests don't upload textures to a GPU.
        output.textures_delta.clear();
    }
    fn pointer(&mut self, pos: Pos2, pressed: bool, modifiers: Modifiers) {
        self.frame(
            vec![
                Event::PointerMoved(pos),
                Event::PointerButton {
                    pos,
                    button: PointerButton::Primary,
                    pressed,
                    modifiers,
                },
            ],
            modifiers,
        );
    }
    fn click(&mut self, pos: Pos2) {
        self.pointer(pos, true, Modifiers::NONE);
        self.pointer(pos, false, Modifiers::NONE);
    }
    fn key(&mut self, key: egui::Key) {
        self.frame(
            vec![Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE,
            }],
            Modifiers::NONE,
        );
    }
    fn replace_text(&mut self, text: &str) {
        self.frame(
            vec![Event::Key {
                key: egui::Key::A,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::COMMAND,
            }],
            Modifiers::COMMAND,
        );
        self.frame(vec![Event::Text(text.into())], Modifiers::NONE);
    }
}

#[test]
fn sustained_editor_rendering_has_bounded_live_memory() {
    let mut test = Harness::new(Vec2::new(1152.0, 768.0));
    for _ in 0..120 {
        test.frame(vec![], Modifiers::NONE);
    }
    let baseline = crate::allocation_tests::live_bytes(false);
    for _ in 0..1200 {
        test.frame(vec![], Modifiers::NONE);
    }
    let growth = crate::allocation_tests::live_bytes(false) - baseline;
    eprintln!("UI live heap growth after 1200 frames: {growth} bytes");
    assert!(
        growth < 256 * 1024,
        "UI retained {growth} bytes after warmup"
    );

    // Exercise changing text/meters, bounded undo history and all panels, too.
    test.editor.file_panel = true;
    test.editor.settings_panel = true;
    test.editor.bands_panel = true;
    test.editor.channel_curves = true;
    let render = |test: &mut Harness, frame: usize| {
        let phase = frame as f32 * 0.037;
        test.editor.meters.publish(&lullaby_dsp::MeterFrame {
            input_db: std::array::from_fn(|i| -40.0 + 18.0 * (phase + i as f32 * 0.1).sin()),
            reduction_db: std::array::from_fn(|c| {
                std::array::from_fn(|i| 3.0 + 2.0 * (phase + (i + c) as f32 * 0.2).sin())
            }),
            peak_reduction_db: 3.0 + 2.0 * phase.sin(),
        });
        test.size = if frame % 120 < 60 {
            Vec2::new(960.0, 640.0)
        } else {
            Vec2::new(1152.0, 768.0)
        };
        let setter = test.gui.param_setter();
        set(&setter, &test.host.params.depth, 5.0 + 3.0 * phase.sin());
        test.frame(vec![], Modifiers::NONE);
        // The mock host's event log is instrumentation, not plugin state.
        test.host.events.lock().unwrap().clear();
        assert!(test.editor.undo.len() <= 64);
        assert_eq!(test.ctx.tex_manager().read().num_allocated(), 2);
    };
    for frame in 0..240 {
        render(&mut test, frame);
    }
    let baseline = crate::allocation_tests::live_bytes(false);
    for frame in 240..1440 {
        render(&mut test, frame);
    }
    let growth = crate::allocation_tests::live_bytes(false) - baseline;
    eprintln!("Active UI live heap growth after 1200 frames: {growth} bytes");
    assert!(
        growth < 256 * 1024,
        "Active UI retained {growth} bytes after warmup"
    );
}

#[test]
fn repeated_editor_creation_releases_owned_memory() {
    let render_and_drop = || {
        let mut test = Harness::new(Vec2::new(1152.0, 768.0));
        let host = Arc::downgrade(&test.host);
        let params = Arc::downgrade(&test.host.params);
        let meters = Arc::downgrade(&test.editor.meters);
        test.frame(vec![], Modifiers::NONE);
        drop(test);
        assert!(host.upgrade().is_none());
        assert!(params.upgrade().is_none());
        assert!(meters.upgrade().is_none());
    };
    for _ in 0..4 {
        render_and_drop();
    }
    let baseline = crate::allocation_tests::live_bytes(false);
    for _ in 0..48 {
        render_and_drop();
    }
    let growth = crate::allocation_tests::live_bytes(false) - baseline;
    eprintln!("Editor live heap growth after 48 creations: {growth} bytes");
    assert!(
        growth < 64 * 1024,
        "Editor teardown retained {growth} bytes"
    );
}

#[test]
fn rotary_drag_keeps_coalesced_release_motion_and_balances_host_gestures() {
    // Check both design coordinates and a host-scaled editor.
    for (scale, single_frame) in [(1.0, false), (0.625, false), (1.0, true), (0.625, true)] {
        let mut test = Harness::new(theme::SIZE * scale);
        let start = pos2(227.0 * scale, 370.0 * scale);
        let end = start - Vec2::new(0.0, 60.0 * scale);
        if single_frame {
            // A complete fast gesture can land between two UI redraws.
            test.frame(
                vec![
                    Event::PointerMoved(start),
                    Event::PointerButton {
                        pos: start,
                        button: PointerButton::Primary,
                        pressed: true,
                        modifiers: Modifiers::NONE,
                    },
                    Event::PointerMoved(end),
                    Event::PointerButton {
                        pos: end,
                        button: PointerButton::Primary,
                        pressed: false,
                        modifiers: Modifiers::NONE,
                    },
                ],
                Modifiers::NONE,
            );
        } else {
            test.pointer(start, true, Modifiers::NONE);
            // The entire move arrives with mouse-up, as it can in a native host.
            test.pointer(end, false, Modifiers::NONE);
        }
        assert!(
            (test.host.params.depth.value() - 5.5).abs() < 0.001,
            "depth={}, events={:?}",
            test.host.params.depth.value(),
            test.host.events.lock().unwrap()
        );
        let events = test.host.events.lock().unwrap();
        assert_eq!(
            events.iter().map(|e| e.0).collect::<Vec<_>>(),
            ["begin", "set", "end"]
        );
        assert!(events.iter().all(|e| e.1 == "Depth"));
    }
}

#[test]
fn rotary_fine_drag_and_typed_values_use_the_existing_parameter_ranges() {
    let mut test = Harness::new(theme::SIZE);
    let start = pos2(671.0, 781.0);
    let initial = test.host.params.attack.unmodulated_normalized_value();
    test.pointer(start, true, Modifiers::SHIFT);
    test.pointer(start - Vec2::new(0.0, 60.0), false, Modifiers::SHIFT);
    assert!(
        (test.host.params.attack.unmodulated_normalized_value() - initial - 0.03).abs() < 0.001,
        "attack={}, initial={initial}, events={:?}",
        test.host.params.attack.unmodulated_normalized_value(),
        test.host.events.lock().unwrap()
    );
    // Direct entry uses the same unitless 0..10 control shown by the host.
    test.click(pos2(671.0, 877.0));
    test.replace_text("7.0");
    test.key(egui::Key::Enter);
    assert!((test.host.params.attack.value() - 7.0).abs() < 0.001);
}

#[test]
fn band_depth_exposes_unitless_range() {
    let params = LullabyParams::default();
    for band in &params.bands {
        assert_eq!(band.depth_db.name(), "Band depth");
        assert_eq!(band.depth_db.unit(), "");
        assert_eq!(band.depth_db.preview_plain(0.0), -10.0);
        assert_eq!(band.depth_db.preview_plain(1.0), 10.0);
        assert_eq!(band.depth_db.string_to_normalized_value("5"), Some(0.75));
    }
}

#[test]
fn timing_and_frequency_tilts_expose_unitless_controls_and_physical_multipliers() {
    let params = LullabyParams::default();
    for (param, default) in [(&params.attack, 5.0), (&params.release, 3.0)] {
        assert_eq!(param.unit(), "");
        assert_eq!(param.value(), default);
        assert_eq!(param.preview_plain(0.0), 0.0);
        assert_eq!(param.preview_plain(1.0), 10.0);
    }
    for param in [
        &params.detail_low,
        &params.detail_high,
        &params.attack_low,
        &params.attack_high,
        &params.release_low,
        &params.release_high,
    ] {
        assert_eq!(param.unit(), "x");
        assert_eq!(param.value(), 1.0);
        assert!((param.preview_plain(0.0) - 0.1).abs() < 1e-6);
        assert!((param.preview_plain(0.5) - 1.0).abs() < 1e-6);
        assert!((param.preview_plain(1.0) - 10.0).abs() < 1e-6);
        assert!(param.normalized_value_to_string(0.5, true).ends_with('x'));
    }
}

#[test]
fn percentage_readouts_accept_display_units_and_escape_cancels_edits() {
    let mut test = Harness::new(theme::SIZE);
    let param = test.host.params.mix.as_ptr();
    test.click(pos2(1136.0, 877.0));
    test.replace_text("25 %");
    test.key(egui::Key::Enter);
    assert!((unsafe { param.unmodulated_normalized_value() } - 0.25).abs() < 0.001);
    test.click(pos2(1136.0, 877.0));
    test.replace_text("90 %");
    test.key(egui::Key::Escape);
    assert!((unsafe { param.unmodulated_normalized_value() } - 0.25).abs() < 0.001);
}

#[test]
fn graph_double_click_adds_an_existing_weighting_band() {
    let mut test = Harness::new(theme::SIZE);
    let point = pos2(700.0, 380.0);
    test.click(point);
    test.click(point);
    assert!(test.editor.bands_panel);
    let band = &test.host.params.bands[0];
    assert!(band.enabled.value());
    let expected_hz = 20.0 * 1000.0_f32.powf((700.0 - 470.0) / 972.0);
    assert!((band.frequency.value() - expected_hz).abs() < 0.1);
    assert!(band.depth_db.value() > 0.0);
    assert_eq!(test.host.params.depth.value(), 3.0);
}

#[test]
fn redesigned_editor_retains_band_editing_and_factory_preset_history() {
    let mut test = Harness::new(theme::SIZE);
    assert_eq!(test.editor.preset, "Gentle start");
    let setter = test.gui.param_setter();
    test.editor.select_preset(1, &setter);
    assert_eq!(test.host.params.max_cut_db.value(), 8.0);
    assert!(test.host.params.bands[0].enabled.value());
    test.frame(vec![], Modifiers::NONE);
    let setter = test.gui.param_setter();
    test.editor.undo(&setter);
    test.frame(vec![], Modifiers::NONE);
    assert_eq!(test.host.params.max_cut_db.value(), 12.0);
    assert!(!test.host.params.bands[0].enabled.value());
    let setter = test.gui.param_setter();
    test.editor.redo(&setter);
    test.frame(vec![], Modifiers::NONE);
    assert_eq!(test.host.params.max_cut_db.value(), 8.0);
    // Existing controls remain drawable together at the minimum supported size.
    test.editor.bands_panel = true;
    test.editor.channel_curves = true;
    test.editor.settings_panel = true;
    test.editor.file_panel = true;
    test.size = Vec2::new(960.0, 640.0);
    test.frame(vec![], Modifiers::NONE);
}

#[test]
fn detail_readout_accepts_unitless_zero_to_ten_values() {
    let mut test = Harness::new(theme::SIZE);
    test.click(pos2(148.0, 877.0));
    test.replace_text("7.5");
    test.key(egui::Key::Enter);
    assert!((test.host.params.detail.value() - 7.5).abs() < 0.001);
}

#[test]
fn maximum_cut_endpoint_displays_off_and_accepts_text_and_finite_limits() {
    let mut test = Harness::new(theme::SIZE);
    let param = &test.host.params.max_cut_db;
    assert_eq!(param.preview_plain(1.0), lullaby_dsp::MAX_CUT_OFF);
    assert_eq!(param.normalized_value_to_string(1.0, true), "Off");
    assert_eq!(param.normalized_value_to_string(0.5, true), "12.00 dB");
    assert_eq!(param.string_to_normalized_value("Off"), Some(1.0));
    assert_eq!(param.string_to_normalized_value("off"), Some(1.0));
    assert_eq!(param.string_to_normalized_value("12 dB"), Some(0.5));
    assert_eq!(param.string_to_normalized_value("36"), None);
    assert_eq!(param.string_to_normalized_value("NaN"), None);
    test.click(pos2(390.0, 877.0));
    test.replace_text("Off");
    test.key(egui::Key::Enter);
    assert_eq!(
        test.host.params.snapshot().max_cut_db,
        lullaby_dsp::MAX_CUT_OFF
    );
    test.frame(
        vec![Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers: Modifiers::NONE,
        }],
        Modifiers::NONE,
    );
    test.click(pos2(390.0, 877.0));
    test.replace_text("6 dB");
    test.key(egui::Key::Enter);
    assert_eq!(test.host.params.snapshot().max_cut_db, 6.0);
    let s = Settings {
        max_cut_db: lullaby_dsp::MAX_CUT_OFF,
        ..Settings::default()
    };
    let setter = test.gui.param_setter();
    test.host.params.apply(s, &setter);
    assert_eq!(test.host.params.snapshot().max_cut_db, s.max_cut_db);
}
