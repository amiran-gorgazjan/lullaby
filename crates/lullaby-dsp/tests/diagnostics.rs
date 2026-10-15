#![cfg(feature = "dsp-diagnostics")]
use lullaby_dsp::{Engine, Phase, Quality, Settings, StereoMode};

fn advance(engine: &mut Engine, samples: usize, same: bool) {
    for i in 0..samples {
        let x = (std::f32::consts::TAU * i as f32 / 32.0).sin() * 0.2;
        engine.process_sample([x, if same { x } else { 0.0 }], [x, 0.0]);
    }
}

#[test]
fn startup_reset_and_caller_control_have_unambiguous_frame_times() {
    let mut e = Engine::new(48000.0, Quality::Normal, false);
    e.set_settings(Settings::default());
    assert!(e.diagnostic_frame().is_none());
    advance(&mut e, 511, false);
    assert!(e.diagnostic_frame().is_none());
    advance(&mut e, 1, false);
    let f = e.diagnostic_frame().unwrap();
    assert_eq!(
        (f.frame_index, f.input_end_sample, f.valid_input_samples),
        (1, 512, 512)
    );
    assert!(
        f.channels
            .iter()
            .all(|c| c.requested_db.iter().all(|&v| v == 0.0))
    );
    assert!(e.take_frame_ready());
    assert!(!e.take_frame_ready());
    e.set_settings(Settings {
        depth: 9.0,
        ..Default::default()
    });
    advance(&mut e, 77, false);
    assert_eq!(e.diagnostic_frame().unwrap().settings.depth, 3.0);
    assert_eq!(e.diagnostic_frame().unwrap().input_end_sample, 512);
    e.reset();
    assert!(e.diagnostic_frame().is_none());
    assert!(!e.take_frame_ready());
    e.set_settings(Settings::default());
    advance(&mut e, 512, false);
    assert_eq!(e.diagnostic_frame().unwrap().frame_index, 1);
}

#[test]
fn views_cover_all_actual_stages_with_finite_values_and_positive_cut_units() {
    for quality in [Quality::LowLatency, Quality::Normal, Quality::Ultra] {
        for phase in [Phase::Minimum, Phase::Linear] {
            let mut e = Engine::new(96000.0, quality, false);
            e.set_settings(Settings {
                quality,
                phase,
                depth: 10.0,
                max_cut_db: 3.0,
                ..Default::default()
            });
            advance(&mut e, 8192, false);
            let f = e.diagnostic_frame().unwrap();
            assert_eq!(f.fft_size, e.latency_samples());
            assert_eq!(f.hop_size, f.fft_size / quality.overlap());
            assert_eq!(f.valid_input_samples, f.fft_size);
            assert_eq!(f.input_end_sample, f.frame_index * f.hop_size as u64);
            for c in f.channels {
                let bins = f.fft_size / 2 + 1;
                assert_eq!(c.normalized_power_prefix.len(), bins + 1);
                assert_eq!(c.normalized_power_prefix[0], 0.0);
                assert!(c.normalized_power_prefix.iter().all(|v| v.is_finite()));
                for v in [
                    c.magnitude_db,
                    c.reference_db,
                    c.prominence_db,
                    c.activity,
                    c.requested_db,
                    c.linked_db,
                    c.spread_db,
                    c.target_db,
                    c.smoothed_db,
                ] {
                    assert_eq!(v.len(), bins);
                    assert!(v.iter().all(|v| v.is_finite()));
                }
                assert!(c.peak_db.is_finite() && c.activity_floor_db.is_finite());
                assert!(c.activity.iter().all(|v| (0.0..=1.0).contains(v)));
                assert!(
                    c.target_db
                        .iter()
                        .chain(c.smoothed_db)
                        .all(|v| (0.0..=3.0).contains(v))
                );
                assert_eq!(c.log_filter.len(), bins);
                assert!(
                    c.log_filter
                        .iter()
                        .all(|v| v.re.is_finite() && v.im.is_finite())
                );
                if phase == Phase::Linear {
                    assert!(c.log_filter.iter().all(|v| v.im == 0.0));
                }
            }
        }
    }
}

#[test]
fn link_and_stereo_attribution_expose_independent_detector_requests() {
    let settings = Settings {
        depth: 10.0,
        external_sidechain: true,
        ..Default::default()
    };
    let mut e = Engine::new(48000.0, Quality::Normal, false);
    e.set_settings(settings);
    advance(&mut e, 8192, false);
    let f = e.diagnostic_frame().unwrap();
    assert!(f.channels[0].requested_db.iter().any(|&v| v > 0.1));
    assert!(f.channels[1].requested_db.iter().all(|&v| v == 0.0));
    assert_eq!(f.channels[0].linked_db, f.channels[1].linked_db);
    e.set_settings(Settings {
        link: 0.0,
        ..settings
    });
    advance(&mut e, 2048, false);
    let f = e.diagnostic_frame().unwrap();
    assert_eq!(f.channels[1].requested_db, f.channels[1].linked_db);
    assert!(f.channels[1].linked_db.iter().all(|&v| v == 0.0));

    for mono in [false, true] {
        let mut e = Engine::new(48000.0, Quality::Normal, mono);
        e.set_settings(Settings {
            stereo_mode: StereoMode::MidSide,
            ..Default::default()
        });
        advance(&mut e, 8192, true);
        let f = e.diagnostic_frame().unwrap();
        assert_eq!(f.mono, mono);
        if mono {
            assert_eq!(f.channels[0].magnitude_db, f.channels[1].magnitude_db);
        } else {
            assert!(f.channels[1].magnitude_db.iter().all(|&v| v <= -150.0));
        }
    }
}
