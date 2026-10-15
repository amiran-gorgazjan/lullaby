//! Host-independent, allocation-free (after construction) spectral resonance suppression.
mod detector;
#[cfg(feature = "research-trace")]
mod diagnostics;
mod engine;
mod settings;

#[cfg(feature = "research-trace")]
pub use diagnostics::{DiagnosticChannel, DiagnosticFrame};
pub use engine::{Engine, METER_BINS, MeterFrame};
pub use settings::*;
