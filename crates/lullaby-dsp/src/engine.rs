use crate::detector::PeakReference;
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
    reference: PeakReference,
    magnitude_db: Vec<f32>,
    reduction: Vec<f32>,
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
            reference: PeakReference::new(n / 2 + 1),
            magnitude_db: vec![0.0; n / 2 + 1],
            reduction: vec![0.0; n / 2 + 1],
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
    reference_guard: Vec<usize>,
    threshold: Vec<f32>,
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
            reference_guard: vec![0; n / 2 + 1],
            threshold: vec![0.0; n / 2 + 1],
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
        for k in 0..=self.n / 2 {
            let f = (k as f32 * self.sample_rate / self.n as f32).max(20.0);
            let low = 1.0 / (1.0 + (f / 500.0).powi(2));
            let high = 1.0 / (1.0 + (2000.0 / f).powi(2));
            let tilt = |t: [f32; 2]| t[0] * low + t[1] * high;
            let detail = (s.detail + 0.5 * tilt(s.detail_tilt)).clamp(0.0, 1.0);
            let extent = (1.25 - 0.5 * detail).exp2();
            self.reference_lower[k] = (k as f32 / extent).floor() as usize;
            self.reference_upper[k] = ((k as f32 * extent).ceil() as usize + 1).min(self.n / 2 + 1);
            self.reference_guard[k] = ((k as f32 * (0.3 - 0.25 * detail)).round() as usize).max(3);
            // Detail tilt must affect detection selectivity as well as bandwidth.
            // Higher depth gradually admits less prominent peaks, while the
            // default leaves ordinary windowed sinusoidal lobes alone.
            self.threshold[k] = (5.0 - 2.0 * detail - (s.depth - 6.0).max(0.0) * 0.15).max(0.5);
            // Mild frequency-dependent response, independently tilted at either end.
            let frequency_scale = (1000.0 / f).powf(0.15).clamp(0.5, 2.0);
            let coefficient = |ms: f32, t: [f32; 2]| {
                let seconds = (ms * 0.001 * frequency_scale * 4.0_f32.powf(tilt(t))).max(0.0001);
                (-(self.hop as f32) / (self.sample_rate * seconds)).exp()
            };
            self.attack[k] = coefficient(s.attack_ms, s.attack_tilt);
            self.release[k] = coefficient(s.release_ms, s.release_tilt);
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
        // Preserve the host's 0..24 scale but give it a useful gentle region:
        // depth 0/6/12/24 -> 0/0.25/0.75/3.75 times detected excess in dB.
        self.depth_amount = ((s.depth / 6.0).exp2() - 1.0) * 0.25;
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
        // Link detector powers, rather than phase-cancellable waveforms.
        for k in 0..bins {
            let power = [
                self.channels[0].detector[k].norm_sqr(),
                self.channels[1].detector[k].norm_sqr(),
            ];
            let shared = if self.mono {
                power[0]
            } else {
                (power[0] + power[1]) * 0.5
            };
            for (c, p) in power.iter().enumerate() {
                let linked = *p + s.link * (shared - *p);
                let magnitude = linked.max(0.0).sqrt() * norm;
                self.channels[c].magnitude_db[k] = gain_to_db(magnitude).max(-150.0);
            }
        }
        self.meter.peak_reduction_db = 0.0;
        for ch in &mut self.channels {
            let peak = ch.magnitude_db.iter().copied().fold(-150.0_f32, f32::max);
            let floor = (peak - 60.0).max(-140.0);
            let reference = ch.reference.analyze(
                &ch.magnitude_db,
                &self.reference_lower,
                &self.reference_upper,
                &self.reference_guard,
            );
            for (k, &baseline) in reference.iter().enumerate() {
                let prominence = (ch.magnitude_db[k] - baseline - self.threshold[k]).max(0.0);
                let activity = ((ch.magnitude_db[k] - floor - 3.0) / 12.0).clamp(0.0, 1.0);
                let requested = match s.mode {
                    Mode::Soft => prominence * self.depth_amount,
                    Mode::Hard => {
                        prominence.min((ch.magnitude_db[k] + 24.0 + s.depth * 2.0).max(0.0))
                            * s.depth
                            * 0.2
                    }
                };
                // Partially filled startup windows invent broad spectral
                // leakage. Wait for real context before deciding what rings.
                let target = if self.filled < self.n || k == 0 || peak < -115.0 {
                    0.0
                } else {
                    (requested * activity * ch.weight[k]).min(s.max_cut_db)
                };
                let a = if target > ch.reduction[k] {
                    self.attack[k]
                } else {
                    self.release[k]
                };
                ch.reduction[k] =
                    (target + a * (ch.reduction[k] - target)).clamp(0.0, s.max_cut_db);
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
