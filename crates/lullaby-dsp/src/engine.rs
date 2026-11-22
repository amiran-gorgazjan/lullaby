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
    #[cfg(feature = "dsp-diagnostics")]
    trace: crate::diagnostics::TraceState,
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
    // Bounded at construction; index construction never grows past this bin
    // count and is abandoned early when the frame is too dense to benefit.
    requested_nonzero: Vec<usize>,
    requested_nonzero_prefix: Vec<usize>,
    weight: Vec<f32>,
}

impl Channel {
    fn new(n: usize) -> Self {
        Self {
            #[cfg(feature = "dsp-diagnostics")]
            trace: crate::diagnostics::TraceState::new(n / 2 + 1),
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
            requested_nonzero: Vec::with_capacity(n / 2 + 1),
            requested_nonzero_prefix: vec![0; n / 2 + 2],
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
    #[cfg(test)]
    reference_filter: bool,
    #[cfg(test)]
    reference_linked: bool,
    #[cfg(test)]
    shared_spread_frames: usize,
    #[cfg(test)]
    shared_filter_frames: usize,
    #[cfg(feature = "dsp-diagnostics")]
    trace_frames: u64,
    #[cfg(feature = "dsp-diagnostics")]
    trace_settings: Settings,
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
    // Common post-link envelope, allocated once and used only for exact matches.
    shared_spread: Vec<f32>,
    kernel_peak: Vec<f32>,
    kernel_peak_distance_squared: Vec<f32>,
    kernel_exponent: Vec<f32>,
    kernel_scale: Vec<f32>,
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
    /// Whether the engine can process audio at the requested sample rate in Hz.
    pub fn supports_sample_rate(sample_rate: f64) -> bool {
        (8_000.0..=384_000.0).contains(&sample_rate)
    }

    pub fn new(sample_rate: f32, quality: Quality, mono: bool) -> Self {
        assert!(Self::supports_sample_rate(f64::from(sample_rate)));
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
            #[cfg(test)]
            reference_filter: false,
            #[cfg(test)]
            reference_linked: false,
            #[cfg(test)]
            shared_spread_frames: 0,
            #[cfg(test)]
            shared_filter_frames: 0,
            #[cfg(feature = "dsp-diagnostics")]
            trace_frames: 0,
            #[cfg(feature = "dsp-diagnostics")]
            trace_settings: Settings::default(),
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
            shared_spread: vec![0.0; n / 2 + 1],
            kernel_peak: vec![0.0; n / 2 + 1],
            kernel_peak_distance_squared: vec![0.0; n / 2 + 1],
            kernel_exponent: vec![0.0; n / 2 + 1],
            kernel_scale: vec![0.0; n / 2 + 1],
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

    /// Borrow the latest actual analysis states; see `DiagnosticFrame` for the
    /// timing, channel, reset, and lifetime contract. Only compiled for diagnostics.
    #[cfg(feature = "dsp-diagnostics")]
    pub fn diagnostic_frame(&self) -> Option<crate::DiagnosticFrame<'_>> {
        (self.trace_frames > 0).then(|| crate::DiagnosticFrame {
            frame_index: self.trace_frames,
            input_end_sample: self.trace_frames * self.hop as u64,
            valid_input_samples: (self.trace_frames * self.hop as u64).min(self.n as u64) as usize,
            fft_size: self.n,
            hop_size: self.hop,
            sample_rate: self.sample_rate,
            mono: self.mono,
            settings: self.trace_settings,
            channels: std::array::from_fn(|c| {
                let ch = &self.channels[c];
                crate::DiagnosticChannel {
                    magnitude_db: &ch.magnitude_db,
                    reference_db: ch.reference.values(),
                    normalized_power_prefix: ch.reference.normalized_power_prefix(),
                    prominence_db: &ch.trace.prominence,
                    activity: &ch.trace.activity,
                    requested_db: &ch.trace.requested,
                    linked_db: &ch.requested,
                    spread_db: &ch.trace.spread,
                    target_db: &ch.trace.target,
                    smoothed_db: &ch.reduction,
                    log_filter: &ch.filter[..self.n / 2 + 1],
                    peak_db: ch.trace.peak,
                    activity_floor_db: ch.trace.floor,
                }
            }),
        })
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
        #[cfg(feature = "dsp-diagnostics")]
        {
            self.trace_frames = 0;
        }
    }

    /// Call once per host block or automation segment, before `process_sample`.
    /// Quality is fixed at construction; hosts must reactivate to change latency.
    pub fn set_settings(&mut self, settings: Settings) {
        let s = settings.sanitized();
        if self.cached == Some(s) {
            return;
        }
        let first = self.cached.is_none();
        // Invalidate only dependent coefficient groups. Recomputed values use
        // the original arithmetic/order; automation is never deferred.
        let detail_changed = self
            .cached
            .is_none_or(|old| old.detail != s.detail || old.detail_tilt != s.detail_tilt);
        let mode_changed = self.cached.is_none_or(|old| old.mode != s.mode);
        let depth_changed = self.cached.is_none_or(|old| old.depth != s.depth);
        let attack_changed = self
            .cached
            .is_none_or(|old| old.attack != s.attack || old.attack_tilt != s.attack_tilt);
        let release_changed = self
            .cached
            .is_none_or(|old| old.release != s.release || old.release_tilt != s.release_tilt);
        let bands_changed = self.cached.is_none_or(|old| old.bands != s.bands);
        let weight_changed = bands_changed || self.cached.is_none_or(|old| old.focus != s.focus);
        let audition_changed = bands_changed
            || self
                .cached
                .is_none_or(|old| old.listen_band != s.listen_band);
        let threshold_changed = detail_changed || mode_changed || depth_changed;
        let detail_tilt = s.detail_tilt.map(f32::log10);
        let attack_tilt = s.attack_tilt.map(f32::log10);
        let release_tilt = s.release_tilt.map(f32::log10);
        // Map the unitless Attack control exponentially to an onset time.
        let attack_seconds = 0.0001 * 5000.0_f32.powf(s.attack / 10.0);
        // Preserve the low/default range and shorten the upper Release range
        // with a smooth transition above the midpoint.
        let upper_release = ((s.release - 5.0) / 5.0).clamp(0.0, 1.0);
        let upper_release = upper_release * upper_release * (3.0 - 2.0 * upper_release);
        let release_seconds =
            0.005 * 400.0_f32.powf(s.release / 10.0) * 0.35_f32.powf(upper_release);
        // With neutral tilts, all rows share their source normalization.
        // With active tilts, a separate arithmetic-only pass permits vectorized
        // square roots before the scalar exponential loop. Preserve the same
        // float operations and never approximate or quantize a coefficient.
        let shared_width =
            (s.detail_tilt == [1.0; 2]).then(|| 1.0 + 9.0 * (1.0 - s.detail / 10.0).powf(1.5));
        if detail_changed && let Some(width) = shared_width {
            fill_kernel_normalization(
                &self.sqrt_frequency,
                &mut self.kernel_peak,
                &mut self.kernel_peak_distance_squared,
                width,
            );
        }
        for k in 0..=self.n / 2 {
            let f = (k as f32 * self.sample_rate / self.n as f32).max(20.0);
            let low = 1.0 / (1.0 + (f / 500.0).powi(2));
            let high = 1.0 / (1.0 + (2000.0 / f).powi(2));
            let tilt = |t: [f32; 2]| t[0] * low + t[1] * high;
            let detail = (s.detail / 10.0 + 0.5 * tilt(detail_tilt)).clamp(0.0, 1.0);
            if detail_changed {
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
                self.spread_upper[k] = (((center + radius).powi(2) * self.n as f32
                    / self.sample_rate)
                    .ceil() as usize
                    + 1)
                .min(self.n / 2 + 1);
                let inverse_width = 1.0 / spread_width;
                let range = self.spread_lower[k]..self.spread_upper[k];
                if shared_width.is_none() {
                    fill_kernel_normalization(
                        &self.sqrt_frequency[range.clone()],
                        &mut self.kernel_peak[range.clone()],
                        &mut self.kernel_peak_distance_squared[range.clone()],
                        spread_width,
                    );
                }
                let frequencies = &self.sqrt_frequency[range.clone()];
                let peaks = &self.kernel_peak[range.clone()];
                let peak_squared = &self.kernel_peak_distance_squared[range.clone()];
                let exponents = &mut self.kernel_exponent[range.clone()];
                let scales = &mut self.kernel_scale[range.clone()];
                for ((((&frequency, &peak), &squared), exponent), scale) in frequencies
                    .iter()
                    .zip(peaks)
                    .zip(peak_squared)
                    .zip(exponents)
                    .zip(scales)
                {
                    let distance = (frequency - self.sqrt_frequency[k]) * inverse_width;
                    *exponent = squared - distance * distance;
                    *scale = center / peak;
                }
                let start =
                    self.spread_kernel_offsets[k] + range.start - self.spread_kernel_lower[k];
                let output = &mut self.spread_kernel[start..start + range.len()];
                for ((weight, &exponent), &scale) in output
                    .iter_mut()
                    .zip(&self.kernel_exponent[range.clone()])
                    .zip(&self.kernel_scale[range])
                {
                    // Smooth frequency-density skew, normalized to keep the
                    // requested maximum unchanged rather than adding gain.
                    *weight = scale * exponent.exp();
                }
            }
            if detail_changed || mode_changed {
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
            }
            if detail_changed {
                let extent = (1.25 - 0.5 * detail).exp2();
                self.reference_lower[k] = (k as f32 / extent).floor() as usize;
                self.reference_upper[k] =
                    ((k as f32 * extent).ceil() as usize + 1).min(self.n / 2 + 1);
                // A lone peak's mean-power contrast depends on bin density. Keep
                // the smaller low-latency transform on a comparable contrast scale
                // while leaving a flat/noise-like spectrum close to zero contrast.
                let count = (self.reference_upper[k] - self.reference_lower[k]).max(2) as f32;
                self.contrast_scale[k] =
                    ((count * 2048.0 / self.n as f32).ln() / count.ln()).clamp(1.0, 2.0);
            }
            if threshold_changed {
                // Detail tilt affects contrast selection as well as bandwidth.
                // Higher depth gradually admits less prominent spectral features.
                self.threshold[k] = (5.0 - 2.0 * detail - (s.depth - 6.0).max(0.0) * 0.15).max(0.5);
                // Low Depth excludes weaker contrast rather than merely scaling
                // every detected feature down. Independent smooth calibration;
                // At 10 the previous high-depth selection is unchanged.
                if s.mode == Mode::Soft {
                    self.threshold[k] += 3.0 * (10.0 - s.depth) / (s.depth + 1.0);
                }
            }
            if depth_changed {
                // Hard-mode thresholds are higher at low frequencies;
                // Depth shifts the threshold through absolute input levels.
                let hard_frequency = (f / 1000.0).powf(1.1);
                self.hard_threshold[k] =
                    -42.0 * hard_frequency / (1.0 + hard_frequency) + 8.0 * (3.0 - s.depth);
            }
            if attack_changed || release_changed {
                // Mild frequency-dependent response, independently tilted at either end.
                let frequency_scale = (1000.0 / f).powf(0.15).clamp(0.5, 2.0);
                let coefficient = |seconds: f32, t: [f32; 2]| {
                    let seconds = (seconds * frequency_scale * 10.0_f32.powf(tilt(t))).max(0.0001);
                    (-(self.hop as f32) / (self.sample_rate * seconds)).exp()
                };
                if attack_changed {
                    self.attack[k] = coefficient(attack_seconds, attack_tilt);
                }
                if release_changed {
                    self.release[k] = coefficient(release_seconds, release_tilt);
                }
            }
            if weight_changed {
                for c in 0..2 {
                    self.channels[c].weight[k] = s.weight(f, c);
                }
            }
            if audition_changed {
                self.audition_mask[k] = if s.listen_band >= 0 {
                    s.bands[s.listen_band as usize].response(f).abs()
                } else {
                    0.0
                };
            }
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
        #[cfg(feature = "dsp-diagnostics")]
        {
            self.trace_frames += 1;
            self.trace_settings = *s;
        }
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
            #[cfg(feature = "dsp-diagnostics")]
            {
                ch.trace.peak = peak;
                ch.trace.floor = floor;
            }
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
                #[cfg(feature = "dsp-diagnostics")]
                {
                    ch.trace.prominence[k] = prominence;
                    ch.trace.activity[k] = activity;
                    ch.trace.requested[k] = ch.requested[k];
                }
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
        // Full linking usually leaves identical requested cuts, but its float
        // arithmetic can still differ by an ulp. Reuse only actual equal arrays,
        // after both independent detectors have finished. Focus weights and old
        // smoothing states remain independent even while the envelope is shared.
        let share_spread =
            s.link == 1.0 && self.channels[0].requested == self.channels[1].requested;
        #[cfg(test)]
        let share_spread = share_spread && !self.reference_linked;
        #[cfg(test)]
        if share_spread {
            self.shared_spread_frames += 1;
        }
        for c in 0..2 {
            let (previous, current) = self.channels.split_at_mut(c);
            let previous = previous.first();
            let ch = &mut current[0];
            let reuse_spread = share_spread && c == 1;
            let sparse = !reuse_spread
                && index_nonzero_requests(
                    &ch.requested,
                    &mut ch.requested_nonzero,
                    &mut ch.requested_nonzero_prefix,
                    bins / 8,
                );
            // Indexed loads only pay off for very sparse cuts. Dense frames
            // keep the original contiguous traversal for cache/vectorization.
            // Zero cuts have exactly zero contribution through finite Gaussian
            // weights; every nonzero source retains its original product.
            for k in 0..bins {
                let lower = self.spread_lower[k];
                let upper = self.spread_upper[k];
                let start = self.spread_kernel_offsets[k] + lower - self.spread_kernel_lower[k];
                let weights = &self.spread_kernel[start..start + upper - lower];
                let spread = if reuse_spread {
                    self.shared_spread[k]
                } else if sparse {
                    let nonzero = &ch.requested_nonzero
                        [ch.requested_nonzero_prefix[lower]..ch.requested_nonzero_prefix[upper]];
                    maximum_sparse_weighted_request(
                        ch.requested[k],
                        &ch.requested,
                        lower,
                        weights,
                        nonzero,
                    )
                } else {
                    maximum_weighted_request(ch.requested[k], &ch.requested[lower..upper], weights)
                };
                if share_spread && c == 0 {
                    self.shared_spread[k] = spread;
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
                #[cfg(feature = "dsp-diagnostics")]
                {
                    ch.trace.spread[k] = spread;
                    ch.trace.target[k] = target;
                }
                ch.reduction[k] = s.limit_reduction(target + a * (ch.reduction[k] - target));
                self.meter.peak_reduction_db = self.meter.peak_reduction_db.max(ch.reduction[k]);
                ch.filter[k] =
                    Complex32::new(-ch.reduction[k] * std::f32::consts::LN_10 / 20.0, 0.0);
            }
            for k in 1..self.n / 2 {
                ch.filter[self.n - k] = ch.filter[k];
            }
            if s.phase == Phase::Minimum
                && share_spread
                && previous.is_some_and(|previous| previous.reduction == ch.reduction)
            {
                // The actual smoothed reductions must match, not just the new
                // targets: relinking must not erase unequal release histories.
                ch.filter.copy_from_slice(&previous.unwrap().filter);
                #[cfg(test)]
                {
                    self.shared_filter_frames += 1;
                }
            } else if s.phase == Phase::Minimum {
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
            #[cfg(test)]
            if self.reference_filter {
                apply_reference_filter(ch, &self.audition_mask);
            } else {
                apply_symmetric_filter(ch, &self.audition_mask, s.phase);
            }
            #[cfg(not(test))]
            apply_symmetric_filter(ch, &self.audition_mask, s.phase);
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

fn apply_symmetric_filter(ch: &mut Channel, audition_mask: &[f32], phase: Phase) {
    // The input and real cepstrum describe real signals, so the response at
    // negative frequencies is the conjugate of the positive-frequency response.
    // Exponentiating only one side avoids redundant exp/sin/cos calls. Keep each
    // original input FFT bin, rather than forcing its finite-precision mirror.
    if phase == Phase::Minimum {
        apply_filter_pairs::<true>(ch, audition_mask);
    } else {
        apply_filter_pairs::<false>(ch, audition_mask);
    }
}

fn apply_filter_pairs<const MINIMUM_PHASE: bool>(ch: &mut Channel, audition_mask: &[f32]) {
    let n = ch.spectrum.len();
    for (k, &mask) in audition_mask.iter().enumerate() {
        let gain = if MINIMUM_PHASE {
            ch.filter[k].exp()
        } else {
            // Linear phase stores a purely real log gain. Its complex exp has
            // zero phase, so no trigonometry or complex multiply is necessary.
            Complex32::new(ch.filter[k].re.exp(), 0.0)
        };
        let filtered = if MINIMUM_PHASE {
            ch.spectrum[k] * gain
        } else {
            ch.spectrum[k] * gain.re
        };
        ch.detector[k] = (ch.spectrum[k] - filtered) * mask;
        ch.spectrum[k] = filtered;
        if k != 0 && k != n / 2 {
            let negative = n - k;
            let filtered = if MINIMUM_PHASE {
                ch.spectrum[negative] * gain.conj()
            } else {
                ch.spectrum[negative] * gain.re
            };
            ch.detector[negative] = (ch.spectrum[negative] - filtered) * mask;
            ch.spectrum[negative] = filtered;
        }
    }
}

#[cfg(test)]
fn apply_reference_filter(ch: &mut Channel, audition_mask: &[f32]) {
    let n = ch.spectrum.len();
    for k in 0..n {
        let filtered = ch.spectrum[k] * ch.filter[k].exp();
        let bin = k.min(n - k);
        ch.detector[k] = (ch.spectrum[k] - filtered) * audition_mask[bin];
        ch.spectrum[k] = filtered;
    }
}

fn index_nonzero_requests(
    requested: &[f32],
    nonzero: &mut Vec<usize>,
    prefix: &mut [usize],
    limit: usize,
) -> bool {
    debug_assert!(limit > 0);
    debug_assert_eq!(prefix.len(), requested.len() + 1);
    nonzero.clear();
    for (i, &request) in requested.iter().enumerate() {
        prefix[i] = nonzero.len();
        if request != 0.0 {
            nonzero.push(i);
            // The partial index is never consumed by dense traversal. Stop
            // constructing it as soon as indexed loads cannot pay off.
            if nonzero.len() >= limit {
                return false;
            }
        }
    }
    prefix[requested.len()] = nonzero.len();
    true
}

#[inline]
fn maximum_sparse_weighted_request(
    initial: f32,
    requested: &[f32],
    lower: usize,
    weights: &[f32],
    nonzero: &[usize],
) -> f32 {
    let mut maximum = initial;
    for &i in nonzero {
        maximum = maximum.max(requested[i] * weights[i - lower]);
    }
    maximum
}

#[inline]
fn maximum_weighted_request(initial: f32, requested: &[f32], weights: &[f32]) -> f32 {
    debug_assert_eq!(requested.len(), weights.len());
    let mut spread = initial;
    for (&request, &weight) in requested.iter().zip(weights) {
        spread = spread.max(request * weight);
    }
    spread
}

// Kept separate from exp() calls so the source-normalization arithmetic can
// vectorize. Scratch storage is bounded at construction and shared by channels.
fn fill_kernel_normalization(sources: &[f32], peaks: &mut [f32], squared: &mut [f32], width: f32) {
    let inverse_width = 1.0 / width;
    for ((&source, peak), squared) in sources.iter().zip(peaks).zip(squared) {
        *peak = 0.5 * (source + (source * source + 2.0 * width * width).sqrt());
        let distance = (*peak - source) * inverse_width;
        *squared = distance * distance;
    }
}

#[cfg(test)]
mod cache_tests {
    use super::*;

    #[test]
    fn linked_sharing_matches_reference_through_focus_and_history_changes() {
        for quality in [
            Quality::LowLatency,
            Quality::Normal,
            Quality::High,
            Quality::Ultra,
        ] {
            for mono in [false, true] {
                let mut shared = Engine::new(48000.0, quality, mono);
                let mut reference = Engine::new(48000.0, quality, mono);
                reference.reference_linked = true;
                let mut saw_unequal_reductions = false;
                let mut saw_unequal_weights = false;
                for stage in 0..11 {
                    let mut settings = Settings {
                        quality,
                        mode: if stage == 8 { Mode::Soft } else { Mode::Hard },
                        depth: if stage == 9 { 0.0 } else { 10.0 },
                        attack: 1.0,
                        release: 7.0,
                        focus: if stage == 1 { 0.7 } else { 0.0 },
                        link: match stage {
                            4 => 0.35,
                            5 => 0.0,
                            _ => 1.0,
                        },
                        phase: if stage == 7 {
                            Phase::Linear
                        } else {
                            Phase::Minimum
                        },
                        stereo_mode: if stage == 7 {
                            StereoMode::MidSide
                        } else {
                            StereoMode::LeftRight
                        },
                        external_sidechain: stage == 7 || stage == 8,
                        listen_band: if stage == 7 { 0 } else { -1 },
                        max_cut_db: if stage == 8 { 3.0 } else { 18.0 },
                        ..Default::default()
                    };
                    settings.bands[0] = crate::Band::bell(1056.0, 8.0, 1.3);
                    settings.bands[0].focus = if stage == 3 { -0.8 } else { 0.0 };
                    if stage == 10 {
                        shared.reset();
                        reference.reset();
                    }
                    shared.set_settings(settings);
                    reference.set_settings(settings);
                    let start_spread = shared.shared_spread_frames;
                    for i in 0..8192 {
                        let sample = stage * 8192 + i;
                        let t = sample as f32 / 48000.0;
                        // Very different spectral contexts, including a much
                        // quieter opposite-channel resonance. Sharing happens
                        // only after both independent requests are computed.
                        let left = (t * std::f32::consts::TAU * 1056.0).sin() * 0.4;
                        let right = if mono {
                            left
                        } else {
                            (t * std::f32::consts::TAU * 3217.0).sin() * 0.025
                                + (t * std::f32::consts::TAU * 40.0).sin() * 0.2
                        };
                        let input = [left, right];
                        let key = [right * 0.2, left * 0.7];
                        let previous_filter_count = shared.shared_filter_frames;
                        assert_eq!(
                            shared.process_sample(input, key),
                            reference.process_sample(input, key),
                            "quality={quality:?}, mono={mono}, stage={stage}, sample={i}"
                        );
                        if shared.take_frame_ready() {
                            assert!(reference.take_frame_ready());
                            assert_eq!(shared.meter.input_db, reference.meter.input_db);
                            assert_eq!(shared.meter.reduction_db, reference.meter.reduction_db);
                            assert_eq!(
                                shared.meter.peak_reduction_db,
                                reference.meter.peak_reduction_db
                            );
                            for (actual, expected) in
                                shared.channels.iter().zip(&reference.channels)
                            {
                                assert_eq!(actual.magnitude_db, expected.magnitude_db);
                                assert_eq!(actual.requested, expected.requested);
                                assert_eq!(actual.reduction, expected.reduction);
                                assert_eq!(actual.filter, expected.filter);
                                #[cfg(feature = "dsp-diagnostics")]
                                {
                                    assert_eq!(
                                        actual.reference.values(),
                                        expected.reference.values()
                                    );
                                    assert_eq!(actual.trace.requested, expected.trace.requested);
                                    assert_eq!(actual.trace.spread, expected.trace.spread);
                                    assert_eq!(actual.trace.target, expected.trace.target);
                                    assert_eq!(actual.trace.prominence, expected.trace.prominence);
                                    assert_eq!(actual.trace.activity, expected.trace.activity);
                                }
                            }
                            if shared.channels[0].reduction != shared.channels[1].reduction {
                                saw_unequal_reductions = true;
                                assert_eq!(shared.shared_filter_frames, previous_filter_count);
                            }
                            saw_unequal_weights |=
                                shared.channels[0].weight != shared.channels[1].weight;
                        }
                    }
                    if settings.link != 1.0 {
                        assert_eq!(shared.shared_spread_frames, start_spread);
                    }
                }
                assert!(shared.shared_spread_frames > 0);
                assert!(shared.shared_filter_frames > 0);
                assert!(saw_unequal_reductions);
                assert!(saw_unequal_weights);
                assert_eq!(reference.shared_spread_frames, 0);
                assert_eq!(reference.shared_filter_frames, 0);
            }
        }
    }

    #[test]
    fn equal_linked_requests_do_not_discard_unequal_smoothing_history() {
        let mut shared = Engine::new(48000.0, Quality::Normal, false);
        let mut reference = Engine::new(48000.0, Quality::Normal, false);
        reference.reference_linked = true;
        let settings = Settings {
            link: 1.0,
            release: 10.0,
            ..Default::default()
        };
        shared.set_settings(settings);
        reference.set_settings(settings);
        // A prior focus/partial-link interval can leave unequal reductions even
        // after current targets become equal. Silence makes both requests exactly
        // zero while the deliberately different old cuts release independently.
        for engine in [&mut shared, &mut reference] {
            engine.channels[0].reduction[50] = 3.0;
            engine.channels[1].reduction[50] = 6.0;
        }
        for _ in 0..4096 {
            assert_eq!(
                shared.process_sample([0.0; 2], [0.0; 2]),
                reference.process_sample([0.0; 2], [0.0; 2])
            );
        }
        assert!(shared.shared_spread_frames > 0);
        assert_eq!(shared.shared_filter_frames, 0);
        assert_ne!(shared.channels[0].reduction, shared.channels[1].reduction);
        for (actual, expected) in shared.channels.iter().zip(&reference.channels) {
            assert_eq!(actual.reduction, expected.reduction);
            assert_eq!(actual.filter, expected.filter);
        }
    }

    #[test]
    fn paired_linear_filter_matches_full_complex_exp_at_every_bin() {
        for n in [512, 2048] {
            let mut paired = Channel::new(n);
            let mut reference = Channel::new(n);
            let mut mask = vec![0.0; n / 2 + 1];
            for (k, mask) in mask.iter_mut().enumerate() {
                // Include unity, extreme attenuation, DC and Nyquist. Input
                // bins deliberately need not be exact conjugates: reuse the
                // response, never discard either input bin's FFT roundoff.
                let cut = if k % 3 == 0 { 0.0 } else { (k % 61) as f32 };
                let log_gain = Complex32::new(-cut * std::f32::consts::LN_10 / 20.0, 0.0);
                paired.filter[k] = log_gain;
                reference.filter[k] = log_gain;
                if k != 0 && k != n / 2 {
                    paired.filter[n - k] = log_gain;
                    reference.filter[n - k] = log_gain;
                }
                *mask = (k % 19) as f32 / 18.0;
            }
            for k in 0..n {
                let value = Complex32::new((k as f32 * 0.13).sin(), (k as f32 * 0.17).cos());
                paired.spectrum[k] = value;
                reference.spectrum[k] = value;
            }
            apply_symmetric_filter(&mut paired, &mask, Phase::Linear);
            apply_reference_filter(&mut reference, &mask);
            assert_eq!(paired.spectrum, reference.spectrum);
            assert_eq!(paired.detector, reference.detector);
        }
    }

    #[test]
    fn conjugate_filter_matches_reference_through_phase_and_audition_changes() {
        for quality in [
            Quality::LowLatency,
            Quality::Normal,
            Quality::High,
            Quality::Ultra,
        ] {
            for mono in [false, true] {
                let sample_rate = if mono { 96000.0 } else { 48000.0 };
                let mut paired = Engine::new(sample_rate, quality, mono);
                let mut reference = Engine::new(sample_rate, quality, mono);
                reference.reference_filter = true;
                let mut settings = Settings {
                    quality,
                    mode: Mode::Hard,
                    depth: 10.0,
                    attack: 0.0,
                    release: 0.0,
                    ..Default::default()
                };
                settings.bands[0] = crate::Band::bell(1056.0, 8.0, 1.3);
                settings.bands[1] = crate::Band::bell(6500.0, -5.0, 2.0);
                let mut noise = 0x9182_abcd_u32;
                let mut squared_error = 0.0_f64;
                let mut values = 0;
                // Longer than an audition fade, including phase changes while
                // it fades out. Also exercise reset and non-conjugate stereo
                // input histories as L/R <-> M/S and sidechain are automated.
                let stage_len = sample_rate as usize / 8;
                for i in 0..stage_len * 7 {
                    if i % stage_len == 0 {
                        let stage = i / stage_len;
                        settings.phase = if stage.is_multiple_of(2) {
                            Phase::Minimum
                        } else {
                            Phase::Linear
                        };
                        settings.listen_band = if stage == 1 || stage == 4 { 0 } else { -1 };
                        settings.external_sidechain = stage.is_multiple_of(3);
                        settings.stereo_mode = if stage % 3 == 1 {
                            StereoMode::MidSide
                        } else {
                            StereoMode::LeftRight
                        };
                        settings.depth = if stage == 5 { 0.0 } else { 10.0 };
                        if stage == 6 {
                            paired.reset();
                            reference.reset();
                        }
                        paired.set_settings(settings);
                        reference.set_settings(settings);
                    }
                    noise = noise.wrapping_mul(1664525).wrapping_add(1013904223);
                    let random = noise as i32 as f32 / i32::MAX as f32;
                    let input = [
                        (i as f32 * 0.13823).sin() * 0.25 + random * 0.05,
                        (i as f32 * 0.041).sin() * 0.11 - random * 0.03,
                    ];
                    let key = [input[1] * 0.5, input[0] * 0.3];
                    let actual = paired.process_sample(input, key);
                    let expected = reference.process_sample(input, key);
                    for c in 0..2 {
                        let error = actual[c] - expected[c];
                        assert!(error.is_finite());
                        assert!(
                            error.abs() <= 0.00002,
                            "quality={quality:?}, mono={mono}, sample={i}, channel={c}, error={error}"
                        );
                        squared_error += f64::from(error).powi(2);
                        values += 1;
                    }
                    if paired.take_frame_ready() {
                        assert!(reference.take_frame_ready());
                        assert_eq!(paired.meter.input_db, reference.meter.input_db);
                        assert_eq!(paired.meter.reduction_db, reference.meter.reduction_db);
                        assert_eq!(
                            paired.meter.peak_reduction_db,
                            reference.meter.peak_reduction_db
                        );
                        for (actual, expected) in paired.channels.iter().zip(&reference.channels) {
                            assert_eq!(actual.reduction, expected.reduction);
                            assert_eq!(actual.filter, expected.filter);
                            assert_eq!(actual.requested, expected.requested);
                            #[cfg(feature = "dsp-diagnostics")]
                            {
                                assert_eq!(actual.trace.target, expected.trace.target);
                                assert_eq!(actual.trace.spread, expected.trace.spread);
                                assert_eq!(actual.trace.prominence, expected.trace.prominence);
                            }
                        }
                    }
                }
                assert!((squared_error / values as f64).sqrt() <= 0.000001);
            }
        }
    }

    #[test]
    fn sparse_index_rebuilds_after_dense_and_silent_frames_without_growing() {
        let mut requested = [1.0; 64];
        let mut nonzero = Vec::with_capacity(requested.len());
        let mut prefix = vec![usize::MAX; requested.len() + 1];
        let capacity = nonzero.capacity();
        assert!(!index_nonzero_requests(
            &requested,
            &mut nonzero,
            &mut prefix,
            8
        ));
        assert_eq!(nonzero, (0..8).collect::<Vec<_>>());
        requested.fill(0.0);
        requested[63] = f32::MIN_POSITIVE;
        assert!(index_nonzero_requests(
            &requested,
            &mut nonzero,
            &mut prefix,
            8
        ));
        assert_eq!(nonzero, [63]);
        assert_eq!(prefix[..64], [0; 64]);
        assert_eq!(prefix[64], 1);
        requested.fill(0.0);
        assert!(index_nonzero_requests(
            &requested,
            &mut nonzero,
            &mut prefix,
            8
        ));
        assert!(nonzero.is_empty());
        assert_eq!(prefix, [0; 65]);
        assert_eq!(nonzero.capacity(), capacity);
    }

    #[test]
    fn sparse_spread_matches_scalar_across_densities_and_ranges() {
        // All-zero input, every sparsity, both range boundaries, subnormal and
        // extreme finite products. Skipping a zero cannot lose a contribution
        // because requested cuts and Gaussian weights are nonnegative/finite.
        let mut random = 0x9367_ab45_u32;
        let mut requested = vec![0.0; 257];
        let mut weights = vec![0.0; 257];
        let mut nonzero = Vec::with_capacity(requested.len());
        let mut prefix = vec![0; requested.len() + 1];
        for density in [0, 1, 3, 17, 64, 128, 257] {
            for i in 0..requested.len() {
                random = random.wrapping_mul(1664525).wrapping_add(1013904223);
                requested[i] = if (random as usize % 257) < density {
                    match i % 17 {
                        1 => f32::MIN_POSITIVE,
                        2 => f32::MAX,
                        _ => (random >> 8) as f32 * (60.0 / 16777216.0),
                    }
                } else {
                    0.0
                };
                random = random.wrapping_mul(1664525).wrapping_add(1013904223);
                weights[i] = (random >> 8) as f32 / 16777216.0;
            }
            assert!(index_nonzero_requests(
                &requested,
                &mut nonzero,
                &mut prefix,
                requested.len() + 1
            ));
            for lower in 0..=requested.len() {
                for upper in lower..=requested.len() {
                    for initial in [0.0, 0.5, 60.0] {
                        let expected = maximum_weighted_request(
                            initial,
                            &requested[lower..upper],
                            &weights[lower..upper],
                        );
                        let actual = maximum_sparse_weighted_request(
                            initial,
                            &requested,
                            lower,
                            &weights[lower..upper],
                            &nonzero[prefix[lower]..prefix[upper]],
                        );
                        assert_eq!(
                            actual.to_bits(),
                            expected.to_bits(),
                            "density={density}, range={lower}..{upper}, initial={initial}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn alternating_controls_match_forced_full_coefficient_rebuilds() {
        for quality in [Quality::LowLatency, Quality::Normal, Quality::Ultra] {
            let mut cached = Engine::new(96000.0, quality, false);
            let mut rebuilt = Engine::new(96000.0, quality, false);
            let mut s = Settings {
                quality,
                external_sidechain: true,
                ..Default::default()
            };
            for i in 0..16000 {
                if i % 83 == 0 {
                    let j = i / 83;
                    let p = (j % 11) as f32;
                    match j % 16 {
                        0 => s.depth = p,
                        1 => s.detail = p,
                        2 => s.detail_tilt = [0.1 + p * 0.8, 10.0 - p * 0.8],
                        3 => {
                            s.mode = if s.mode == Mode::Soft {
                                Mode::Hard
                            } else {
                                Mode::Soft
                            }
                        }
                        4 => s.attack = p,
                        5 => s.release = p,
                        6 => s.attack_tilt = [0.1 + p * 0.8, 10.0],
                        7 => s.release_tilt = [10.0, 0.1 + p * 0.8],
                        8 => s.focus = p / 5.0 - 1.0,
                        9 => {
                            s.bands[0] =
                                crate::Band::bell(120.0 + p * 1200.0, p - 5.0, 0.3 + p * 0.2)
                        }
                        10 => s.listen_band = if s.listen_band == -1 { 0 } else { -1 },
                        11 => {
                            s.phase = if s.phase == Phase::Minimum {
                                Phase::Linear
                            } else {
                                Phase::Minimum
                            }
                        }
                        12 => s.mix = p / 10.0,
                        13 => s.max_cut_db = p,
                        14 => {
                            s.stereo_mode = if s.stereo_mode == StereoMode::LeftRight {
                                StereoMode::MidSide
                            } else {
                                StereoMode::LeftRight
                            }
                        }
                        _ => {
                            cached.reset();
                            rebuilt.reset();
                        }
                    }
                    if let Some(mut old) = rebuilt.cached {
                        // Force every coefficient group dirty without resetting
                        // audio or control smoothing state. No math is duplicated.
                        old.detail = -1.0;
                        old.depth = -1.0;
                        old.mode = if s.mode == Mode::Soft {
                            Mode::Hard
                        } else {
                            Mode::Soft
                        };
                        old.attack = -1.0;
                        old.release = -1.0;
                        old.focus = -2.0;
                        old.bands[0].frequency = -1.0;
                        old.listen_band = -2;
                        rebuilt.cached = Some(old);
                    }
                    cached.set_settings(s);
                    rebuilt.set_settings(s);
                }
                let x = [
                    (i as f32 * 0.031).sin() * 0.3,
                    (i as f32 * 0.137).sin() * 0.1,
                ];
                assert_eq!(
                    cached.process_sample(x, [x[1], x[0]]).map(f32::to_bits),
                    rebuilt.process_sample(x, [x[1], x[0]]).map(f32::to_bits)
                );
                assert_eq!(
                    cached.meter().peak_reduction_db.to_bits(),
                    rebuilt.meter().peak_reduction_db.to_bits()
                );
            }
        }
    }
}
