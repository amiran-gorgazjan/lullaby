//! Shared audio fixtures and output measurements; independent of detector meters.
use lullaby_dsp::{Engine, Settings};
use rustfft::{FftPlanner, num_complex::Complex32};
use std::{fs, path::Path};

pub const VOCAL_BANDS: [(f32, f32); 3] = [(950.0, 1150.0), (150.0, 500.0), (3000.0, 8000.0)];

pub struct Audio {
    pub sample_rate: u32,
    pub samples: Vec<[f32; 2]>,
}

/// Deliberately limited to the stereo PCM16 format of the checked-in fixtures.
pub fn read_fixture(name: &str) -> Audio {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    let bytes = fs::read(path).expect("read WAV fixture");
    assert_eq!(&bytes[..4], b"RIFF");
    assert_eq!(&bytes[8..12], b"WAVE");
    let u16_at = |i| u16::from_le_bytes(bytes[i..i + 2].try_into().unwrap());
    let u32_at = |i| u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap());
    let (mut sample_rate, mut samples) = (None, None);
    let mut pos = 12;
    while pos + 8 <= bytes.len() {
        let size = u32_at(pos + 4) as usize;
        let data = &bytes[pos + 8..pos + 8 + size];
        match &bytes[pos..pos + 4] {
            b"fmt " => {
                assert!(size >= 16);
                assert_eq!(u16_at(pos + 8), 1, "PCM encoding");
                assert_eq!(u16_at(pos + 10), 2, "stereo");
                assert_eq!(u16_at(pos + 20), 4, "frame size");
                assert_eq!(u16_at(pos + 22), 16, "PCM16");
                sample_rate = Some(u32_at(pos + 12));
            }
            b"data" => {
                assert_eq!(size % 4, 0);
                samples = Some(
                    data.chunks_exact(4)
                        .map(|frame| {
                            [0, 2].map(|i| {
                                i16::from_le_bytes([frame[i], frame[i + 1]]) as f32 / 32768.0
                            })
                        })
                        .collect(),
                );
            }
            _ => {}
        }
        pos += 8 + size + size % 2;
    }
    Audio {
        sample_rate: sample_rate.unwrap(),
        samples: samples.unwrap(),
    }
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

/// Float WAV keeps the render's exact level, including peaks above 0 dBFS.
pub fn write_render(audio: &Audio, path: &Path) {
    use std::io::Write;
    let mut file = fs::File::create(path).unwrap();
    let data_len = u32::try_from(audio.samples.len() * 8).unwrap();
    file.write_all(b"RIFF").unwrap();
    file.write_all(&(48 + data_len).to_le_bytes()).unwrap();
    file.write_all(b"WAVEfmt \x10\0\0\0\x03\0\x02\0").unwrap();
    file.write_all(&audio.sample_rate.to_le_bytes()).unwrap();
    file.write_all(&(audio.sample_rate * 8).to_le_bytes())
        .unwrap();
    file.write_all(b"\x08\0\x20\0fact\x04\0\0\0").unwrap();
    file.write_all(&(audio.samples.len() as u32).to_le_bytes())
        .unwrap();
    file.write_all(b"data").unwrap();
    file.write_all(&data_len.to_le_bytes()).unwrap();
    let data: Vec<_> = audio
        .samples
        .iter()
        .flatten()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    file.write_all(&data).unwrap();
}
