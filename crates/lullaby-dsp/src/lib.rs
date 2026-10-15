//! Host-independent, allocation-free (after construction) spectral resonance suppression.
mod detector;
#[cfg(feature = "dsp-diagnostics")]
mod diagnostics;
mod engine;
mod settings;

#[cfg(feature = "dsp-diagnostics")]
pub use diagnostics::{DiagnosticChannel, DiagnosticFrame};
pub use engine::{Engine, METER_BINS, MeterFrame};
pub use settings::*;
