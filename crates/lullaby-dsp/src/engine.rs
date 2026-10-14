use crate::detector::SpectralReference;
use crate::{Mode, Phase, Quality, Settings, StereoMode, db_to_gain, gain_to_db};
use rustfft::{Fft, FftPlanner, num_complex::Complex32};
use std::{f32::consts::PI, sync::Arc};

pub const METER_BINS: usize = 192;

#[derive(Clone, Copy)]
pub struct MeterFrame {
    pub input_db: [f32; METER_BINS],
    pub reduction_db: [[f32; METER_BINS]; 2],
    pub peak_reduction_db: f32,
}

impl Default for MeterFrame {
    fn default() -> Self {
        Self {
            input_db: [-120.0; METER_BINS],
            reduction_db: [[0.0; METER_BINS]; 2],
            peak_reduction_db: 0.0,
        }
    }
}

struct Channel {
    input: Vec<f32>,
    key: Vec<f32>,
    output: Vec<f32>,
    audition: Vec<f32>,
    spectrum: Vec<Complex32>,
    detector: Vec<Complex32>,
    filter: Vec<Complex32>,
    reference: SpectralReference,
    magnitude_db: Vec<f32>,
    reduction: Vec<f32>,
    requested: Vec<f32>,
    weight: Vec<f32>,
}

impl Channel {
    fn new(n: usize) -> Self {
        Self {
            input: vec![0.0; n],
            key: vec![0.0; n],
            output: vec![0.0; n],
            audition: vec![0.0; n],
            spectrum: vec![Complex32::default(); n],
            detector: vec![Complex32::default(); n],
            filter: vec![Complex32::default(); n],
            reference: SpectralReference::new(n / 2 + 1),
            magnitude_db: vec![0.0; n / 2 + 1],
            reduction: vec![0.0; n / 2 + 1],
            requested: vec![0.0; n / 2 + 1],
            weight: vec![0.0; n / 2 + 1],
        }
    }

    fn reset(&mut self) {
        for v in [
            &mut self.input,
            &mut self.key,
            &mut self.output,
            &mut self.audition,
            &mut self.reduction,
        ] {
            v.fill(0.0);
        }
    }
}

/// Weighted overlap-add FFT engine. Construction/reset/configuration happen outside processing.
/// Sample calls perform no allocation, locking, I/O, or FFT planning.
pub struct Engine {
    sample_rate: f32,
    quality: Quality,
    channels: [Channel; 2],
    mono: bool,
    n: usize,
    hop: usize,
    pos: usize,
    until_frame: usize,
    filled: usize,
    window: Vec<f32>,
    forward: Arc<dyn Fft<f32>>,
    inverse: Arc<dyn Fft<f32>>,
    scratch: Vec<Complex32>,
    dry: Vec<[f32; 2]>,
    sidechain_delay: Vec<[f32; 2]>,
    attack: Vec<f32>,
    release: Vec<f32>,
    reference_lower: Vec<usize>,
    reference_upper: Vec<usize>,
    spread_lower: Vec<usize>,
    spread_upper: Vec<usize>,
    sqrt_frequency: Vec<f32>,
    spread_kernel_offsets: Vec<usize>,
    spread_kernel_lower: Vec<usize>,
    spread_kernel: Vec<f32>,
    contrast_scale: Vec<f32>,
    detail_strength: Vec<f32>,
    threshold: Vec<f32>,
    hard_threshold: Vec<f32>,
    audition_mask: Vec<f32>,
    cached: Option<Settings>,
    mix: f32,
    trim: f32,
    gain: f32,
    target_trim: f32,
    target_gain: f32,
    depth_amount: f32,
    bypass: f32,
    delta: f32,
    listen: f32,
    band_listen: f32,
    smooth: f32,
    meter: MeterFrame,
    frame_ready: bool,
}

