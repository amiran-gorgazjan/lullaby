#[cfg(test)]
mod allocation_tests;
mod editor;
mod params;

use lullaby_dsp::{Engine, METER_BINS, MeterFrame};
use nice_plug::{editor::dpi::LogicalSize, prelude::*};
use nice_plug_egui::{
    EguiEditor, EguiEditorState, EguiNiceSettings, RepaintNotifier, create_egui_editor,
};
use params::LullabyParams;
use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

/// Lossy, lock-free GUI metering. No GUI-owned locks on the audio thread.
pub(crate) struct Meters {
    input: [AtomicU32; METER_BINS],
    reduction: [[AtomicU32; METER_BINS]; 2],
    peak: AtomicU32,
    latency: AtomicU32,
}

impl Default for Meters {
    fn default() -> Self {
        Self {
            input: std::array::from_fn(|_| AtomicU32::new((-120.0_f32).to_bits())),
            reduction: std::array::from_fn(|_| std::array::from_fn(|_| AtomicU32::new(0))),
            peak: AtomicU32::new(0),
            latency: AtomicU32::new(2048),
        }
    }
}

impl Meters {
    fn publish(&self, frame: &MeterFrame) {
        for i in 0..METER_BINS {
            self.input[i].store(frame.input_db[i].to_bits(), Ordering::Relaxed);
            for c in 0..2 {
                self.reduction[c][i].store(frame.reduction_db[c][i].to_bits(), Ordering::Relaxed);
            }
        }
        self.peak
            .store(frame.peak_reduction_db.to_bits(), Ordering::Relaxed);
    }

    fn read(&self) -> MeterFrame {
        MeterFrame {
            input_db: std::array::from_fn(|i| {
                f32::from_bits(self.input[i].load(Ordering::Relaxed))
            }),
            reduction_db: std::array::from_fn(|c| {
                std::array::from_fn(|i| {
                    f32::from_bits(self.reduction[c][i].load(Ordering::Relaxed))
                })
            }),
            peak_reduction_db: f32::from_bits(self.peak.load(Ordering::Relaxed)),
        }
    }
}

pub struct Lullaby {
    params: Arc<LullabyParams>,
    engine: Option<Engine>,
    meters: Arc<Meters>,
    editor_state: Arc<EguiEditorState>,
    repaint: RepaintNotifier,
    restart_requested: bool,
}

impl Default for Lullaby {
    fn default() -> Self {
        Self {
            params: Arc::new(LullabyParams::default()),
            engine: None,
            meters: Arc::new(Meters::default()),
            editor_state: EguiEditorState::from_size(LogicalSize::new(1152.0, 768.0), 1.0),
            repaint: RepaintNotifier::new(),
            restart_requested: false,
        }
    }
}

impl Plugin for Lullaby {
    const NAME: &'static str = "Lullaby";
    const VENDOR: &'static str = "Lullaby Audio";
    const URL: &'static str = "https://github.com/amiran-gorgazjan/lullaby";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");
    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(2),
            main_output_channels: NonZeroU32::new(2),
            aux_input_ports: &[new_nonzero_u32(2)],
            names: PortNames {
                aux_inputs: &["Sidechain"],
                ..PortNames::const_default()
            },
            ..AudioIOLayout::const_default()
        },
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(1),
            main_output_channels: NonZeroU32::new(1),
            aux_input_ports: &[new_nonzero_u32(1)],
            names: PortNames {
                aux_inputs: &["Sidechain"],
                ..PortNames::const_default()
            },
            ..AudioIOLayout::const_default()
        },
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(2),
            main_output_channels: NonZeroU32::new(2),
            ..AudioIOLayout::const_default()
        },
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(1),
            main_output_channels: NonZeroU32::new(1),
            ..AudioIOLayout::const_default()
        },
    ];
    const SAMPLE_ACCURATE_AUTOMATION: bool = true;
    type Editor = EguiEditor<editor::LullabyEditor>;
    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _executor: AsyncExecutor<Self>) -> Option<Self::Editor> {
        create_egui_editor(
            self.editor_state.clone(),
            self.repaint.clone(),
            EguiNiceSettings::new().with_resize_hint(
                ResizeHint::resizable().with_min_logical_size(LogicalSize::new(960.0, 640.0)),
            ),
            editor::LullabyEditor::new(self.params.clone(), self.meters.clone()),
        )
    }

    fn activate(
        &mut self,
        layout: &AudioIOLayout,
        config: &BufferConfig,
        context: &mut impl ActivateContext<Self>,
    ) -> bool {
        if !config.sample_rate.is_finite() || !(8000.0..=384000.0).contains(&config.sample_rate) {
            return false;
        }
        let s = self.params.snapshot();
        let mut engine = Engine::new(
            config.sample_rate,
            s.quality,
            layout.main_input_channels == NonZeroU32::new(1),
        );
        engine.set_settings(s);
        context.set_latency_samples(engine.latency_samples() as u32);
        self.meters
            .latency
            .store(engine.latency_samples() as u32, Ordering::Relaxed);
        self.engine = Some(engine);
        self.restart_requested = false;
        true
    }

    fn reset(&mut self) {
        if let Some(e) = &mut self.engine {
            e.reset();
            e.set_settings(self.params.snapshot());
        }
        self.meters.publish(&MeterFrame::default());
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let Some(engine) = &mut self.engine else {
            return ProcessStatus::Normal;
        };
        let settings = self.params.snapshot();
        if settings.quality != engine.quality() && !self.restart_requested {
            context.request_restart();
            self.restart_requested = true;
        }
        engine.set_settings(settings);
        let count = buffer.samples();
        let channels = buffer.as_slice();
        let key = aux.inputs.first_mut().map(|b| b.as_slice());
        for i in 0..count {
            let left = channels[0][i];
            let right = if channels.len() > 1 {
                channels[1][i]
            } else {
                left
            };
            let sidechain = key
                .as_ref()
                .map(|k| {
                    let l = k.first().and_then(|c| c.get(i)).copied().unwrap_or(0.0);
                    [l, k.get(1).and_then(|c| c.get(i)).copied().unwrap_or(l)]
                })
                .unwrap_or([0.0; 2]);
            let output = engine.process_sample([left, right], sidechain);
            channels[0][i] = output[0];
            if channels.len() > 1 {
                channels[1][i] = output[1];
            }
        }
        if engine.take_frame_ready() && self.editor_state.is_open() {
            self.meters.publish(engine.meter());
            self.repaint.request_repaint();
        }
        ProcessStatus::Tail((engine.latency_samples() * 2) as u32)
    }
}

impl ClapPlugin for Lullaby {
    const CLAP_ID: &'static str = "audio.lullaby.resonance";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Adaptive spectral resonance suppression");
    const CLAP_MANUAL_URL: Option<&'static str> =
        Some("https://github.com/amiran-gorgazjan/lullaby#use");
    const CLAP_SUPPORT_URL: Option<&'static str> =
        Some("https://github.com/amiran-gorgazjan/lullaby/issues");
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Stereo,
        ClapFeature::Mono,
        ClapFeature::Equalizer,
    ];
}

impl Vst3Plugin for Lullaby {
    const VST3_CLASS_ID: [u8; 16] = *b"LullabyResonance";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Eq];
}

nice_export_clap!(Lullaby);
nice_export_vst3!(Lullaby);
