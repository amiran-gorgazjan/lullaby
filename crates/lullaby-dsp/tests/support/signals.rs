use super::audio::Audio;
use std::f64::consts::TAU;

/// A repeatable stereo chord with moving pitches, evolving harmonic balance,
/// independently panned voices, breath noise and short percussive bursts.
pub fn musical_mix(seconds: usize) -> Audio {
    let sample_rate = 48000;
    let mut phases = [0.0; 3];
    let mut random = 0x8234_9761_u32;
    let samples = (0..sample_rate as usize * seconds)
        .map(|i| {
            let t = i as f64 / sample_rate as f64;
            let mut output = [0.0; 2];
            for (v, f) in [139.0, 220.3, 330.1].into_iter().enumerate() {
                phases[v] += TAU * f * (1.0 + 0.003 * (TAU * (4.5 + v as f64) * t).sin())
                    / sample_rate as f64;
                let envelope = (1.0 - (-t * 50.0).exp()) * (0.7 + 0.3 * (t * 2.1 + v as f64).sin());
                let voice: f64 = (1..=32)
                    .map(|h| {
                        let h = h as f64;
                        0.018 / h.powf(0.8)
                            * (1.0 + 0.2 * (t * 0.91 + h * 0.3).sin())
                            * (h * phases[v] + h * h * 0.173).sin()
                    })
                    .sum();
                output[0] += voice * envelope * [1.0, 0.6, 0.3][v];
                output[1] += voice * envelope * [0.3, 0.6, 1.0][v];
            }
            random ^= random << 13;
            random ^= random >> 17;
            random ^= random << 5;
            let noise = random as f64 / u32::MAX as f64 * 2.0 - 1.0;
            let percussion = 0.006 * (-40.0 * ((t + 0.13) % 0.5)).exp();
            output.map(|v| (v + noise * (0.001 + percussion)) as f32)
        })
        .collect();
    Audio {
        sample_rate,
        samples,
    }
}

/// Test signal construction using the peaking-EQ equations in the W3C Audio EQ
/// Cookbook: https://www.w3.org/TR/audio-eq-cookbook/ . This is independent of
/// the production detector and does not model FL Studio's particular EQ.
pub fn peaking_eq(input: &Audio, frequency: f64, gain_db: f64, q: f64) -> Audio {
    let a = 10.0_f64.powf(gain_db / 40.0);
    let w = TAU * frequency / input.sample_rate as f64;
    let alpha = w.sin() / (2.0 * q);
    let a0 = 1.0 + alpha / a;
    let b = [
        (1.0 + alpha * a) / a0,
        -2.0 * w.cos() / a0,
        (1.0 - alpha * a) / a0,
    ];
    let a1 = -2.0 * w.cos() / a0;
    let a2 = (1.0 - alpha / a) / a0;
    let mut state = [[0.0; 2]; 2];
    let samples = input
        .samples
        .iter()
        .map(|frame| {
            std::array::from_fn(|c| {
                let x = frame[c] as f64;
                let y = b[0] * x + state[c][0];
                state[c][0] = b[1] * x - a1 * y + state[c][1];
                state[c][1] = b[2] * x - a2 * y;
                y as f32
            })
        })
        .collect();
    Audio {
        sample_rate: input.sample_rate,
        samples,
    }
}
