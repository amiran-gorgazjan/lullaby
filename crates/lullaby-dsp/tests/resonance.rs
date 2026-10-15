#[path = "support/audio.rs"]
mod audio;
#[path = "support/signals.rs"]
mod signals;
use audio::{Audio, band_energy, energy, ratio_db, render};
use lullaby_dsp::{MAX_CUT_OFF, Phase, Quality, Settings};
use signals::{musical_mix, peaking_eq};

#[test]
fn generated_eq_peak_receives_selective_reduction_across_quality_and_phase() {
    let original = musical_mix(2);
    let resonant = peaking_eq(&original, 834.0, 18.0, 12.0);
    let bands = [(790.0, 880.0), (180.0, 400.0)];
    let clean_bands = band_energy(&original, &bands);
    let boosted_bands = band_energy(&resonant, &bands);
    assert!(
        ratio_db(boosted_bands[0], clean_bands[0]) > 10.0,
        "generated input must contain the added EQ peak"
    );
    for quality in [Quality::Normal, Quality::LowLatency] {
        for phase in [Phase::Minimum, Phase::Linear] {
            let settings = Settings {
                depth: 6.0,
                phase,
                quality,
                ..Settings::default()
            };
            let clean_out = render(&original, settings);
            let boosted_out = render(&resonant, settings);
            let clean_after = band_energy(&clean_out, &bands);
            let boosted_after = band_energy(&boosted_out, &bands);
            let clean_cut = -ratio_db(clean_after[0], clean_bands[0]);
            let boosted_cut = -ratio_db(boosted_after[0], boosted_bands[0]);
            assert!(
                boosted_cut > 1.0,
                "18 dB EQ bump is escaping detection: {boosted_cut:.3} dB, {phase:?}"
            );
            assert!(
                boosted_cut > clean_cut + 0.1,
                "added resonance should receive more reduction: clean {clean_cut:.3}, boosted {boosted_cut:.3}"
            );
            assert!(
                ratio_db(
                    energy(&render(
                        &original,
                        Settings {
                            depth: 3.0,
                            ..settings
                        }
                    )),
                    energy(&original)
                ) > -1.5,
                "low-depth processing should preserve the original chord"
            );
            assert!(
                (ratio_db(boosted_after[1], boosted_bands[1])
                    - ratio_db(clean_after[1], clean_bands[1]))
                .abs()
                    < 0.75,
                "added EQ peak changes suppression of unrelated bass"
            );
            let stronger = render(
                &resonant,
                Settings {
                    depth: 10.0,
                    ..settings
                },
            );
            let stronger_cut = -ratio_db(band_energy(&stronger, &bands)[0], boosted_bands[0]);
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
fn boosting_a_quiet_gap_preserves_remote_notes_and_bounds_local_spread() {
    let clean = musical_mix(2);
    let input = peaking_eq(&clean, 750.0, 18.0, 8.0);
    let settings = Settings {
        depth: 10.0,
        ..Settings::default()
    };
    let output = render(&input, settings);
    let clean_output = render(&clean, settings);
    let bands = [(710.0, 790.0), (180.0, 400.0)];
    let before = band_energy(&input, &bands);
    let after = band_energy(&output, &bands);
    let clean_before = band_energy(&clean, &bands);
    let clean_after = band_energy(&clean_output, &bands);
    for i in 0..bands.len() {
        let added_cut = ratio_db(after[i], before[i]) - ratio_db(clean_after[i], clean_before[i]);
        // Bound changes outside the EQ peak while allowing its smooth skirts
        // to affect the nearby measurement band more than remote bass.
        let limit = if i == 0 { 2.0 } else { 1.0 };
        assert!(
            added_cut.abs() < limit,
            "boosting a quiet gap changes band {i} suppression by {added_cut:.3} dB"
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
                depth: 3.0,
                phase,
                ..Settings::default()
            },
        );
        assert!(
            ratio_db(energy(&gentle), energy(&clean)) > -1.0,
            "ordinary chord must stay gentle"
        );
        let settings = Settings {
            depth: 10.0,
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
                    (ratio_db(after[1], before[1])
                        - ratio_db(
                            band_energy(&clean_out, &bands)[1],
                            band_energy(&clean, &bands)[1]
                        ))
                    .abs()
                        < 1.0,
                    "EQ bump changes suppression of remote bass at {frequency} Hz, Q {q}"
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
            depth: 10.0,
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
        depth: 10.0,
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
fn pure_notes_and_a_boosted_fundamental_are_not_exempt() {
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
        // High-depth suppression remains active on isolated tonal signals.
        // Exercise several frequencies independently of the detector's meters.
        let mut output = render(
            &input,
            Settings {
                depth: 10.0,
                max_cut_db: MAX_CUT_OFF,
                ..Settings::default()
            },
        );
        // Measure a sustained note away from both startup and end-of-file
        // transients: truncating a sine creates a different, broadband signal.
        input.samples.truncate(36000);
        output.samples.truncate(36000);
        input.samples.drain(..12000);
        output.samples.drain(..12000);
        assert!(
            ratio_db(energy(&output), energy(&input)) < -1.0,
            "isolated sine should receive nonzero high-depth suppression: {frequency} Hz, {:.3} dB",
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
    let output = render(
        &input,
        Settings {
            depth: 6.0,
            ..Settings::default()
        },
    );
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
    let output = render(
        &input,
        Settings {
            depth: 6.0,
            ..Settings::default()
        },
    );
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
                    depth: 6.0,
                    phase,
                    ..Settings::default()
                },
            );
            let cut = -ratio_db(
                band_energy(&output, &band)[0],
                band_energy(&input, &band)[0],
            );
            // A relative detector should approach a finite lone-tone response
            // as one peak dominates its neighborhood. Do not require a fixed
            // extra cut per boost once the spectral contrast has saturated.
            assert!(cut > 2.0, "dominant peak escapes suppression: {cut:.3} dB");
            assert!(
                cut >= previous - 0.05,
                "stronger peak receives less suppression: boost {boost}, cut {cut:.3}, previous {previous:.3}, {phase:?}"
            );
            previous = cut;
        }
    }
}

#[test]
fn detail_controls_cut_concentration_on_close_tones() {
    let sample_rate = 48000;
    let input = Audio {
        sample_rate,
        samples: (0..sample_rate * 2)
            .map(|i| {
                let t = i as f32 / sample_rate as f32;
                [(std::f32::consts::TAU * 1056.0 * t).sin() * 0.2
                    + (std::f32::consts::TAU * 1000.0 * t).sin() * 0.03
                    + (std::f32::consts::TAU * 1112.0 * t).sin() * 0.03; 2]
            })
            .collect(),
    };
    let bands = [(990.0, 1010.0), (1046.0, 1066.0), (1102.0, 1122.0)];
    let before = band_energy(&input, &bands);
    let cuts = |detail| {
        let output = render(
            &input,
            Settings {
                depth: 10.0,
                detail,
                max_cut_db: MAX_CUT_OFF,
                phase: Phase::Linear,
                ..Settings::default()
            },
        );
        let after = band_energy(&output, &bands);
        std::array::from_fn::<_, 3, _>(|i| -ratio_db(after[i], before[i]))
    };
    let broad = cuts(1.0);
    let narrow = cuts(10.0);
    assert!(broad[1] > 2.0, "broad cuts remain active: {broad:?}");
    assert!(
        (broad[1] - broad[0]).abs() < 2.0 && (broad[1] - broad[2]).abs() < 2.0,
        "low Detail should cover neighboring tones: {broad:?}"
    );
    assert!(
        narrow[1] > broad[1] + 6.0,
        "high Detail concentrates stronger cuts: {broad:?} -> {narrow:?}"
    );
    assert!(
        narrow[1] > narrow[0] + 3.0 && narrow[1] > narrow[2] + 3.0,
        "high Detail should distinguish neighboring tones: {narrow:?}"
    );
}
