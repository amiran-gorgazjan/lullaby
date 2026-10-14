/// Estimates a local spectral-power envelope. The candidate participates in
/// the neighborhood average, so a lone sinusoid has finite contrast and a
/// dense, flat spectrum has little contrast. Construction allocates the prefix
/// buffer; analysis performs bounded linear work without allocation.
pub(crate) struct SpectralReference {
    power_sum: Vec<f64>,
    reference: Vec<f32>,
}

impl SpectralReference {
    pub fn new(bins: usize) -> Self {
        Self {
            power_sum: vec![0.0; bins + 1],
            reference: vec![0.0; bins],
        }
    }

    pub fn analyze(&mut self, db: &[f32], lower: &[usize], upper: &[usize]) -> &[f32] {
        // Normalize before summing: subtraction of two large prefix sums must
        // still resolve quiet neighborhoods, and a uniform input gain should
        // not alter this relative detector.
        let peak = db.iter().copied().fold(-150.0_f32, f32::max);
        self.power_sum[0] = 0.0;
        for (k, &level) in db.iter().enumerate() {
            self.power_sum[k + 1] = self.power_sum[k] + 10.0_f64.powf((level - peak) as f64 * 0.1);
        }
        for k in 0..db.len() {
            let lo = lower[k].min(k);
            let hi = upper[k].max(k + 1).min(db.len());
            let mean = (self.power_sum[hi] - self.power_sum[lo]) / (hi - lo) as f64;
            // Global contrast calibration, independently chosen against the
            // reviewed corpus; not a recovered proprietary envelope offset.
            self.reference[k] = peak + (10.0 * mean.max(1e-15).log10()) as f32 - 2.0;
        }
        &self.reference
    }
}

#[cfg(test)]
mod tests {
    use super::SpectralReference;

    #[test]
    fn a_flat_spectrum_has_little_contrast_but_an_isolated_peak_has_bounded_contrast() {
        let mut detector = SpectralReference::new(129);
        let lower: Vec<_> = (0usize..129).map(|k| k.saturating_sub(16)).collect();
        let upper: Vec<_> = (0usize..129).map(|k| (k + 17).min(129)).collect();
        let mut db = vec![-30.0; 129];
        let reference = detector.analyze(&db, &lower, &upper);
        assert!(
            db.iter()
                .zip(reference)
                .all(|(level, base)| (level - base).abs() < 3.0)
        );

        db.fill(-140.0);
        db[64] = -20.0;
        let first = db[64] - detector.analyze(&db, &lower, &upper)[64];
        assert!(
            (5.0..30.0).contains(&first),
            "isolated-peak contrast: {first}"
        );
        // A stronger isolated tone should approach the same finite contrast;
        // numerical valleys must not make the result unbounded.
        db[64] = 0.0;
        let stronger = db[64] - detector.analyze(&db, &lower, &upper)[64];
        assert!((stronger - first).abs() < 0.01);
    }

    #[test]
    fn relative_envelope_is_invariant_to_a_uniform_gain_change() {
        let mut detector = SpectralReference::new(65);
        let lower: Vec<_> = (0usize..65).map(|k| k.saturating_sub(8)).collect();
        let upper: Vec<_> = (0usize..65).map(|k| (k + 9).min(65)).collect();
        let db: Vec<_> = (0..65)
            .map(|k| -20.0 - (k as f32 * 1.37).sin().abs() * 45.0)
            .collect();
        let first = detector.analyze(&db, &lower, &upper).to_vec();
        let quieter: Vec<_> = db.iter().map(|level| level - 40.0).collect();
        let second = detector.analyze(&quieter, &lower, &upper);
        for ((level, quiet), (base, quiet_base)) in
            db.iter().zip(&quieter).zip(first.iter().zip(second))
        {
            assert!(((level - base) - (quiet - quiet_base)).abs() < 2e-5);
        }
    }
}
