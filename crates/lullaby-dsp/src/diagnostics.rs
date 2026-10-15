use crate::Settings;
use rustfft::num_complex::Complex32;

/// Borrowed actual states of one analysis channel, indexed by positive FFT bin.
/// All cuts are positive dB; activity is unitless. `log_filter` is
/// the complex natural logarithm of the designed filter, before exponentiation.
/// Its exponential is not the complete, time-varying WOLA audio transfer.
pub struct DiagnosticChannel<'a> {
    pub magnitude_db: &'a [f32],
    pub reference_db: &'a [f32],
    /// Actual prefix sum used by the reference estimator: bins+1 entries,
    /// normalized to this channel's `peak_db`. Entry 0 is zero.
    pub normalized_power_prefix: &'a [f64],
    pub prominence_db: &'a [f32],
    pub activity: &'a [f32],
    /// After activity/detail/startup gating, before stereo linking.
    pub requested_db: &'a [f32],
    pub linked_db: &'a [f32],
    /// After kernel spreading and the Detail-blended Soft Depth map, before weighting.
    pub spread_db: &'a [f32],
    /// After weighting and Max cut, before attack/release smoothing.
    pub target_db: &'a [f32],
    pub smoothed_db: &'a [f32],
    pub log_filter: &'a [Complex32],
    pub peak_db: f32,
    pub activity_floor_db: f32,
}

/// Latest completed analysis frame. Read offline between sample calls.
///
/// The borrow prevents processing while views are in use. No allocation,
/// spectral-buffer copy, I/O, or ready-flag consumption occurs on access.
/// Settings and scalar metadata are copied. Before the first hop and
/// after reset, `Engine::diagnostic_frame` returns None. Subsequent reads return
/// the same frame until another hop; settings changed between hops do not
/// relabel the previous frame. Frame indices restart at 1 after reset.
///
/// `input_end_sample` is the exclusive input sample count since reset. The
/// analysis window spans [end - fft_size, end); negative startup indices are
/// zero padding. `valid_input_samples` counts real samples in that window.
/// These are input-analysis times, not latency-aligned output times.
///
/// Channels are L/R, or M=(L+R)/2 and S=(L-R)/2 in stereo M/S mode. Mono engines
/// analyze the left detector for both channels; weights still act per channel.
/// Bin slices include DC and Nyquist (fft_size/2+1 entries); the power prefix
/// contains one additional entry. Bin frequency is
/// k*sample_rate/fft_size. All fields are measured actual engine states, not
/// reconstructed from the output or inferred proprietary quantities.
pub struct DiagnosticFrame<'a> {
    pub frame_index: u64,
    pub input_end_sample: u64,
    pub valid_input_samples: usize,
    pub fft_size: usize,
    pub hop_size: usize,
    pub sample_rate: f32,
    pub mono: bool,
    pub settings: Settings,
    pub channels: [DiagnosticChannel<'a>; 2],
}

pub(crate) struct TraceState {
    pub prominence: Vec<f32>,
    pub activity: Vec<f32>,
    pub requested: Vec<f32>,
    pub spread: Vec<f32>,
    pub target: Vec<f32>,
    pub peak: f32,
    pub floor: f32,
}

impl TraceState {
    pub fn new(bins: usize) -> Self {
        Self {
            prominence: vec![0.0; bins],
            activity: vec![0.0; bins],
            requested: vec![0.0; bins],
            spread: vec![0.0; bins],
            target: vec![0.0; bins],
            peak: -150.0,
            floor: -140.0,
        }
    }
}
