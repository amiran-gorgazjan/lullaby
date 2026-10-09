//! Reproducible, synthetic calibration measurements (no proprietary audio).
#[path = "../tests/support/mod.rs"]
mod support;

use lullaby_dsp::{Phase, Quality, factory_preset};
use support::{harmonic_fixture, measure, noise_fixture};

fn main() {
    println!(
        "fixture,preset,phase,quality,rms_db,400_hz_db,800_hz_db,1600_hz_db,3200_hz_db,6400_hz_db,peak_cut_db"
    );
    for (fixture, signal) in [
        ("balanced", harmonic_fixture(48000.0, false)),
        ("resonant", harmonic_fixture(48000.0, true)),
        ("noise", noise_fixture(48000)),
    ] {
        for (preset, index) in [("gentle", 0), ("vocal", 1)] {
            for phase in [Phase::Minimum, Phase::Linear] {
                for quality in [Quality::Normal, Quality::LowLatency] {
                    let mut settings = factory_preset(index);
                    settings.phase = phase;
                    settings.quality = quality;
                    let m = measure(&signal, 48000.0, settings);
                    println!(
                        "{fixture},{preset},{phase:?},{quality:?},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3}",
                        m.level_db,
                        m.tones_db[0],
                        m.tones_db[1],
                        m.tones_db[2],
                        m.tones_db[3],
                        m.tones_db[4],
                        m.max_reduction_db
                    );
                }
            }
        }
    }
}
