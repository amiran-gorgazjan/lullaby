use lullaby_dsp::{Engine, Settings, gain_to_db};
use std::f32::consts::TAU;

pub const PROBE_FREQUENCIES: [f32; 5] = [400.0, 800.0, 1600.0, 3200.0, 6400.0];

/// A controlled harmonic instrument: smooth spectral tilt and repeatable phases.
/// The optional 12 dB bump is confined to the 3.2 kHz harmonic.
pub fn harmonic_fixture(sample_rate: f32, resonance: bool) -> Vec<f32> {
    harmonic_fixture_at(sample_rate, 200.0, resonance)
}

pub fn harmonic_fixture_at(sample_rate: f32, fundamental: f32, resonance: bool) -> Vec<f32> {
    let count = sample_rate as usize;
    (0..count)
        .map(|i| {
            let t = i as f32 / sample_rate;
            (1..=64)
                .filter(|h| *h as f32 * fundamental < sample_rate * 0.45)
                .map(|h| {
                    let h = h as f32;
                    let emphasis = if resonance && h == 16.0 { 4.0 } else { 1.0 };
                    0.045 * emphasis / h.powf(0.6)
                        * (TAU * fundamental * h * t + h * h * 0.173).sin()
                })
                .sum()
        })
        .collect()
}

pub fn noise_fixture(count: usize) -> Vec<f32> {
    let mut state = 0x9876_5432_u32;
    (0..count)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            (state as f64 / u32::MAX as f64 * 2.0 - 1.0) as f32 * 0.1
        })
        .collect()
}

pub struct Measurement {
    pub level_db: f32,
    pub tones_db: [f32; 5],
    pub max_reduction_db: f32,
}

pub fn measure(input: &[f32], sample_rate: f32, settings: Settings) -> Measurement {
    let mut engine = Engine::new(sample_rate, settings.quality, true);
    engine.set_settings(settings);
    let latency = engine.latency_samples();
    let mut max_reduction_db = 0.0_f32;
    let output: Vec<_> = input
        .iter()
        .chain(std::iter::repeat_n(&0.0, latency))
        .enumerate()
        .map(|(i, x)| {
            let output = engine.process_sample([*x; 2], [0.0; 2])[0];
            if i >= input.len() / 2 && i < input.len() {
                max_reduction_db = max_reduction_db.max(engine.meter().peak_reduction_db);
            }
            output
        })
        .collect();
    // Analyze a settled half-second. Align dry and wet by the reported latency.
    let start = input.len() / 2;
    let end = input.len();
    let input = &input[start..end];
    let output = &output[start + latency..end + latency];
    let energy = |samples: &[f32]| samples.iter().map(|x| (*x as f64).powi(2)).sum::<f64>();
    let amplitude = |samples: &[f32], frequency: f32| {
        let (mut re, mut im) = (0.0_f64, 0.0_f64);
        for (i, x) in samples.iter().enumerate() {
            let angle = std::f64::consts::TAU * frequency as f64 * i as f64 / sample_rate as f64;
            re += *x as f64 * angle.cos();
            im += *x as f64 * angle.sin();
        }
        re.hypot(im).max(1e-12)
    };
    Measurement {
        level_db: gain_to_db((energy(output) / energy(input)).sqrt() as f32),
        tones_db: PROBE_FREQUENCIES
            .map(|f| gain_to_db((amplitude(output, f) / amplitude(input, f)) as f32)),
        max_reduction_db,
    }
}
