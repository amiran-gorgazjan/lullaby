use lullaby_dsp::*;
use nice_plug::prelude::*;

#[derive(Params)]
pub struct LullabyParams {
    #[id = "depth"]
    pub depth: FloatParam,
    #[id = "detail"]
    pub detail: FloatParam,
    #[id = "attack_ms"]
    pub attack: FloatParam,
    #[id = "release_ms"]
    pub release: FloatParam,
    #[id = "max_cut_db"]
    pub max_cut_db: FloatParam,
    #[id = "link"]
    pub link: FloatParam,
    #[id = "focus"]
    pub focus: FloatParam,
    #[id = "detail_low"]
    pub detail_low: FloatParam,
    #[id = "detail_high"]
    pub detail_high: FloatParam,
    #[id = "attack_low"]
    pub attack_low: FloatParam,
    #[id = "attack_high"]
    pub attack_high: FloatParam,
    #[id = "release_low"]
    pub release_low: FloatParam,
    #[id = "release_high"]
    pub release_high: FloatParam,
    #[id = "mix"]
    pub mix: FloatParam,
    #[id = "wet_trim_db"]
    pub wet_trim_db: FloatParam,
    #[id = "output_db"]
    pub output_db: FloatParam,
    #[id = "bypass"]
    pub bypass: BoolParam,
    #[id = "delta"]
    pub delta: BoolParam,
    #[id = "external_sidechain"]
    pub external_sidechain: BoolParam,
    #[id = "sidechain_listen"]
    pub sidechain_listen: BoolParam,
    #[id = "mode"]
    pub mode: IntParam,
    #[id = "stereo_mode"]
    pub stereo_mode: IntParam,
    #[id = "phase"]
    pub phase: IntParam,
    #[id = "quality"]
    pub quality: IntParam,
    #[id = "listen_band"]
    pub listen_band: IntParam,
    #[nested(array, group = "Band")]
    pub bands: [BandParams; BAND_COUNT],
}

impl Default for LullabyParams {
    fn default() -> Self {
        Self {
            depth: FloatParam::new(
                "Depth",
                3.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: 10.0,
                },
            )
            .with_unit("")
            .with_value_to_string(formatters::v2s_f32_rounded(2)),
            detail: FloatParam::new(
                "Detail",
                5.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: 10.0,
                },
            )
            .with_unit("")
            .with_value_to_string(formatters::v2s_f32_rounded(2)),
            attack: FloatParam::new(
                "Attack",
                5.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: 10.0,
                },
            )
            .with_unit("")
            .with_value_to_string(formatters::v2s_f32_rounded(2)),
            release: FloatParam::new(
                "Release",
                3.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: 10.0,
                },
            )
            .with_unit("")
            .with_value_to_string(formatters::v2s_f32_rounded(2)),
            max_cut_db: FloatParam::new(
                "Max cut",
                12.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: MAX_CUT_OFF,
                },
            )
            .with_unit("")
            .with_value_to_string(std::sync::Arc::new(max_cut_text))
            .with_string_to_value(std::sync::Arc::new(|text| {
                let text = text.trim();
                if text.eq_ignore_ascii_case("off") {
                    Some(MAX_CUT_OFF)
                } else {
                    text.trim_end_matches("dB")
                        .trim()
                        .parse::<f32>()
                        .ok()
                        .filter(|v| v.is_finite() && (0.0..=MAX_CUT_OFF).contains(v))
                }
            })),
            link: FloatParam::new(
                "Stereo link",
                1.0,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_unit("")
            .with_value_to_string(formatters::v2s_f32_rounded(2)),
            focus: FloatParam::new(
                "Stereo focus",
                0.0,
                FloatRange::Linear {
                    min: -1.0,
                    max: 1.0,
                },
            )
            .with_unit("")
            .with_value_to_string(formatters::v2s_f32_rounded(2)),
            detail_low: multiplier_param("Detail / low"),
            detail_high: multiplier_param("Detail / high"),
            attack_low: multiplier_param("Attack / low"),
            attack_high: multiplier_param("Attack / high"),
            release_low: multiplier_param("Release / low"),
            release_high: multiplier_param("Release / high"),
            mix: FloatParam::new("Mix", 1.0, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_unit("")
                .with_value_to_string(formatters::v2s_f32_rounded(2)),
            wet_trim_db: FloatParam::new(
                "Wet trim",
                0.0,
                FloatRange::Linear {
                    min: -24.0,
                    max: 24.0,
                },
            )
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_rounded(2)),
            output_db: FloatParam::new(
                "Output",
                0.0,
                FloatRange::Linear {
                    min: -24.0,
                    max: 24.0,
                },
            )
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_rounded(2)),
            bypass: BoolParam::new("Bypass", false).make_bypass(),
            delta: BoolParam::new("Delta", false),
            external_sidechain: BoolParam::new("External sidechain", false),
            sidechain_listen: BoolParam::new("Listen to sidechain", false),
            mode: labeled_int_param("Mode", &["Soft", "Hard"]),
            stereo_mode: labeled_int_param("Channels", &["L/R", "M/S"]),
            phase: labeled_int_param("Phase", &["Minimum", "Linear"]),
            quality: labeled_int_param("Quality", &["Normal", "High", "Ultra", "Low latency"])
                .non_automatable(),
            listen_band: IntParam::new("Band audition", -1, IntRange::Linear { min: -1, max: 7 })
                .non_automatable(),
            bands: std::array::from_fn(BandParams::new),
        }
    }
}

