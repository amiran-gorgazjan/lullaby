#[path = "support/audio.rs"]
#[allow(dead_code)]
mod audio;
#[path = "support/signals.rs"]
mod signals;
use audio::{Audio, band_energy, energy, ratio_db, render};
use lullaby_dsp::{Phase, Settings};
use signals::{musical_mix, peaking_eq};

#[test]
#[cfg(feature = "vocal-fixtures")]
fn singing_eq_peak_receives_selective_reduction() {
    use audio::{VOCAL_BANDS, read_fixture};
    use lullaby_dsp::Quality;
    let original = read_fixture("janice_singing_original.wav");
    let resonant = read_fixture("janice_singing_resonance.wav");
    let clean_bands = band_energy(&original, &VOCAL_BANDS);
    let boosted_bands = band_energy(&resonant, &VOCAL_BANDS);
    assert!(
        ratio_db(boosted_bands[0], clean_bands[0]) > 10.0,
        "fixture must contain the added EQ peak"
    );
    for quality in [Quality::Normal, Quality::LowLatency] {
        for phase in [Phase::Minimum, Phase::Linear] {
            let settings = Settings {
                phase,
                quality,
                ..Settings::default()
            };
            let clean_out = render(&original, settings);
            let boosted_out = render(&resonant, settings);
            let clean_after = band_energy(&clean_out, &VOCAL_BANDS);
            let boosted_after = band_energy(&boosted_out, &VOCAL_BANDS);
            let clean_cut = -ratio_db(clean_after[0], clean_bands[0]);
            let boosted_cut = -ratio_db(boosted_after[0], boosted_bands[0]);
            assert!(
                boosted_cut > 1.0,
                "18 dB EQ bump is escaping detection: {boosted_cut:.3} dB, {phase:?}"
            );
            assert!(
                boosted_cut > clean_cut + 1.0,
                "added resonance should receive more reduction: clean {clean_cut:.3}, boosted {boosted_cut:.3}"
            );
            assert!(
                ratio_db(energy(&clean_out), energy(&original)) > -1.5,
                "gentle processing should preserve the original vocal"
            );
            assert!(
                ratio_db(boosted_after[1], boosted_bands[1]) > -0.75,
                "unrelated bass is being suppressed"
            );
            let stronger = render(
                &resonant,
                Settings {
                    depth: 12.0,
                    ..settings
                },
            );
            let stronger_cut = -ratio_db(band_energy(&stronger, &VOCAL_BANDS)[0], boosted_bands[0]);
            assert!(
                stronger_cut > boosted_cut + 2.0,
                "Depth should give useful extra suppression: {boosted_cut:.3} -> {stronger_cut:.3}"
            );
        }
    }
}

#[test]
fn measurement_renders_are_latency_aligned_and_keep_the_entire_input() {
    let input = musical_mix(1);
    let output = render(
        &input,
        Settings {
            depth: 0.0,
            ..Settings::default()
        },
    );
    assert_eq!(output.samples.len(), input.samples.len());
    for (actual, expected) in output.samples.iter().zip(&input.samples) {
        for c in 0..2 {
            assert!((actual[c] - expected[c]).abs() < 2e-6);
        }
    }
}

#[test]
fn boosting_a_quiet_gap_does_not_force_suppression_of_nearby_notes() {
    let input = peaking_eq(&musical_mix(2), 750.0, 18.0, 8.0);
    let output = render(
        &input,
        Settings {
            depth: 12.0,
            ..Settings::default()
        },
    );
    let bands = [(710.0, 790.0), (180.0, 400.0)];
    let before = band_energy(&input, &bands);
    let after = band_energy(&output, &bands);
    for i in 0..bands.len() {
        assert!(
            ratio_db(after[i], before[i]) > -1.0,
            "boosting noise in a spectral gap should not be treated as a dominant peak"
        );
    }
}