impl Engine {
    pub fn new(sample_rate: f32, quality: Quality, mono: bool) -> Self {
        assert!(sample_rate.is_finite() && (8000.0..=384000.0).contains(&sample_rate));
        let n = quality.fft_size();
        let hop = n / quality.overlap();
        let mut planner = FftPlanner::new();
        let forward = planner.plan_fft_forward(n);
        let inverse = planner.plan_fft_inverse(n);
        let scratch_len = forward
            .get_inplace_scratch_len()
            .max(inverse.get_inplace_scratch_len());
        // Reserve the widest possible Detail envelope once. Automation only
        // overwrites existing weights; the audio path never allocates.
        let mut spread_kernel_offsets = Vec::with_capacity(n / 2 + 2);
        let mut spread_kernel_lower = Vec::with_capacity(n / 2 + 1);
        let mut kernel_count = 0;
        let maximum_radius = 3.0 * 10.0;
        for k in 0..=n / 2 {
            let center = (k as f32 * sample_rate / n as f32).sqrt();
            let lower = (((center - maximum_radius).max(0.0).powi(2) * n as f32 / sample_rate)
                .floor() as usize)
                .max(1);
            let upper =
                (((center + maximum_radius).powi(2) * n as f32 / sample_rate).ceil() as usize + 1)
                    .min(n / 2 + 1);
            spread_kernel_offsets.push(kernel_count);
            spread_kernel_lower.push(lower);
            kernel_count += upper.saturating_sub(lower);
        }
        spread_kernel_offsets.push(kernel_count);
        Self {
            sample_rate,
            quality,
            channels: [Channel::new(n), Channel::new(n)],
            mono,
            n,
            hop,
            pos: 0,
            until_frame: hop,
            filled: 0,
            window: (0..n)
                .map(|i| (0.5 - 0.5 * (2.0 * PI * i as f32 / n as f32).cos()).sqrt())
                .collect(),
            forward,
            inverse,
            scratch: vec![Complex32::default(); scratch_len],
            dry: vec![[0.0; 2]; n],
            sidechain_delay: vec![[0.0; 2]; n],
            attack: vec![0.0; n / 2 + 1],
            release: vec![0.0; n / 2 + 1],
            reference_lower: vec![0; n / 2 + 1],
            reference_upper: vec![0; n / 2 + 1],
            spread_lower: vec![0; n / 2 + 1],
            spread_upper: vec![0; n / 2 + 1],
            sqrt_frequency: (0..=n / 2)
                .map(|k| (k as f32 * sample_rate / n as f32).sqrt())
                .collect(),
            spread_kernel_offsets,
            spread_kernel_lower,
            spread_kernel: vec![0.0; kernel_count],
            contrast_scale: vec![1.0; n / 2 + 1],
            detail_strength: vec![0.0; n / 2 + 1],
            threshold: vec![0.0; n / 2 + 1],
            hard_threshold: vec![0.0; n / 2 + 1],
            audition_mask: vec![0.0; n / 2 + 1],
            cached: None,
            mix: 1.0,
            trim: 1.0,
            gain: 1.0,
            target_trim: 1.0,
            target_gain: 1.0,
            depth_amount: 0.0,
            bypass: 0.0,
            delta: 0.0,
            listen: 0.0,
            band_listen: 0.0,
            smooth: 1.0 - (-1.0 / (sample_rate * 0.01)).exp(),
            meter: MeterFrame::default(),
            frame_ready: false,
        }
    }

    pub fn latency_samples(&self) -> usize {
        self.n
    }
    pub fn quality(&self) -> Quality {
        self.quality
    }
    pub fn meter(&self) -> &MeterFrame {
        &self.meter
    }
    pub fn take_frame_ready(&mut self) -> bool {
        std::mem::take(&mut self.frame_ready)
    }

    pub fn reset(&mut self) {
        for c in &mut self.channels {
            c.reset();
        }
        self.dry.fill([0.0; 2]);
        self.sidechain_delay.fill([0.0; 2]);
        self.pos = 0;
        self.until_frame = self.hop;
        self.filled = 0;
        self.cached = None;
        self.meter = MeterFrame::default();
        self.frame_ready = false;
    }

