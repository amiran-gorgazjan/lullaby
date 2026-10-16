//! Deterministic release-build CPU and output probe. See docs/performance.md.
use lullaby_dsp::{Band, Engine, Mode, Phase, Quality, Settings, StereoMode};
use serde_json::json;
use std::{hint::black_box, io::Write, time::Instant};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let capture = args.windows(2).find(|a| a[0] == "--capture").map(|a| &a[1]);
    let frames: usize = args
        .windows(2)
        .find(|a| a[0] == "--frames")
        .map(|a| a[1].parse().expect("frame count"))
        .unwrap_or(24000);
    assert!(frames >= 4096);
    let mut audio = capture.map(|p| std::fs::File::create(p).expect("capture output"));
    let mut results = Vec::new();
    for (name, rate, mono, settings, automation) in cases() {
        let mut engine = Engine::new(rate, settings.quality, mono);
        engine.set_settings(settings);
        let input = signal(frames + 8192, rate, mono);
        for &(main, key) in &input[..8192] {
            black_box(engine.process_sample(main, key));
        }
        let mut output = vec![[0.0_f32; 2]; frames];
        let mut block_ns = Vec::with_capacity(frames.div_ceil(128));
        let start = Instant::now();
        for (block, samples) in input[8192..].chunks(128).enumerate() {
            let block_start = Instant::now();
            let mut s = settings;
            if automation {
                let p = (block % 17) as f32 / 16.0;
                s.depth = 2.0 + 7.0 * p;
                s.attack = 2.0 + 5.0 * p;
                s.bands[0].frequency = 300.0 + 3700.0 * p;
                s.external_sidechain = block % 13 < 7;
                s.stereo_mode = if block % 11 < 5 {
                    StereoMode::MidSide
                } else {
                    StereoMode::LeftRight
                };
                s.listen_band = if block % 19 < 5 { 0 } else { -1 };
                s.phase = if block % 23 < 11 {
                    Phase::Minimum
                } else {
                    Phase::Linear
                };
                s.link = if block % 7 < 3 { 1.0 } else { 0.35 };
            }
            engine.set_settings(black_box(s));
            for (i, &(main, key)) in samples.iter().enumerate() {
                output[block * 128 + i] = engine.process_sample(black_box(main), black_box(key));
            }
            block_ns.push(block_start.elapsed().as_nanos() as u64);
        }
        let elapsed_ns = start.elapsed().as_nanos() as u64;
        black_box(&output);
        assert!(output.iter().flatten().all(|x| x.is_finite()));
        if let Some(file) = &mut audio {
            for value in output.iter().flatten() {
                file.write_all(&value.to_le_bytes()).unwrap();
            }
            let meter = engine.meter();
            for value in meter
                .input_db
                .iter()
                .chain(meter.reduction_db.iter().flatten())
                .chain([&meter.peak_reduction_db])
            {
                file.write_all(&value.to_le_bytes()).unwrap();
            }
        }
        block_ns.sort_unstable();
        let percentile = |p: f64| block_ns[((block_ns.len() - 1) as f64 * p).round() as usize];
        results.push(json!({"case":name,"frames":frames,"sample_rate":rate,"elapsed_ns":elapsed_ns,
            "ns_per_sample":elapsed_ns as f64 / frames as f64,
            "realtime_percent":elapsed_ns as f64 / (frames as f64 / rate as f64 * 1e9) * 100.0,
            "block_p50_ns":percentile(0.5),"block_p95_ns":percentile(0.95),"block_p99_ns":percentile(0.99)}));
    }
    println!("{}", json!({"schema":1,"cases":results}));
}

fn cases() -> Vec<(&'static str, f32, bool, Settings, bool)> {
    let default = Settings::default();
    let hard = Settings {
        mode: Mode::Hard,
        depth: 10.0,
        ..default
    };
    let mut banded = hard;
    banded.bands[0] = Band::bell(1056.0, 8.0, 1.3);
    banded.bands[1] = Band::bell(6500.0, -5.0, 2.0);
    banded.bands[0].focus = -0.5;
    vec![
        ("default_stereo_48k", 48000.0, false, default, false),
        ("default_stereo_96k", 96000.0, false, default, false),
        ("default_mono", 48000.0, true, default, false),
        ("hard_banded", 48000.0, false, banded, false),
        (
            "unlinked",
            48000.0,
            false,
            Settings {
                link: 0.0,
                ..banded
            },
            false,
        ),
        (
            "partial_link",
            48000.0,
            false,
            Settings {
                link: 0.35,
                ..banded
            },
            false,
        ),
        (
            "sidechain",
            48000.0,
            false,
            Settings {
                external_sidechain: true,
                ..banded
            },
            false,
        ),
        (
            "mid_side",
            48000.0,
            false,
            Settings {
                stereo_mode: StereoMode::MidSide,
                ..banded
            },
            false,
        ),
        (
            "linear_phase",
            48000.0,
            false,
            Settings {
                phase: Phase::Linear,
                ..banded
            },
            false,
        ),
        (
            "band_audition",
            48000.0,
            false,
            Settings {
                listen_band: 0,
                ..banded
            },
            false,
        ),
        (
            "high_quality",
            48000.0,
            false,
            Settings {
                quality: Quality::High,
                ..default
            },
            false,
        ),
        (
            "ultra_quality",
            48000.0,
            false,
            Settings {
                quality: Quality::Ultra,
                ..default
            },
            false,
        ),
        (
            "low_latency",
            48000.0,
            false,
            Settings {
                quality: Quality::LowLatency,
                ..default
            },
            false,
        ),
        (
            "wide_detail",
            48000.0,
            false,
            Settings {
                detail: 0.0,
                ..banded
            },
            false,
        ),
        (
            "narrow_detail",
            48000.0,
            false,
            Settings {
                detail: 10.0,
                ..banded
            },
            false,
        ),
        ("automated", 48000.0, false, banded, true),
    ]
}

fn signal(frames: usize, rate: f32, mono: bool) -> Vec<([f32; 2], [f32; 2])> {
    let mut state = 0x9182_abcd_u32;
    (0..frames)
        .map(|i| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let noise = state as i32 as f32 / i32::MAX as f32 * 0.025;
            let t = i as f32 / rate;
            let sine = |f: f32| (std::f32::consts::TAU * f * t).sin();
            let saw = |f: f32| 2.0 * (f * t).fract() - 1.0;
            let envelope = if (i / 4096) % 4 == 3 { 0.05 } else { 1.0 };
            let left = envelope * (0.12 * saw(110.0) + 0.19 * sine(1056.0) + noise);
            let right = if mono {
                left
            } else {
                envelope * (0.1 * saw(146.8) + 0.16 * sine(3217.0) - noise)
            };
            (
                [left, right],
                [0.2 * sine(680.0) + noise, 0.17 * sine(4480.0) - noise],
            )
        })
        .collect()
}