#[test]
fn eq_resonances_on_an_evolving_stereo_chord_are_reduced() {
    let clean = musical_mix(2);
    for phase in [Phase::Minimum, Phase::Linear] {
        let gentle = render(
            &clean,
            Settings {
                phase,
                ..Settings::default()
            },
        );
        assert!(
            ratio_db(energy(&gentle), energy(&clean)) > -1.0,
            "ordinary chord must stay gentle"
        );
        let settings = Settings {
            depth: 12.0,
            phase,
            ..Settings::default()
        };
        let clean_out = render(&clean, settings);
        // Occupied harmonics of the three independently moving voices.
        for frequency in [834.0, 1762.4, 5281.6] {
            let bands = [
                (frequency as f32 * 0.94, frequency as f32 * 1.06),
                (180.0, 400.0),
            ];
            let clean_cut = -ratio_db(
                band_energy(&clean_out, &bands)[0],
                band_energy(&clean, &bands)[0],
            );
            for q in [8.0, 16.0] {
                let boosted = peaking_eq(&clean, frequency, 18.0, q);
                let output = render(&boosted, settings);
                let before = band_energy(&boosted, &bands);
                assert!(
                    ratio_db(before[0], band_energy(&clean, &bands)[0]) > 8.0,
                    "test EQ must produce a material boost on the chord"
                );
                let after = band_energy(&output, &bands);
                let cut = -ratio_db(after[0], before[0]);
                assert!(
                    cut > clean_cut + 1.0,
                    "EQ bump not detected over chord: {frequency} Hz, Q {q}, {phase:?}, clean cut {clean_cut:.3}, boosted cut {cut:.3}"
                );
                assert!(
                    ratio_db(after[1], before[1]) > -1.0,
                    "remote bass changed at {frequency} Hz, Q {q}"
                );
                assert!(output.samples.iter().flatten().all(|x| x.is_finite()));
            }
        }
    }
}

#[test]
fn two_separated_resonances_can_be_reduced_together() {
    let clean = musical_mix(2);
    let input = peaking_eq(&peaking_eq(&clean, 834.0, 18.0, 12.0), 5281.6, 18.0, 12.0);
    let bands = [(790.0, 880.0), (5050.0, 5500.0)];
    let before = band_energy(&input, &bands);
    let output = render(
        &input,
        Settings {
            depth: 12.0,
            ..Settings::default()
        },
    );
    let after = band_energy(&output, &bands);
    for i in 0..bands.len() {
        assert!(
            ratio_db(after[i], before[i]) < -1.5,
            "simultaneous peak escaped: {:?}, {:.3} dB",
            bands[i],
            ratio_db(after[i], before[i])
        );
    }
}

#[test]
fn suppression_recovers_after_an_intermittent_resonance() {
    let clean = musical_mix(4);
    let mut input = peaking_eq(&clean, 1762.4, 24.0, 12.0);
    for (i, (frame, dry)) in input.samples.iter_mut().zip(&clean.samples).enumerate() {
        let t = i as f32 / clean.sample_rate as f32;
        let mix = ((t - 0.75) / 0.02).clamp(0.0, 1.0) * ((2.0 - t) / 0.02).clamp(0.0, 1.0);
        for c in 0..2 {
            frame[c] = dry[c] + mix * (frame[c] - dry[c]);
        }
    }
    let settings = Settings {
        depth: 12.0,
        ..Settings::default()
    };
    let output = render(&input, settings);
    let clean_out = render(&clean, settings);
    let excerpt = |a: &Audio, from: usize, to: usize| Audio {
        sample_rate: a.sample_rate,
        samples: a.samples[from..to].to_vec(),
    };
    let band = [(1700.0, 2000.0)];
    let on = |a: &Audio| band_energy(&excerpt(a, 48000, 90000), &band)[0];
    let recovered = |a: &Audio| band_energy(&excerpt(a, 144000, 192000), &band)[0];
    assert!(
        ratio_db(on(&output), on(&input)) < -2.0,
        "intermittent resonance should be reduced"
    );
    assert!(
        ratio_db(recovered(&output), recovered(&clean_out)).abs() < 0.1,
        "extra suppression persists after the resonance disappears"
    );
}