    /// Call once per host block or automation segment, before `process_sample`.
    /// Quality is fixed at construction; hosts must reactivate to change latency.
    pub fn set_settings(&mut self, settings: Settings) {
        let s = settings.sanitized();
        if self.cached == Some(s) {
            return;
        }
        let first = self.cached.is_none();
        let detail_changed = self
            .cached
            .is_none_or(|old| old.detail != s.detail || old.detail_tilt != s.detail_tilt);
        let detail_tilt = s.detail_tilt.map(f32::log10);
        let attack_tilt = s.attack_tilt.map(f32::log10);
        let release_tilt = s.release_tilt.map(f32::log10);
        // Independent render law, retained after the closer isolated-key
        // time-constant hypothesis worsened the phase-sensitive corpus score.
        let attack_seconds = 0.0001 * 5000.0_f32.powf(s.attack / 10.0);
        let release_seconds = 0.005 * 400.0_f32.powf(s.release / 10.0);
        for k in 0..=self.n / 2 {
            let f = (k as f32 * self.sample_rate / self.n as f32).max(20.0);
            let low = 1.0 / (1.0 + (f / 500.0).powi(2));
            let high = 1.0 / (1.0 + (2000.0 / f).powi(2));
            let tilt = |t: [f32; 2]| t[0] * low + t[1] * high;
            let detail = (s.detail / 10.0 + 0.5 * tilt(detail_tilt)).clamp(0.0, 1.0);
            // Independently modeled Gaussian cut envelope in square-root
            // frequency. Its absolute bandwidth grows with frequency while its
            // relative bandwidth narrows; Detail controls concentration.
            let spread_width = 1.0 + 9.0 * (1.0 - detail).powf(1.5);
            let radius = 3.0 * spread_width;
            let center = self.sqrt_frequency[k];
            self.spread_lower[k] = (((center - radius).max(0.0).powi(2) * self.n as f32
                / self.sample_rate)
                .floor() as usize)
                .max(1);
            self.spread_upper[k] = (((center + radius).powi(2) * self.n as f32 / self.sample_rate)
                .ceil() as usize
                + 1)
            .min(self.n / 2 + 1);
            if detail_changed {
                let inverse_width = 1.0 / spread_width;
                for j in self.spread_lower[k]..self.spread_upper[k] {
                    let distance =
                        (self.sqrt_frequency[j] - self.sqrt_frequency[k]) * inverse_width;
                    let index = self.spread_kernel_offsets[k] + j - self.spread_kernel_lower[k];
                    let source = self.sqrt_frequency[j];
                    let peak = 0.5
                        * (source + (source * source + 2.0 * spread_width * spread_width).sqrt());
                    let peak_distance = (peak - source) * inverse_width;
                    // Smooth frequency-density skew, normalized to keep the
                    // requested maximum unchanged rather than adding gain.
                    self.spread_kernel[index] =
                        center / peak * (peak_distance * peak_distance - distance * distance).exp();
                }
            }
            self.detail_strength[k] =
                ((detail - 0.5) * if detail < 0.5 { 1.085 } else { 2.918 }).exp2();
            // Independent Soft-mode sensitivity hypothesis from isolated
            // equal-level tones: a broad presence emphasis, not a fixed audio
            // EQ. It still acts only on detected contrast and active bins.
            if s.mode == Mode::Soft {
                let presence = (f / 2500.0).powi(4);
                self.detail_strength[k] *=
                    1.0 + presence / (1.0 + presence) / (1.0 + (f / 7000.0).powi(3));
            }
            let extent = (1.25 - 0.5 * detail).exp2();
            self.reference_lower[k] = (k as f32 / extent).floor() as usize;
            self.reference_upper[k] = ((k as f32 * extent).ceil() as usize + 1).min(self.n / 2 + 1);
            // A lone peak's mean-power contrast depends on bin density. Keep
            // the smaller low-latency transform on a comparable contrast scale
            // while leaving a flat/noise-like spectrum close to zero contrast.
            let count = (self.reference_upper[k] - self.reference_lower[k]).max(2) as f32;
            self.contrast_scale[k] =
                ((count * 2048.0 / self.n as f32).ln() / count.ln()).clamp(1.0, 2.0);
            // Detail tilt affects contrast selection as well as bandwidth.
            // Higher depth gradually admits less prominent spectral features.
            self.threshold[k] = (5.0 - 2.0 * detail - (s.depth - 6.0).max(0.0) * 0.15).max(0.5);
            // Low Depth excludes weaker contrast rather than merely scaling
            // every detected feature down. Independent smooth calibration;
            // At 10 the previous high-depth selection is unchanged.
            if s.mode == Mode::Soft {
                self.threshold[k] += 3.0 * (10.0 - s.depth) / (s.depth + 1.0);
            }
            // Independent Hard-mode soft-knee threshold model. Equal-level
            // native tone probes show much higher low-frequency thresholds,
            // while Depth moves the threshold through absolute input levels.
            let hard_frequency = (f / 1000.0).powf(1.1);
            self.hard_threshold[k] =
                -42.0 * hard_frequency / (1.0 + hard_frequency) + 8.0 * (3.0 - s.depth);
            // Mild frequency-dependent response, independently tilted at either end.
            let frequency_scale = (1000.0 / f).powf(0.15).clamp(0.5, 2.0);
            let coefficient = |seconds: f32, t: [f32; 2]| {
                let seconds = (seconds * frequency_scale * 10.0_f32.powf(tilt(t))).max(0.0001);
                (-(self.hop as f32) / (self.sample_rate * seconds)).exp()
            };
            self.attack[k] = coefficient(attack_seconds, attack_tilt);
            self.release[k] = coefficient(release_seconds, release_tilt);
            for c in 0..2 {
                self.channels[c].weight[k] = s.weight(f, c);
            }
            self.audition_mask[k] = if s.listen_band >= 0 {
                s.bands[s.listen_band as usize].response(f).abs()
            } else {
                0.0
            };
        }
        self.target_trim = db_to_gain(s.wet_trim_db);
        self.target_gain = db_to_gain(s.output_db);
        // Independent global calibration: depth is a unitless 0..10 control.
        // Soft is relative; Hard retains its separate absolute-level gate.
        self.depth_amount = match s.mode {
            Mode::Soft => 0.03 * s.depth.powf(1.15),
            Mode::Hard => 0.25 * (1.0 - (-2.0 * s.depth).exp()),
        };
        if first {
            self.mix = s.mix;
            self.trim = db_to_gain(s.wet_trim_db);
            self.gain = db_to_gain(s.output_db);
            self.bypass = f32::from(s.bypass);
            self.delta = f32::from(s.delta);
            self.listen = f32::from(s.sidechain_listen);
            self.band_listen = f32::from(s.listen_band >= 0);
        }
        self.cached = Some(s);
    }