fn labeled_int_param(name: &'static str, labels: &'static [&'static str]) -> IntParam {
    IntParam::new(
        name,
        0,
        IntRange::Linear {
            min: 0,
            max: labels.len() as i32 - 1,
        },
    )
    .with_value_to_string(std::sync::Arc::new(move |value| {
        labels[value as usize].to_owned()
    }))
    .with_string_to_value(std::sync::Arc::new(move |text| {
        labels
            .iter()
            .position(|label| label.eq_ignore_ascii_case(text))
            .map(|index| index as i32)
            .or_else(|| text.parse().ok())
    }))
}

impl LullabyParams {
    pub fn snapshot(&self) -> Settings {
        Settings {
            depth: self.depth.value(),
            detail: self.detail.value(),
            attack: self.attack.value(),
            release: self.release.value(),
            max_cut_db: self.max_cut_db.value(),
            link: self.link.value(),
            focus: self.focus.value(),
            mix: self.mix.value(),
            wet_trim_db: self.wet_trim_db.value(),
            output_db: self.output_db.value(),
            bypass: self.bypass.value(),
            delta: self.delta.value(),
            external_sidechain: self.external_sidechain.value(),
            sidechain_listen: self.sidechain_listen.value(),
            mode: [Mode::Soft, Mode::Hard][self.mode.value() as usize],
            stereo_mode: [StereoMode::LeftRight, StereoMode::MidSide]
                [self.stereo_mode.value() as usize],
            phase: [Phase::Minimum, Phase::Linear][self.phase.value() as usize],
            quality: [
                Quality::Normal,
                Quality::High,
                Quality::Ultra,
                Quality::LowLatency,
            ][self.quality.value() as usize],
            detail_tilt: [self.detail_low.value(), self.detail_high.value()],
            attack_tilt: [self.attack_low.value(), self.attack_high.value()],
            release_tilt: [self.release_low.value(), self.release_high.value()],
            listen_band: self.listen_band.value(),
            bands: std::array::from_fn(|i| self.bands[i].snapshot()),
        }
    }

    pub fn apply(&self, s: Settings, setter: &ParamSetter) {
        let s = s.sanitized();
        set(setter, &self.depth, s.depth);
        set(setter, &self.detail, s.detail);
        set(setter, &self.attack, s.attack);
        set(setter, &self.release, s.release);
        set(setter, &self.max_cut_db, s.max_cut_db);
        set(setter, &self.link, s.link);
        set(setter, &self.focus, s.focus);
        set(setter, &self.detail_low, s.detail_tilt[0]);
        set(setter, &self.detail_high, s.detail_tilt[1]);
        set(setter, &self.attack_low, s.attack_tilt[0]);
        set(setter, &self.attack_high, s.attack_tilt[1]);
        set(setter, &self.release_low, s.release_tilt[0]);
        set(setter, &self.release_high, s.release_tilt[1]);
        set(setter, &self.mix, s.mix);
        set(setter, &self.wet_trim_db, s.wet_trim_db);
        set(setter, &self.output_db, s.output_db);
        set(setter, &self.bypass, s.bypass);
        set(setter, &self.delta, s.delta);
        set(setter, &self.external_sidechain, s.external_sidechain);
        set(setter, &self.sidechain_listen, s.sidechain_listen);
        set(setter, &self.mode, s.mode as i32);
        set(setter, &self.stereo_mode, s.stereo_mode as i32);
        set(setter, &self.phase, s.phase as i32);
        set(setter, &self.quality, s.quality as i32);
        set(setter, &self.listen_band, s.listen_band);
        for (p, band) in self.bands.iter().zip(s.bands) {
            p.apply(band, setter);
        }
    }
}

// Positive multipliers, with neutral 1x at the center of the knob. The gesture
// is logarithmic; the host's plain value is the multiplier.
fn multiplier_param(name: &'static str) -> FloatParam {
    FloatParam::new(name, 1.0, FloatRange::gain_range(-20.0, 20.0))
        .with_unit("x")
        .with_value_to_string(formatters::v2s_f32_rounded(2))
}

pub fn max_cut_text(value: f32) -> String {
    if value >= MAX_CUT_OFF {
        "Off".to_owned()
    } else {
        format!("{value:.2} dB")
    }
}

pub fn set<P: Param>(setter: &ParamSetter, param: &P, value: P::Plain) {
    setter.begin_set_parameter(param);
    setter.set_parameter(param, value);
    setter.end_set_parameter(param);
}

#[derive(Params)]
pub struct BandParams {
    #[id = "band_enabled"]
    pub enabled: BoolParam,
    #[id = "band_shape"]
    pub shape: IntParam,
    #[id = "band_frequency"]
    pub frequency: FloatParam,
    #[id = "band_depth_db"]
    pub depth_db: FloatParam,
    #[id = "band_width"]
    pub width: FloatParam,
    #[id = "band_focus"]
    pub focus: FloatParam,
}

impl BandParams {
    fn new(i: usize) -> Self {
        Self {
            enabled: BoolParam::new("Enabled", false),
            shape: IntParam::new("Shape", 0, IntRange::Linear { min: 0, max: 7 }),
            frequency: FloatParam::new(
                "Frequency",
                [100.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 12000.0][i],
                FloatRange::Skewed {
                    min: 20.0,
                    max: 20000.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_value_to_string(formatters::v2s_f32_hz_then_khz(1))
            .with_string_to_value(formatters::s2v_f32_hz_then_khz()),
            depth_db: FloatParam::new(
                "Band depth",
                0.0,
                FloatRange::Linear {
                    min: -10.0,
                    max: 10.0,
                },
            )
            .with_value_to_string(formatters::v2s_f32_rounded(2)),
            width: FloatParam::new("Width", 1.0, FloatRange::Linear { min: 0.1, max: 4.0 })
                .with_unit(" oct")
                .with_value_to_string(formatters::v2s_f32_rounded(2)),
            focus: FloatParam::new(
                "Focus",
                0.0,
                FloatRange::Linear {
                    min: -1.0,
                    max: 1.0,
                },
            )
            .with_value_to_string(formatters::v2s_f32_rounded(2)),
        }
    }
    pub fn snapshot(&self) -> Band {
        Band {
            enabled: self.enabled.value(),
            shape: [
                BandShape::Bell,
                BandShape::LowShelf,
                BandShape::HighShelf,
                BandShape::LowCut,
                BandShape::HighCut,
                BandShape::BandPass,
                BandShape::BandReject,
                BandShape::Tilt,
            ][self.shape.value() as usize],
            frequency: self.frequency.value(),
            depth_db: self.depth_db.value(),
            width: self.width.value(),
            focus: self.focus.value(),
        }
    }
    pub fn apply(&self, b: Band, setter: &ParamSetter) {
        set(setter, &self.enabled, b.enabled);
        set(setter, &self.shape, b.shape as i32);
        set(setter, &self.frequency, b.frequency);
        set(setter, &self.depth_db, b.depth_db);
        set(setter, &self.width, b.width);
        set(setter, &self.focus, b.focus);
    }
}