#[test]
fn pure_notes_are_preserved_but_a_boosted_fundamental_is_not_exempt() {
    let sample_rate = 48000;
    for frequency in [97.0, 440.0, 1056.0, 8000.0] {
        let mut input = Audio {
            sample_rate,
            samples: (0..sample_rate)
                .map(|i| {
                    [0.1 * (std::f64::consts::TAU * frequency * i as f64 / sample_rate as f64).sin()
                        as f32; 2]
                })
                .collect(),
        };
        let mut output = render(&input, Settings::default());
        // Measure a sustained note away from both startup and end-of-file
        // transients: truncating a sine creates a different, broadband signal.
        input.samples.truncate(36000);
        output.samples.truncate(36000);
        input.samples.drain(..12000);
        output.samples.drain(..12000);
        assert!(
            ratio_db(energy(&output), energy(&input)).abs() < 0.1,
            "isolated musical sine should not be detected through its window leakage: {frequency} Hz, {:.3} dB",
            ratio_db(energy(&output), energy(&input))
        );
    }
    let clean = Audio {
        sample_rate,
        samples: (0..sample_rate)
            .map(|i| {
                let t = i as f64 / sample_rate as f64;
                [(1..=24)
                    .map(|h| 0.02 / h as f64 * (std::f64::consts::TAU * 200.0 * h as f64 * t).sin())
                    .sum::<f64>() as f32; 2]
            })
            .collect(),
    };
    let input = peaking_eq(&clean, 200.0, 18.0, 12.0);
    let output = render(&input, Settings::default());
    let band = [(175.0, 225.0)];
    let cut = ratio_db(
        band_energy(&output, &band)[0],
        band_energy(&input, &band)[0],
    );
    assert!(
        cut < -1.0,
        "boosted fundamental escaped detection: {cut:.3} dB"
    );
    // Two independent peaks already provide usable context; requiring three
    // would let a dominant resonance escape on sparse source material.
    let input = Audio {
        sample_rate,
        samples: (0..sample_rate)
            .map(|i| {
                let t = std::f64::consts::TAU * i as f64 / sample_rate as f64;
                [(0.1 * (1000.0 * t).sin() + 0.01 * (1400.0 * t).sin()) as f32; 2]
            })
            .collect(),
    };
    let output = render(&input, Settings::default());
    let band = [(950.0, 1050.0)];
    assert!(
        ratio_db(
            band_energy(&output, &band)[0],
            band_energy(&input, &band)[0]
        ) < -1.0
    );
}

#[test]
fn increasing_peak_strength_does_not_hide_it_from_the_detector() {
    // A strong 3.2 kHz harmonic on a dense musical spectrum, not a lone sine.
    let sample_rate = 48000;
    let band = [(3100.0, 3300.0)];
    for phase in [Phase::Minimum, Phase::Linear] {
        let mut previous = 0.0;
        for boost in [12.0_f32, 18.0, 24.0, 30.0] {
            let samples = (0..sample_rate)
                .map(|i| {
                    let t = i as f32 / sample_rate as f32;
                    let x = (1..=64)
                        .map(|h| {
                            let h = h as f32;
                            let gain = if h == 16.0 {
                                10.0_f32.powf(boost / 20.0)
                            } else {
                                1.0
                            };
                            0.045 * gain / h.powf(0.6)
                                * (std::f32::consts::TAU * 200.0 * h * t + h * h * 0.173).sin()
                        })
                        .sum();
                    [x; 2]
                })
                .collect();
            let input = Audio {
                sample_rate,
                samples,
            };
            let output = render(
                &input,
                Settings {
                    phase,
                    ..Settings::default()
                },
            );
            let cut = -ratio_db(
                band_energy(&output, &band)[0],
                band_energy(&input, &band)[0],
            );
            assert!(
                cut >= previous + 0.3,
                "stronger peak receives less/unchanged suppression: boost {boost}, cut {cut:.3}, previous {previous:.3}, {phase:?}"
            );
            previous = cut;
        }
    }
}
