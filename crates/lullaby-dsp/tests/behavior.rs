use lullaby_dsp::*;
use std::f32::consts::TAU;

fn signal(i: usize) -> [f32; 2] {
    let t = i as f32 / 48000.0;
    [
        0.2 * (TAU * 1500.0 * t).sin() + 0.03 * (TAU * 300.0 * t).sin(),
        0.1 * (TAU * 3000.0 * t).sin(),
    ]
}

fn render(
    s: Settings,
    count: usize,
    input: impl Fn(usize) -> [f32; 2],
    key: impl Fn(usize) -> [f32; 2],
) -> (Vec<[f32; 2]>, Engine) {
    let mut e = Engine::new(48000.0, s.quality, false);
    e.set_settings(s);
    let output = (0..count)
        .map(|i| e.process_sample(input(i), key(i)))
        .collect();
    (output, e)
}

fn rms(x: &[[f32; 2]], channel: usize) -> f32 {
    (x.iter().map(|s| s[channel].powi(2)).sum::<f32>() / x.len() as f32).sqrt()
}

#[test]
fn unity_reconstructs_every_quality_and_phase_with_reported_latency() {
    for quality in [
        Quality::Normal,
        Quality::High,
        Quality::Ultra,
        Quality::LowLatency,
    ] {
        for phase in [Phase::Minimum, Phase::Linear] {
            let s = Settings {
                depth: 0.0,
                quality,
                phase,
                ..Settings::default()
            };
            let (out, e) = render(s, 12000, signal, |_| [0.0; 2]);
            for (i, actual) in out.iter().enumerate().skip(e.latency_samples()) {
                for (c, value) in actual.iter().enumerate() {
                    assert!(
                        (*value - signal(i - e.latency_samples())[c]).abs() < 2e-5,
                        "{quality:?} {phase:?} sample {i}"
                    );
                }
            }
        }
    }
}

#[test]
fn bypass_is_exact_and_ignores_output_gain() {
    let s = Settings {
        bypass: true,
        output_db: 12.0,
        ..Settings::default()
    };
    let (out, e) = render(s, 8000, signal, |_| [0.0; 2]);
    for (i, actual) in out.iter().enumerate().skip(e.latency_samples()) {
        for (c, value) in actual.iter().enumerate() {
            assert!((*value - signal(i - e.latency_samples())[c]).abs() < 1e-6);
        }
    }
}

#[test]
fn dry_mix_is_latency_aligned() {
    let s = Settings {
        mix: 0.0,
        ..Settings::default()
    };
    let (out, e) = render(s, 8000, signal, |_| [0.0; 2]);
    for (i, actual) in out.iter().enumerate().skip(e.latency_samples()) {
        assert_eq!(*actual, signal(i - e.latency_samples()));
    }
}

#[test]
fn delta_plus_wet_reconstructs_dry() {
    for phase in [Phase::Minimum, Phase::Linear] {
        let s = Settings {
            phase,
            ..Settings::default()
        };
        let (wet, e) = render(s, 16000, signal, |_| [0.0; 2]);
        let (delta, _) = render(Settings { delta: true, ..s }, 16000, signal, |_| [0.0; 2]);
        for i in e.latency_samples()..wet.len() {
            for c in 0..2 {
                assert!(
                    (wet[i][c] + delta[i][c] - signal(i - e.latency_samples())[c]).abs() < 2e-6
                );
            }
        }
    }
}

#[test]
fn native_delta_scales_with_mix_wet_trim_and_output_gain() {
    let s = Settings {
        depth: 10.0,
        phase: Phase::Linear,
        delta: true,
        ..Settings::default()
    };
    let (basis, e) = render(s, 16000, signal, |_| [0.0; 2]);
    assert!(
        rms(&basis[8000..], 0) > 0.01,
        "test must remove audible energy"
    );
    for (mix, wet_trim_db, output_db) in [
        (0.0, 6.0, -6.0),
        (0.5, 0.0, 0.0),
        (1.0, 6.0, 0.0),
        (1.0, 0.0, -6.0),
        (0.5, 6.0, -6.0),
    ] {
        let (delta, _) = render(
            Settings {
                mix,
                wet_trim_db,
                output_db,
                ..s
            },
            16000,
            signal,
            |_| [0.0; 2],
        );
        let factor = mix * db_to_gain(wet_trim_db) * db_to_gain(output_db);
        for i in e.latency_samples()..delta.len() {
            for c in 0..2 {
                assert!(
                    (delta[i][c] - factor * basis[i][c]).abs() < 3e-6,
                    "mix={mix}, wet={wet_trim_db}, output={output_db}, sample={i}, channel={c}"
                );
            }
        }
    }
}

