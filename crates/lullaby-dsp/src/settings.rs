use serde::{Deserialize, Serialize};

pub const BAND_COUNT: usize = 8;
/// Host/JSON endpoint for disabling the maximum-reduction cap. Values below
/// this endpoint remain finite dB limits; the endpoint itself is not a 24 dB cap.
pub const MAX_CUT_OFF: f32 = 24.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mode {
    #[default]
    Soft,
    Hard,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum StereoMode {
    #[default]
    LeftRight,
    MidSide,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    #[default]
    Minimum,
    Linear,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Quality {
    #[default]
    Normal,
    High,
    Ultra,
    LowLatency,
}

impl Quality {
    pub fn fft_size(self) -> usize {
        if self == Self::LowLatency { 512 } else { 2048 }
    }

    pub fn overlap(self) -> usize {
        match self {
            Self::Normal | Self::LowLatency => 4,
            Self::High => 8,
            Self::Ultra => 16,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BandShape {
    #[default]
    Bell,
    LowShelf,
    HighShelf,
    LowCut,
    HighCut,
    BandPass,
    BandReject,
    Tilt,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Band {
    pub enabled: bool,
    pub shape: BandShape,
    pub frequency: f32,
    pub depth_db: f32,
    /// Width in octaves, rather than an analog filter's Q.
    pub width: f32,
    /// -1 = left/mid only; +1 = right/side only.
    pub focus: f32,
}

impl Default for Band {
    fn default() -> Self {
        Self {
            enabled: false,
            shape: BandShape::Bell,
            frequency: 1000.0,
            depth_db: 0.0,
            width: 1.0,
            focus: 0.0,
        }
    }
}

impl Band {
    pub fn bell(frequency: f32, depth_db: f32, width: f32) -> Self {
        Self {
            enabled: true,
            frequency,
            depth_db,
            width,
            ..Self::default()
        }
    }

    /// An original log-frequency weighting curve, not an audio EQ response.
    pub fn response(self, frequency: f32) -> f32 {
        let x = (frequency.max(1.0) / self.frequency.max(1.0)).log2();
        let w = self.width.max(0.05);
        let bell = (-0.5 * (x / (w * 0.425)).powi(2)).exp();
        match self.shape {
            BandShape::Bell | BandShape::BandPass | BandShape::BandReject => bell,
            BandShape::LowShelf | BandShape::HighCut => 1.0 / (1.0 + (x * 4.0 / w).exp()),
            BandShape::HighShelf | BandShape::LowCut => 1.0 / (1.0 + (-x * 4.0 / w).exp()),
            BandShape::Tilt => (x / w).tanh(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub mode: Mode,
    /// Unitless 0..10 suppression strength; not a dB amount.
    pub depth: f32,
    /// Unitless 0..10 cut concentration: higher values select narrower, stronger cuts.
    /// This independently calibrated law is not a filter Q or bandwidth in Hz.
    pub detail: f32,
    /// Unitless 0..10 onset/recovery controls; not milliseconds.
    pub attack: f32,
    pub release: f32,
    /// 0..24: finite dB cap below 24; the 24 endpoint means Off (unlimited reduction).
    pub max_cut_db: f32,
    pub stereo_mode: StereoMode,
    pub link: f32,
    pub focus: f32,
    /// Low/high displayed multipliers (.1x..10x), neutral 1x. Their frequency
    /// interpolation and effect on Detail are independently modeled.
    pub detail_tilt: [f32; 2],
    pub attack_tilt: [f32; 2],
    pub release_tilt: [f32; 2],
    pub bands: [Band; BAND_COUNT],
    pub mix: f32,
    pub wet_trim_db: f32,
    pub output_db: f32,
    pub delta: bool,
    pub bypass: bool,
    pub external_sidechain: bool,
    pub sidechain_listen: bool,
    /// -1 disables band delta audition; 0..7 selects a band.
    pub listen_band: i32,
    pub phase: Phase,
    pub quality: Quality,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mode: Mode::Soft,
            depth: 3.0,
            detail: 5.0,
            attack: 5.0,
            release: 3.0,
            max_cut_db: 12.0,
            stereo_mode: StereoMode::LeftRight,
            link: 1.0,
            focus: 0.0,
            detail_tilt: [1.0; 2],
            attack_tilt: [1.0; 2],
            release_tilt: [1.0; 2],
            bands: [Band::default(); BAND_COUNT],
            mix: 1.0,
            wet_trim_db: 0.0,
            output_db: 0.0,
            delta: false,
            bypass: false,
            external_sidechain: false,
            sidechain_listen: false,
            listen_band: -1,
            phase: Phase::Minimum,
            quality: Quality::Normal,
        }
    }
}

impl Settings {
    /// Defend the public DSP API against invalid automation or malformed preset values.
    pub fn sanitized(mut self) -> Self {
        fn clamp(v: f32, min: f32, max: f32, fallback: f32) -> f32 {
            if v.is_finite() {
                v.clamp(min, max)
            } else {
                fallback
            }
        }
        self.depth = clamp(self.depth, 0.0, 10.0, 3.0);
        self.detail = clamp(self.detail, 0.0, 10.0, 5.0);
        self.attack = clamp(self.attack, 0.0, 10.0, 5.0);
        self.release = clamp(self.release, 0.0, 10.0, 3.0);
        self.max_cut_db = clamp(self.max_cut_db, 0.0, MAX_CUT_OFF, 12.0);
        self.link = clamp(self.link, 0.0, 1.0, 1.0);
        self.focus = clamp(self.focus, -1.0, 1.0, 0.0);
        for tilts in [
            &mut self.detail_tilt,
            &mut self.attack_tilt,
            &mut self.release_tilt,
        ] {
            for v in tilts {
                *v = clamp(*v, 0.1, 10.0, 1.0);
            }
        }
        for band in &mut self.bands {
            band.frequency = clamp(band.frequency, 20.0, 20000.0, 1000.0);
            band.depth_db = clamp(band.depth_db, -24.0, 24.0, 0.0);
            band.width = clamp(band.width, 0.1, 4.0, 1.0);
            band.focus = clamp(band.focus, -1.0, 1.0, 0.0);
        }
        self.mix = clamp(self.mix, 0.0, 1.0, 1.0);
        self.wet_trim_db = clamp(self.wet_trim_db, -24.0, 24.0, 0.0);
        self.output_db = clamp(self.output_db, -24.0, 24.0, 0.0);
        self.listen_band = self.listen_band.clamp(-1, 7);
        self
    }

    /// Apply only an enabled cap. Off does not silently substitute a large
    /// finite limit, and the stored endpoint stays finite for preset JSON.
    pub fn limit_reduction(&self, reduction_db: f32) -> f32 {
        let reduction_db = reduction_db.max(0.0);
        if self.max_cut_db >= MAX_CUT_OFF {
            reduction_db
        } else {
            reduction_db.min(self.max_cut_db)
        }
    }

    pub fn weight(&self, frequency: f32, channel: usize) -> f32 {
        let mut db = 0.0;
        let mut mask = 1.0;
        let mut pass = 0.0_f32;
        let mut has_pass = false;
        for band in self.bands.iter().filter(|b| b.enabled) {
            let response = band.response(frequency);
            let focus = channel_focus(band.focus, channel);
            match band.shape {
                BandShape::LowCut | BandShape::HighCut => mask *= 1.0 + focus * (response - 1.0),
                BandShape::BandReject => mask *= 1.0 - response * focus,
                BandShape::BandPass => {
                    has_pass = true;
                    pass = pass.max(response * focus);
                    db += band.depth_db * response * focus;
                }
                _ => db += band.depth_db * response * focus,
            }
        }
        if has_pass {
            mask *= pass;
        }
        db_to_gain(db.clamp(-48.0, 36.0)) * mask * channel_focus(self.focus, channel)
    }
}

pub fn channel_focus(focus: f32, channel: usize) -> f32 {
    if channel == 0 {
        1.0 - focus.max(0.0)
    } else {
        1.0 + focus.min(0.0)
    }
}

pub fn db_to_gain(db: f32) -> f32 {
    10.0_f32.powf(db * 0.05)
}

pub fn gain_to_db(gain: f32) -> f32 {
    20.0 * gain.max(1e-9).log10()
}

pub const FACTORY_PRESETS: &[&str] = &[
    "Gentle start",
    "Vocal silk",
    "Cymbal calm",
    "Low-mid clarity",
    "Mix polish",
    "Sidechain space",
];

pub fn factory_preset(index: usize) -> Settings {
    let mut s = Settings::default();
    match index {
        1 => {
            s.bands[0] = Band::bell(3200.0, 6.0, 1.5);
            s.max_cut_db = 8.0;
        }
        2 => {
            s.bands[0] = Band::bell(7000.0, 9.0, 2.0);
            s.attack = 3.5;
            s.release = 4.6;
        }
        3 => {
            s.bands[0] = Band::bell(300.0, 9.0, 1.5);
            s.attack = 6.9;
            s.max_cut_db = 6.0;
        }
        4 => {
            s.depth = 3.0;
            s.max_cut_db = 3.0;
            s.stereo_mode = StereoMode::MidSide;
            s.attack = 6.7;
        }
        5 => {
            s.external_sidechain = true;
            s.depth = 10.0;
            s.max_cut_db = 9.0;
            s.bands[0] = Band::bell(1800.0, 6.0, 2.0);
        }
        _ => {}
    }
    s
}
