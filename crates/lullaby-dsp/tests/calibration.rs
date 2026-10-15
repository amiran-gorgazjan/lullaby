mod support;

use lullaby_dsp::{MAX_CUT_OFF, Phase, Quality, factory_preset};
use support::{harmonic_fixture, harmonic_fixture_at, measure, noise_fixture};

#[test]
fn low_depth_remains_gentle_on_an_ordinary_harmonic_spectrum() {
    for sample_rate in [44100.0, 48000.0, 96000.0] {
        for fundamental in [100.0, 200.0, 400.0] {
            let signal = harmonic_fixture_at(sample_rate, fundamental, false);
            for phase in [Phase::Minimum, Phase::Linear] {
                for quality in [Quality::Normal, Quality::LowLatency] {
                    let mut settings = factory_preset(0);
                    // Low Depth should preserve ordinary musical harmonics.
                    // This guard does not exempt those harmonics at high Depth.
                    settings.depth = 3.0;
                    settings.phase = phase;
                    settings.quality = quality;
                    let m = measure(&signal, sample_rate, settings);
                    assert!(
                        m.level_db > -1.0,
                        "low depth muffles ordinary harmonics: {} dB, {sample_rate} Hz, {phase:?}, {quality:?}",
                        m.level_db
                    );
                    assert!(
                        (m.tones_db[4] - m.tones_db[0]).abs() < 0.75,
                        "low depth changes bass/treble balance: {:?}, {sample_rate} Hz, {phase:?}, {quality:?}",
                        m.tones_db
                    );
                    assert!(
                        m.max_reduction_db < 3.0,
                        "low depth is reaching deep cuts: {}",
                        m.max_reduction_db
                    );
                }
            }
        }
    }
}

#[test]
fn vocal_silk_adds_local_reduction_without_changing_remote_harmonics() {
    let signal = harmonic_fixture(48000.0, true);
    for phase in [Phase::Minimum, Phase::Linear] {
        let mut gentle = factory_preset(0);
        let mut vocal = factory_preset(1);
        gentle.phase = phase;
        vocal.phase = phase;
        let g = measure(&signal, 48000.0, gentle);
        let v = measure(&signal, 48000.0, vocal);
        assert!(
            g.tones_db[3] < -0.3,
            "added resonance was not detected: {:?}",
            g.tones_db
        );
        assert!(
            v.tones_db[3] < g.tones_db[3] * 1.8,
            "bell has no useful effect: gentle {:?}, vocal {:?}, {phase:?}",
            g.tones_db,
            v.tones_db
        );
        for i in 0..2 {
            assert!(
                (v.tones_db[i] - g.tones_db[i]).abs() < 0.2,
                "preset changes bass outside the bell: gentle {:?}, vocal {:?}",
                g.tones_db,
                v.tones_db
            );
        }
    }
}

#[test]
fn gentle_start_does_not_turn_broadband_noise_into_a_lowpass() {
    let signal = noise_fixture(48000);
    for quality in [
        Quality::Normal,
        Quality::High,
        Quality::Ultra,
        Quality::LowLatency,
    ] {
        for phase in [Phase::Minimum, Phase::Linear] {
            let mut s = factory_preset(0);
            s.phase = phase;
            s.quality = quality;
            let m = measure(&signal, 48000.0, s);
            assert!(
                m.level_db > -0.75,
                "broadband loss: {} dB in {phase:?}/{quality:?}",
                m.level_db
            );
        }
    }
}

#[test]
fn depth_increases_suppression_progressively_instead_of_saturating_at_default() {
    let signal = harmonic_fixture(48000.0, true);
    let mut reductions = Vec::new();
    for depth in [0.0, 1.0, 3.0, 6.0, 8.0, 10.0] {
        let mut s = factory_preset(0);
        s.depth = depth;
        // Measure the depth law independently of the factory 12 dB ceiling.
        s.max_cut_db = MAX_CUT_OFF;
        s.phase = Phase::Linear;
        reductions.push(-measure(&signal, 48000.0, s).tones_db[3]);
    }
    assert!(reductions[0].abs() < 0.01);
    assert!(
        reductions[1] < 0.5,
        "low depth is too strong: {reductions:?}"
    );
    assert!(
        reductions[2] < 3.0,
        "low depth has no gentle range: {reductions:?}"
    );
    assert!(
        reductions[4] > reductions[3] + 1.0,
        "no useful range above default: {reductions:?}"
    );
    assert!(
        reductions[5] > reductions[4] + 1.0,
        "high depth has no remaining range: {reductions:?}"
    );
    assert!(
        reductions.windows(2).all(|pair| pair[1] >= pair[0] - 0.01),
        "non-monotonic depth: {reductions:?}"
    );
}