#[test]
fn resonances_are_reduced_and_max_cut_is_respected() {
    let s = Settings {
        max_cut_db: 6.0,
        depth: 10.0,
        phase: Phase::Linear,
        ..Settings::default()
    };
    let (out, e) = render(s, 24000, signal, |_| [0.0; 2]);
    let dry: Vec<_> = (12000..24000).map(signal).collect();
    let ratio = rms(&out[12000..], 0) / rms(&dry, 0);
    assert!(ratio < 0.8, "insufficient suppression: {ratio}");
    assert!(ratio > db_to_gain(-6.0) - 0.02, "overcut: {ratio}");
    assert!(e.meter().peak_reduction_db <= 6.0);
}

#[test]
fn max_cut_off_removes_the_cap_and_preserves_finite_endpoint_state() {
    let mut s = Settings {
        mode: Mode::Hard,
        depth: 10.0,
        detail: 10.0,
        attack: 0.0,
        max_cut_db: MAX_CUT_OFF,
        ..Settings::default()
    };
    s.bands[0] = Band::bell(1500.0, 24.0, 2.0);
    assert_eq!(s.limit_reduction(1000.0), 1000.0);
    let json = serde_json::to_string(&s).unwrap();
    assert_eq!(serde_json::from_str::<Settings>(&json).unwrap(), s);
    for quality in [
        Quality::Normal,
        Quality::High,
        Quality::Ultra,
        Quality::LowLatency,
    ] {
        for phase in [Phase::Minimum, Phase::Linear] {
            let (off, engine) = render(
                Settings {
                    quality,
                    phase,
                    ..s
                },
                16000,
                signal,
                |_| [0.0; 2],
            );
            assert!(
                engine.meter().peak_reduction_db > 36.0,
                "{quality:?}/{phase:?}"
            );
            assert!(off.iter().flatten().all(|x| x.is_finite() && x.abs() < 1.0));
        }
    }
    for limit in [0.0, 3.0, 12.0, 23.99] {
        let (_, engine) = render(
            Settings {
                max_cut_db: limit,
                ..s
            },
            16000,
            signal,
            |_| [0.0; 2],
        );
        assert!(engine.meter().peak_reduction_db <= limit, "cap={limit}");
    }
    assert_eq!(
        Settings {
            max_cut_db: 36.0,
            ..s
        }
        .sanitized()
        .max_cut_db,
        MAX_CUT_OFF
    );
    assert_eq!(
        Settings {
            max_cut_db: f32::NAN,
            ..s
        }
        .sanitized()
        .max_cut_db,
        12.0
    );
}

#[test]
fn relative_activity_retains_loud_high_tones_and_rejects_very_quiet_partials() {
    let s = Settings {
        depth: 10.0,
        max_cut_db: MAX_CUT_OFF,
        phase: Phase::Linear,
        ..Settings::default()
    };
    let upper_gain = |amplitude: f32| {
        let input = |i: usize| {
            let t = i as f32 / 48000.0;
            [0.1 * (TAU * 1056.0 * t).sin() + amplitude * (TAU * 4500.0 * t).sin(); 2]
        };
        let (out, e) = render(s, 146048, input, |_| [0.0; 2]);
        let mut dry = [0.0_f64; 2];
        let mut wet = [0.0_f64; 2];
        for i in 96000..144000 {
            let angle = std::f64::consts::TAU * 4500.0 * i as f64 / 48000.0;
            let basis = [angle.cos(), angle.sin()];
            for j in 0..2 {
                dry[j] += input(i)[0] as f64 * basis[j];
                wet[j] += out[i + e.latency_samples()][0] as f64 * basis[j];
            }
        }
        20.0 * (wet[0].hypot(wet[1]) / dry[0].hypot(dry[1])).log10()
    };
    let loud = upper_gain(0.1);
    let quieter = upper_gain(0.01);
    let quiet = upper_gain(0.001);
    assert!(loud < -8.0, "loud isolated high tone was exempt: {loud}");
    assert!(
        quieter > loud + 2.0,
        "relative activity is insensitive: {loud}/{quieter}"
    );
    assert!(quiet > -0.75, "weak upper partial is overcut: {quiet}");
}

#[test]
fn soft_mode_is_level_invariant_above_noise_floor() {
    let s = Settings::default();
    let (a, _) = render(s, 16000, signal, |_| [0.0; 2]);
    let (b, _) = render(s, 16000, |i| signal(i).map(|v| v * 0.1), |_| [0.0; 2]);
    assert!((rms(&a[8000..], 0) * 0.1 - rms(&b[8000..], 0)).abs() < 1e-4);
}

