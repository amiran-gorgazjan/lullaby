//! Shared generated audio and output measurements; independent of detector meters.
use lullaby_dsp::{Engine, Settings};
use rustfft::{FftPlanner, num_complex::Complex32};

pub struct Audio {
    pub sample_rate: u32,
    pub samples: Vec<[f32; 2]>,
}

pub fn render(audio: &Audio, settings: Settings) -> Audio {
    let mut engine = Engine::new(audio.sample_rate as f32, settings.quality, false);
    engine.set_settings(settings);
    let latency = engine.latency_samples();
    let samples = audio
        .samples
        .iter()
        .copied()
        .chain(std::iter::repeat_n([0.0; 2], latency))
        .enumerate()
        .filter_map(|(i, sample)| {
            // Process every sample: putting skip() after map() can skip the
            // stateful processing calls themselves via Iterator::nth().
            let output = engine.process_sample(sample, [0.0; 2]);
            (i >= latency).then_some(output)
        })
        .collect();
    Audio {
        sample_rate: audio.sample_rate,
        samples,
    }
}

/// Integrated Hann-windowed spectral energy, averaging channel powers rather
/// than summing their waveforms. This measures output, not the engine's meter.
pub fn band_energy(audio: &Audio, bands: &[(f32, f32)]) -> Vec<f64> {
    const N: usize = 8192;
    assert!(
        audio.samples.len() >= N,
        "measurement needs at least one FFT window"
    );
    let fft = FftPlanner::new().plan_fft_forward(N);
    let mut scratch = vec![Complex32::default(); fft.get_inplace_scratch_len()];
    let mut frame = vec![Complex32::default(); N];
    let window: Vec<_> = (0..N)
        .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / N as f32).cos())
        .collect();
    let mut energy = vec![0.0; bands.len()];
    for start in (0..=audio.samples.len() - N).step_by(N / 4) {
        for channel in 0..2 {
            for i in 0..N {
                frame[i] = Complex32::new(audio.samples[start + i][channel] * window[i], 0.0);
            }
            fft.process_with_scratch(&mut frame, &mut scratch);
            for (e, &(lo, hi)) in energy.iter_mut().zip(bands) {
                let from = (lo * N as f32 / audio.sample_rate as f32).ceil() as usize;
                let to = (hi * N as f32 / audio.sample_rate as f32).ceil() as usize;
                *e += frame[from..to.min(N / 2 + 1)]
                    .iter()
                    .map(|v| v.norm_sqr() as f64)
                    .sum::<f64>();
            }
        }
    }
    energy
}

pub fn ratio_db(output: f64, input: f64) -> f64 {
    10.0 * (output.max(1e-30) / input.max(1e-30)).log10()
}

pub fn energy(audio: &Audio) -> f64 {
    audio
        .samples
        .iter()
        .flatten()
        .map(|v| f64::from(*v).powi(2))
        .sum()
}