    pub fn process_sample(&mut self, input: [f32; 2], sidechain: [f32; 2]) -> [f32; 2] {
        let s = self.cached.unwrap_or_default();
        let clean = |x: f32| {
            if x.is_finite() {
                x.clamp(-64.0, 64.0)
            } else {
                0.0
            }
        };
        let input = input.map(clean);
        let sidechain = sidechain.map(clean);
        let dry = self.dry[self.pos];
        let delayed_key = self.sidechain_delay[self.pos];
        self.dry[self.pos] = input;
        self.sidechain_delay[self.pos] = sidechain;
        // Store in L/R so automating stereo mode never reinterprets an old input ring.
        let mut wet = [0.0; 2];
        let mut audition = [0.0; 2];
        for c in 0..2 {
            let ch = &mut self.channels[c];
            wet[c] = ch.output[self.pos];
            ch.output[self.pos] = 0.0;
            audition[c] = ch.audition[self.pos];
            ch.audition[self.pos] = 0.0;
            ch.input[self.pos] = input[c];
            ch.key[self.pos] = if s.external_sidechain {
                sidechain[c]
            } else {
                input[c]
            };
        }
        self.pos = (self.pos + 1) % self.n;
        self.filled = (self.filled + 1).min(self.n);
        self.until_frame -= 1;
        if self.until_frame == 0 {
            self.render_frame(&s);
            self.until_frame = self.hop;
            self.frame_ready = true;
        }

        let a = self.smooth;
        self.mix += a * (s.mix - self.mix);
        self.trim += a * (self.target_trim - self.trim);
        self.gain += a * (self.target_gain - self.gain);
        self.bypass += a * (f32::from(s.bypass) - self.bypass);
        self.delta += a * (f32::from(s.delta) - self.delta);
        self.listen += a * (f32::from(s.sidechain_listen) - self.listen);
        self.band_listen += a * (f32::from(s.listen_band >= 0) - self.band_listen);
        std::array::from_fn(|c| {
            let mixed = dry[c] + self.mix * (wet[c] * self.trim - dry[c]);
            let removed = self.mix * self.trim * (dry[c] - wet[c]);
            let monitored = mixed + self.delta * (removed - mixed);
            let monitored = monitored + self.band_listen * (audition[c] - monitored);
            let monitored = monitored + self.listen * (delayed_key[c] - monitored);
            let processed = monitored * self.gain;
            processed + self.bypass * (dry[c] - processed)
        })
    }