#[test]
fn hard_mode_responds_to_absolute_level() {
    let s = Settings {
        mode: Mode::Hard,
        depth: 8.0,
        phase: Phase::Linear,
        ..Settings::default()
    };
    let (_, a) = render(s, 16000, signal, |_| [0.0; 2]);
    let (_, b) = render(s, 16000, |i| signal(i).map(|v| v * 0.01), |_| [0.0; 2]);
    assert!(a.meter().peak_reduction_db > b.meter().peak_reduction_db + 1.0);
}

#[test]
fn silent_external_sidechain_does_not_reduce_main_input() {
    let s = Settings {
        external_sidechain: true,
        ..Settings::default()
    };
    let (out, e) = render(s, 12000, signal, |_| [0.0; 2]);
    for (i, actual) in out.iter().enumerate().skip(e.latency_samples()) {
        assert!((actual[0] - signal(i - e.latency_samples())[0]).abs() < 2e-5);
    }
    assert_eq!(e.meter().peak_reduction_db, 0.0);
}

#[test]
fn external_key_triggers_reduction() {
    let s = Settings {
        external_sidechain: true,
        // Deliberately request strong processing. The default must not remove
        // several dB from an otherwise ordinary pair of musical tones.
        depth: 10.0,
        ..Settings::default()
    };
    let (_, e) = render(s, 16000, signal, signal);
    assert!(e.meter().peak_reduction_db > 3.0);
}

#[test]
fn sidechain_listen_is_delayed_and_unprocessed() {
    let s = Settings {
        sidechain_listen: true,
        ..Settings::default()
    };
    let (out, e) = render(s, 8000, |_| [0.0; 2], signal);
    for (i, actual) in out.iter().enumerate().skip(e.latency_samples()) {
        assert_eq!(*actual, signal(i - e.latency_samples()));
    }
}

#[test]
fn fully_linked_channels_use_identical_reduction() {
    let (_, e) = render(Settings::default(), 16000, signal, |_| [0.0; 2]);
    assert_eq!(e.meter().reduction_db[0], e.meter().reduction_db[1]);
}

#[test]
fn mid_side_can_leave_mid_untouched() {
    let s = Settings {
        stereo_mode: StereoMode::MidSide,
        focus: 1.0,
        ..Settings::default()
    };
    let (out, e) = render(s, 12000, |i| [signal(i)[0]; 2], |_| [0.0; 2]);
    for (i, actual) in out.iter().enumerate().skip(e.latency_samples()) {
        for value in actual {
            assert!((*value - signal(i - e.latency_samples())[0]).abs() < 2e-5);
        }
    }
}

#[test]
fn silence_and_nonfinite_values_remain_finite() {
    let s = Settings {
        depth: f32::NAN,
        attack: f32::NEG_INFINITY,
        ..Settings::default()
    };
    let (out, _) = render(
        s,
        10000,
        |i| {
            if i == 5 {
                [f32::NAN, f32::INFINITY]
            } else {
                [0.0; 2]
            }
        },
        |_| [0.0; 2],
    );
    assert!(
        out.iter()
            .flatten()
            .all(|x| x.is_finite() && x.abs() < 1e-8)
    );
}

#[test]
fn reset_removes_old_audio() {
    let (_, mut e) = render(Settings::default(), 8000, signal, |_| [0.0; 2]);
    e.reset();
    e.set_settings(Settings::default());
    for _ in 0..8000 {
        assert_eq!(e.process_sample([0.0; 2], [0.0; 2]), [0.0; 2]);
    }
}

#[test]
fn frequency_masks_and_parallel_bandpasses_work() {
    let mut s = Settings::default();
    s.bands[0] = Band {
        shape: BandShape::BandPass,
        ..Band::bell(300.0, 0.0, 0.5)
    };
    s.bands[1] = Band {
        shape: BandShape::BandPass,
        ..Band::bell(5000.0, 0.0, 0.5)
    };
    assert!(s.weight(300.0, 0) > 0.99);
    assert!(s.weight(5000.0, 0) > 0.99);
    assert!(s.weight(1200.0, 0) < 0.001);
    s.bands[2] = Band {
        shape: BandShape::BandReject,
        ..Band::bell(300.0, 0.0, 0.5)
    };
    assert!(s.weight(300.0, 0) < 0.001);
}

#[test]
fn presets_round_trip_and_partial_presets_use_defaults() {
    for i in 0..FACTORY_PRESETS.len() {
        let s = factory_preset(i);
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<Settings>(&json).unwrap(), s);
    }
    let s: Settings = serde_json::from_str(r#"{"depth": 3.0}"#).unwrap();
    assert_eq!(s.depth, 3.0);
    assert_eq!(s.release, 3.0);
}

