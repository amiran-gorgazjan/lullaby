//! Host-independent, allocation-free (after construction) spectral resonance suppression.
mod detector;
mod engine;
mod settings;

pub use engine::{Engine, METER_BINS, MeterFrame};
pub use settings::*;
