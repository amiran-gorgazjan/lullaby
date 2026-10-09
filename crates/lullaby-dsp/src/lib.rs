//! Host-independent, allocation-free (after construction) spectral resonance suppression.
mod engine;
mod settings;

pub use engine::{Engine, METER_BINS, MeterFrame};
pub use settings::*;