#[test]
fn temporal_controls_and_tilt_multipliers_have_explicit_ranges_and_defaults() {
    let s = Settings::default();
    assert_eq!((s.attack, s.release), (5.0, 3.0));
    assert_eq!(s.detail_tilt, [1.0; 2]);
    assert_eq!(s.attack_tilt, [1.0; 2]);
    assert_eq!(s.release_tilt, [1.0; 2]);
    let s = Settings {
        attack: -1.0,
        release: 12.0,
        detail_tilt: [0.0, 100.0],
        attack_tilt: [f32::NAN, f32::INFINITY],
        release_tilt: [0.1, 10.0],
        ..s
    }
    .sanitized();
    assert_eq!((s.attack, s.release), (0.0, 10.0));
    assert_eq!(s.detail_tilt, [0.1, 10.0]);
    assert_eq!(s.attack_tilt, [1.0; 2]);
    assert_eq!(s.release_tilt, [0.1, 10.0]);
    let s = Settings {
        attack: f32::NAN,
        release: f32::NEG_INFINITY,
        ..s
    }
    .sanitized();
    assert_eq!((s.attack, s.release), (5.0, 3.0));
    let json = serde_json::to_value(s).unwrap();
    assert!(json.get("attack").is_some());
    assert!(json.get("release").is_some());
    assert!(json.get("attack_ms").is_none());
    assert!(json.get("release_ms").is_none());
}

fn keyed_sine_level(settings: Settings, frequency: f32, start: f32, end: f32) -> f32 {
    let tone = |i: usize| [0.15 * (TAU * frequency * i as f32 / 48000.0).sin(); 2];
    let key = |i: usize| {
        if (48000..96000).contains(&i) {
            tone(i)
        } else {
            [0.0; 2]
        }
    };
    let (audio, engine) = render(settings, 144000, tone, key);
    let a = (start * 48000.0) as usize + engine.latency_samples();
    let b = (end * 48000.0) as usize + engine.latency_samples();
    gain_to_db(rms(&audio[a..b], 0) / (0.15 / 2.0_f32.sqrt()))
}

#[test]
fn higher_unitless_attack_delays_onset_and_release_extends_recovery() {
    let fast = Settings {
        depth: 10.0,
        attack: 0.0,
        release: 0.0,
        external_sidechain: true,
        ..Settings::default()
    };
    let slow_attack = Settings {
        attack: 10.0,
        ..fast
    };
    let a = keyed_sine_level(fast, 1056.0, 1.08, 1.18);
    let b = keyed_sine_level(slow_attack, 1056.0, 1.08, 1.18);
    assert!(a < -3.0 && b > a + 2.0, "attack fast={a}, slow={b}");
    let slow_release = Settings {
        release: 10.0,
        ..fast
    };
    let a = keyed_sine_level(fast, 1056.0, 2.15, 2.25);
    let b = keyed_sine_level(slow_release, 1056.0, 2.15, 2.25);
    assert!(a > -0.1 && b < a - 2.0, "release fast={a}, slow={b}");
}

#[test]
fn temporal_tilt_multipliers_slow_low_and_high_frequency_onsets() {
    let neutral = Settings {
        depth: 10.0,
        attack: 5.0,
        release: 0.0,
        external_sidechain: true,
        ..Settings::default()
    };
    for (frequency, tilts) in [(120.0, [10.0, 1.0]), (6000.0, [1.0, 10.0])] {
        let a = keyed_sine_level(neutral, frequency, 1.04, 1.09);
        let b = keyed_sine_level(
            Settings {
                attack_tilt: tilts,
                ..neutral
            },
            frequency,
            1.04,
            1.09,
        );
        assert!(b > a + 0.5, "{frequency}Hz neutral={a}, slowed={b}");
    }
}

#[test]
fn depth_and_detail_have_unitless_zero_to_ten_ranges() {
    let s = Settings {
        depth: 24.0,
        detail: 12.0,
        ..Settings::default()
    }
    .sanitized();
    assert_eq!(s.depth, 10.0);
    assert_eq!(s.detail, 10.0);
    let s = Settings {
        depth: f32::NAN,
        detail: f32::NAN,
        ..Settings::default()
    }
    .sanitized();
    assert_eq!(s.depth, 3.0);
    assert_eq!(s.detail, 5.0);
    let s = Settings {
        depth: -1.0,
        detail: -1.0,
        ..Settings::default()
    }
    .sanitized();
    assert_eq!(s.depth, 0.0);
    assert_eq!(s.detail, 0.0);
}