    fn render_frame(&mut self, s: &Settings) {
        let bins = self.n / 2 + 1;
        let mid_side = s.stereo_mode == StereoMode::MidSide && !self.mono;
        let norm = 2.0 / self.window.iter().sum::<f32>();
        for i in 0..self.n {
            let p = (self.pos + i) % self.n;
            let (left, right) = (self.channels[0].input[p], self.channels[1].input[p]);
            let (kl, kr) = (self.channels[0].key[p], self.channels[1].key[p]);
            let main = if mid_side {
                [(left + right) * 0.5, (left - right) * 0.5]
            } else {
                [left, right]
            };
            let key = if mid_side {
                [(kl + kr) * 0.5, (kl - kr) * 0.5]
            } else {
                [kl, kr]
            };
            for c in 0..2 {
                self.channels[c].spectrum[i] = Complex32::new(main[c] * self.window[i], 0.0);
                self.channels[c].detector[i] = Complex32::new(key[c] * self.window[i], 0.0);
            }
        }
        for ch in &mut self.channels {
            self.forward
                .process_with_scratch(&mut ch.spectrum, &mut self.scratch);
            self.forward
                .process_with_scratch(&mut ch.detector, &mut self.scratch);
        }
        // Analyze each channel's spectral context independently. Sharing raw
        // power first can hide a quieter channel's resonances behind a louder
        // opposite-channel peak, even when both need the same linked filter.
        for k in 0..bins {
            for c in 0..2 {
                let detector_channel = if self.mono { 0 } else { c };
                let magnitude = self.channels[detector_channel].detector[k]
                    .norm_sqr()
                    .sqrt()
                    * norm;
                self.channels[c].magnitude_db[k] = gain_to_db(magnitude).max(-150.0);
            }
        }
        self.meter.peak_reduction_db = 0.0;
        for ch in &mut self.channels {
            let peak = ch.magnitude_db.iter().copied().fold(-150.0_f32, f32::max);
            let floor = (peak - if s.mode == Mode::Soft { 40.0 } else { 60.0 }).max(-140.0);
            let reference = ch.reference.analyze(
                &ch.magnitude_db,
                &self.reference_lower,
                &self.reference_upper,
            );
            for (k, &baseline) in reference.iter().enumerate() {
                let prominence = (ch.magnitude_db[k] - baseline - self.threshold[k]).max(0.0)
                    * self.contrast_scale[k];
                let activity = if s.mode == Mode::Soft {
                    // Relative activity independently modeled from 20/40 dB
                    // context probes. Hard retains its old safety floor and
                    // separate absolute-level gate; these probes cover Soft.
                    let x = ((ch.magnitude_db[k] - floor) / 32.0).clamp(0.0, 1.0);
                    x * x * (3.0 - 2.0 * x)
                } else {
                    ((ch.magnitude_db[k] - floor - 3.0) / 12.0).clamp(0.0, 1.0)
                };
                let requested = match s.mode {
                    Mode::Soft => prominence * self.depth_amount,
                    Mode::Hard => {
                        let excess = (ch.magnitude_db[k] - self.hard_threshold[k]) / 5.0;
                        let knee = if excess > 20.0 {
                            excess
                        } else {
                            excess.exp().ln_1p()
                        };
                        // Contrast selects tonal activity rather than limiting
                        // every loud peak to the same relative reduction.
                        5.0 * knee * self.depth_amount * (prominence / 3.0).clamp(0.0, 1.0)
                    }
                };
                // Partially filled startup windows invent broad spectral
                // leakage. Wait for real context before deciding what rings.
                ch.requested[k] = if self.filled < self.n || k == 0 || peak < -115.0 {
                    0.0
                } else {
                    requested * activity * self.detail_strength[k]
                };
            }
        }
        // Full linking preserves the stronger independently requested cut in
        // each bin. Partial linking crossfeeds only the extra reduction, so
        // linking cannot weaken a resonance already detected by either side.
        for k in 0..bins {
            let shared = self.channels[0].requested[k].max(self.channels[1].requested[k]);
            for ch in &mut self.channels {
                ch.requested[k] += s.link * (shared - ch.requested[k]);
            }
        }
        for ch in &mut self.channels {
            for k in 0..bins {
                let mut spread = ch.requested[k];
                let lower = self.spread_lower[k];
                let upper = self.spread_upper[k];
                let start = self.spread_kernel_offsets[k] + lower - self.spread_kernel_lower[k];
                let weights = &self.spread_kernel[start..start + upper - lower];
                for (&requested, &weight) in ch.requested[lower..upper].iter().zip(weights) {
                    spread = spread.max(requested * weight);
                }
                let target = if k == 0 {
                    0.0
                } else {
                    s.limit_reduction(spread * ch.weight[k])
                };
                let a = if target > ch.reduction[k] {
                    self.attack[k]
                } else {
                    self.release[k]
                };
                ch.reduction[k] = s.limit_reduction(target + a * (ch.reduction[k] - target));
                self.meter.peak_reduction_db = self.meter.peak_reduction_db.max(ch.reduction[k]);
                ch.filter[k] =
                    Complex32::new(-ch.reduction[k] * std::f32::consts::LN_10 / 20.0, 0.0);
            }
            for k in 1..self.n / 2 {
                ch.filter[self.n - k] = ch.filter[k];
            }
            if s.phase == Phase::Minimum {
                // Real cepstrum -> causal minimum-phase spectrum, recomputed each frame.
                self.inverse
                    .process_with_scratch(&mut ch.filter, &mut self.scratch);
                for i in 0..self.n {
                    ch.filter[i] *= if i == 0 || i == self.n / 2 {
                        1.0 / self.n as f32
                    } else if i < self.n / 2 {
                        2.0 / self.n as f32
                    } else {
                        0.0
                    };
                }
                self.forward
                    .process_with_scratch(&mut ch.filter, &mut self.scratch);
            }
            for k in 0..self.n {
                let filtered = ch.spectrum[k] * ch.filter[k].exp();
                let bin = k.min(self.n - k);
                ch.detector[k] = (ch.spectrum[k] - filtered) * self.audition_mask[bin];
                ch.spectrum[k] = filtered;
            }
            self.inverse
                .process_with_scratch(&mut ch.spectrum, &mut self.scratch);
            // Skip the audition IFFT unless the fade is audible or audition is requested.
            if s.listen_band >= 0 || self.band_listen > 0.0001 {
                self.inverse
                    .process_with_scratch(&mut ch.detector, &mut self.scratch);
            } else {
                ch.detector.fill(Complex32::default());
            }
        }
        let normalization = 2.0 / (self.n * self.quality.overlap()) as f32;
        for i in 0..self.n {
            let p = (self.pos + i) % self.n;
            let scale = self.window[i] * normalization;
            let mut wet = [
                self.channels[0].spectrum[i].re,
                self.channels[1].spectrum[i].re,
            ];
            let mut audition = [
                self.channels[0].detector[i].re,
                self.channels[1].detector[i].re,
            ];
            if mid_side {
                wet = [wet[0] + wet[1], wet[0] - wet[1]];
                audition = [audition[0] + audition[1], audition[0] - audition[1]];
            }
            for c in 0..2 {
                self.channels[c].output[p] += wet[c] * scale;
                self.channels[c].audition[p] += audition[c] * scale;
            }
        }
        for i in 0..METER_BINS {
            let f = 20.0 * 1000.0_f32.powf(i as f32 / (METER_BINS - 1) as f32);
            let k = ((f * self.n as f32 / self.sample_rate).round() as usize).min(bins - 1);
            self.meter.input_db[i] =
                self.channels[0].magnitude_db[k].max(self.channels[1].magnitude_db[k]);
            for c in 0..2 {
                self.meter.reduction_db[c][i] = -gain_to_db(
                    1.0 - self.mix + self.mix * db_to_gain(-self.channels[c].reduction[k]),
                );
            }
        }
    }
}
