//! Analyze the supplied singing pair; optionally export renders to a directory.
#[path = "../tests/support/audio.rs"]
mod audio;
use audio::{VOCAL_BANDS, band_energy, energy, ratio_db, read_fixture, render, write_render};
use lullaby_dsp::{Phase, Settings};

fn main() {
    let output_dir = std::env::args_os().nth(1).map(std::path::PathBuf::from);
    if let Some(dir) = &output_dir {
        std::fs::create_dir_all(dir).unwrap();
    }
    println!("fixture,depth,phase,target_db,bass_db,treble_db,rms_db");
    for name in ["original", "resonance"] {
        let input = read_fixture(&format!("janice_singing_{name}.wav"));
        let input_bands = band_energy(&input, &VOCAL_BANDS);
        for phase in [Phase::Minimum, Phase::Linear] {
            for depth in [3.0, 6.0, 8.0, 10.0] {
                let result = render(
                    &input,
                    Settings {
                        depth,
                        phase,
                        ..Settings::default()
                    },
                );
                let output_bands = band_energy(&result, &VOCAL_BANDS);
                let changes: Vec<_> = output_bands
                    .iter()
                    .zip(&input_bands)
                    .map(|(o, i)| ratio_db(*o, *i))
                    .collect();
                println!(
                    "{name},{depth},{phase:?},{:.3},{:.3},{:.3},{:.3}",
                    changes[0],
                    changes[1],
                    changes[2],
                    ratio_db(energy(&result), energy(&input))
                );
                if let Some(dir) = &output_dir
                    && name == "resonance"
                    && phase == Phase::Minimum
                    && depth <= 10.0
                {
                    write_render(
                        &result,
                        &dir.join(format!("janice_{name}_depth{depth}_{phase:?}.wav")),
                    );
                }
            }
        }
    }
}
