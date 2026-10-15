//! Bounded offline PCM renderer and actual-stage CSV exporter. See docs/diagnostics.md.
use lullaby_dsp::{Engine, Settings};
use serde::Deserialize;
use std::{
    error::Error,
    fs::File,
    io::{BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Event {
    frame: u64,
    settings: Settings,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Job {
    sample_rate: u32,
    /// Interleaved stereo f32 little endian, also for mono engines.
    input: PathBuf,
    sidechain_input: Option<PathBuf>,
    output: PathBuf,
    trace_csv: PathBuf,
    frames: u64,
    settings: Settings,
    #[serde(default)]
    mono: bool,
    #[serde(default)]
    automation: Vec<Event>,
    frequencies_hz: Vec<f32>,
    max_records: usize,
    every_n_frames: u64,
    #[serde(default)]
    start_sample: u64,
    end_sample: Option<u64>,
}

fn stereo_sample(reader: &mut impl Read) -> std::io::Result<[f32; 2]> {
    let mut bytes = [0u8; 8];
    reader.read_exact(&mut bytes)?;
    Ok([
        f32::from_le_bytes(bytes[..4].try_into().unwrap()),
        f32::from_le_bytes(bytes[4..].try_into().unwrap()),
    ])
}

fn input(path: &Path, frames: u64) -> Result<BufReader<File>, Box<dyn Error>> {
    let file = File::open(path)?;
    if file.metadata()?.len() != frames * 8 {
        return Err(format!(
            "{}: expected exactly {frames} stereo frames",
            path.display()
        )
        .into());
    }
    Ok(BufReader::new(file))
}

fn run(job: Job) -> Result<(), Box<dyn Error>> {
    if !(8000..=384000).contains(&job.sample_rate)
        || job.frames == 0
        || job.frames > u64::from(job.sample_rate) * 600
        || job.frequencies_hz.is_empty()
        || job.frequencies_hz.len() > 128
        || job.max_records > 1_000_000
        || job.every_n_frames == 0
        || job.start_sample > job.frames
        || job
            .end_sample
            .is_some_and(|end| end < job.start_sample || end > job.frames)
        || job
            .frequencies_hz
            .iter()
            .any(|&f| !f.is_finite() || f < 0.0 || f > job.sample_rate as f32 / 2.0)
        || job.automation.len() > 100_000
        || job.automation.windows(2).any(|e| e[0].frame >= e[1].frame)
        || job
            .automation
            .iter()
            .any(|e| e.frame >= job.frames || e.settings.quality != job.settings.quality)
    {
        return Err("invalid bounds, frequencies, or automation (quality must stay fixed)".into());
    }
    let mut main = input(&job.input, job.frames)?;
    let mut key = job
        .sidechain_input
        .as_ref()
        .map(|p| input(p, job.frames))
        .transpose()?;
    let mut output = BufWriter::new(
        File::options()
            .write(true)
            .create_new(true)
            .open(&job.output)?,
    );
    let mut csv = BufWriter::new(
        File::options()
            .write(true)
            .create_new(true)
            .open(&job.trace_csv)?,
    );
    writeln!(
        csv,
        "frame_index,input_end_sample,valid_input_samples,channel,requested_hz,bin,bin_hz,magnitude_db,reference_db,prominence_db,activity,requested_db,linked_db,spread_db,target_db,smoothed_db,log_filter_re,log_filter_im,filter_re_reconstructed,filter_im_reconstructed,peak_db,activity_floor_db,power_prefix_before,power_prefix_after"
    )?;
    let mut engine = Engine::new(job.sample_rate as f32, job.settings.quality, job.mono);
    engine.set_settings(job.settings);
    let mut events = job.automation.iter().peekable();
    let mut records = 0;
    for i in 0..job.frames {
        if events.peek().is_some_and(|event| event.frame == i) {
            engine.set_settings(events.next().unwrap().settings);
        }
        let main_sample = stereo_sample(&mut main)?;
        let key_sample = match &mut key {
            Some(reader) => stereo_sample(reader)?,
            None => [0.0; 2],
        };
        let result = engine.process_sample(main_sample, key_sample);
        for x in result {
            output.write_all(&x.to_le_bytes())?;
        }
        if !engine.take_frame_ready() || records >= job.max_records {
            continue;
        }
        let frame = engine.diagnostic_frame().unwrap();
        if frame.input_end_sample < job.start_sample
            || frame.input_end_sample > job.end_sample.unwrap_or(job.frames)
            || !(frame.frame_index - 1).is_multiple_of(job.every_n_frames)
        {
            continue;
        }
        for (channel, ch) in frame.channels.iter().enumerate() {
            for &requested_hz in &job.frequencies_hz {
                if records >= job.max_records {
                    break;
                }
                let bin =
                    (requested_hz * frame.fft_size as f32 / frame.sample_rate).round() as usize;
                let bin_hz = bin as f32 * frame.sample_rate / frame.fft_size as f32;
                let log_filter = ch.log_filter[bin];
                let filter = log_filter.exp();
                writeln!(
                    csv,
                    "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
                    frame.frame_index,
                    frame.input_end_sample,
                    frame.valid_input_samples,
                    channel,
                    requested_hz,
                    bin,
                    bin_hz,
                    ch.magnitude_db[bin],
                    ch.reference_db[bin],
                    ch.prominence_db[bin],
                    ch.activity[bin],
                    ch.requested_db[bin],
                    ch.linked_db[bin],
                    ch.spread_db[bin],
                    ch.target_db[bin],
                    ch.smoothed_db[bin],
                    log_filter.re,
                    log_filter.im,
                    filter.re,
                    filter.im,
                    ch.peak_db,
                    ch.activity_floor_db,
                    ch.normalized_power_prefix[bin],
                    ch.normalized_power_prefix[bin + 1]
                )?;
                records += 1;
            }
        }
    }
    output.flush()?;
    csv.flush()?;
    println!(
        "{}",
        serde_json::json!({"frames":job.frames,"records":records,"latency_samples":engine.latency_samples(),"sample_rate":job.sample_rate,"mono":job.mono,"initial_settings":job.settings,"automation":job.automation.len(),"output_alignment":"raw engine output; declared latency not removed"})
    );
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let path = std::env::args_os()
        .nth(1)
        .ok_or("usage: diagnostic_trace JOB.json")?;
    // Bound job parsing too, including invalid or hostile manifests.
    let job: Job =
        serde_json::from_reader(BufReader::new(File::open(path)?).take(4 * 1024 * 1024))?;
    run(job)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn existing_files_and_hardlinks_are_never_truncated() {
        let folder = std::env::temp_dir().join(format!(
            "lullaby-trace-files-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&folder).unwrap();
        let source = folder.join("main.pcm");
        let alias = folder.join("alias.pcm");
        let csv = folder.join("existing.csv");
        let bytes = [0u8; 8];
        std::fs::write(&source, bytes).unwrap();
        std::fs::hard_link(&source, &alias).unwrap();
        std::fs::write(&csv, b"preserved").unwrap();
        let job = |output: &Path, trace: &Path| {
            serde_json::from_value(serde_json::json!({
                "sample_rate":8000,"input":source,"output":output,"trace_csv":trace,
                "frames":1,"settings":{},"frequencies_hz":[120.0],
                "max_records":1,"every_n_frames":1
            }))
            .unwrap()
        };
        assert!(run(job(&source, &folder.join("unused.csv"))).is_err());
        assert!(run(job(&alias, &folder.join("unused.csv"))).is_err());
        assert!(run(job(&folder.join("fresh.pcm"), &csv)).is_err());
        assert_eq!(std::fs::read(&source).unwrap(), bytes);
        assert_eq!(std::fs::read(&alias).unwrap(), bytes);
        assert_eq!(std::fs::read(&csv).unwrap(), b"preserved");
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn record_cap_and_frequency_selection_do_not_change_audio_or_automation() {
        let folder = std::env::temp_dir().join(format!(
            "lullaby-trace-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&folder).unwrap();
        let frames = 4096;
        let pcm: Vec<u8> = (0..frames)
            .flat_map(|i| {
                let x = (i as f32 * 0.12).sin() * 0.2;
                [x, -x].into_iter().flat_map(f32::to_le_bytes)
            })
            .collect();
        let main = folder.join("main.pcm");
        std::fs::write(&main, &pcm).unwrap();
        let settings = Settings {
            quality: lullaby_dsp::Quality::LowLatency,
            ..Default::default()
        };
        let changed = Settings {
            depth: 9.0,
            mix: 0.3,
            ..settings
        };
        for (name, cap, frequencies) in [
            ("bounded", 3, vec![0.0, 120.0, 24000.0]),
            ("audio", 0, vec![6000.0]),
        ] {
            run(Job {
                sample_rate: 48000,
                input: main.clone(),
                sidechain_input: None,
                output: folder.join(format!("{name}.pcm")),
                trace_csv: folder.join(format!("{name}.csv")),
                frames,
                settings,
                mono: false,
                automation: vec![Event {
                    frame: 1234,
                    settings: changed,
                }],
                frequencies_hz: frequencies,
                max_records: cap,
                every_n_frames: 1,
                start_sample: 0,
                end_sample: None,
            })
            .unwrap();
        }
        let rendered = std::fs::read(folder.join("bounded.pcm")).unwrap();
        assert_eq!(rendered, std::fs::read(folder.join("audio.pcm")).unwrap());
        let mut engine = Engine::new(48000.0, settings.quality, false);
        engine.set_settings(settings);
        let mut expected = Vec::new();
        for (i, bytes) in pcm.chunks_exact(8).enumerate() {
            if i == 1234 {
                engine.set_settings(changed);
            }
            let x = [
                f32::from_le_bytes(bytes[..4].try_into().unwrap()),
                f32::from_le_bytes(bytes[4..].try_into().unwrap()),
            ];
            expected.extend(
                engine
                    .process_sample(x, [0.0; 2])
                    .into_iter()
                    .flat_map(f32::to_le_bytes),
            );
        }
        assert_eq!(rendered, expected);
        let csv = std::fs::read_to_string(folder.join("bounded.csv")).unwrap();
        let rows: Vec<_> = csv.lines().collect();
        assert_eq!(rows.len(), 4);
        assert!(rows.iter().all(|r| r.split(',').count() == 24));
        let nyquist: Vec<_> = rows[3].split(',').collect();
        assert_eq!(
            (
                nyquist[0], nyquist[1], nyquist[2], nyquist[3], nyquist[5], nyquist[6]
            ),
            ("1", "128", "128", "0", "256", "24000")
        );
        std::fs::remove_dir_all(folder).unwrap();
    }
}
