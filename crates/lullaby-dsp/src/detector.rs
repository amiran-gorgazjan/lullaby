/// Estimates a spectral envelope from independent neighboring peaks. A peak
/// must not contribute to its own reference: that would make the reference
/// follow a large resonance upward and hide it. Valleys are excluded too, so
/// ordinary harmonic spacing is not mistaken for a series of resonances.
pub(crate) struct PeakReference {
    peaks: Vec<usize>,
    peak_reference: Vec<f32>,
    reference: Vec<f32>,
    log_bin: Vec<f32>,
    maxima: Vec<f32>,
    leaves: usize,
}

impl PeakReference {
    pub fn new(bins: usize) -> Self {
        let leaves = bins.next_power_of_two();
        Self {
            peaks: Vec::with_capacity(bins),
            peak_reference: vec![0.0; bins],
            reference: vec![0.0; bins],
            log_bin: (0..bins).map(|k| (k.max(1) as f32).log2()).collect(),
            maxima: vec![f32::NEG_INFINITY; 2 * leaves],
            leaves,
        }
    }

    pub fn analyze(
        &mut self,
        db: &[f32],
        lower: &[usize],
        upper: &[usize],
        guard: &[usize],
    ) -> &[f32] {
        self.peaks.clear();
        self.maxima[self.leaves..].fill(f32::NEG_INFINITY);
        let floor = db.iter().copied().fold(-150.0_f32, f32::max) - 60.0;
        for k in 1..db.len() - 1 {
            if db[k] > db[k - 1] && db[k] >= db[k + 1] && db[k] > floor {
                self.peaks.push(k);
                self.maxima[self.leaves + k] = db[k];
            }
        }
        // A lone sinusoid has no independent spectral context. Do not label
        // its window leakage as a set of quieter musical neighbors.
        if self.peaks.len() < 2 {
            self.reference.copy_from_slice(db);
            return &self.reference;
        }
        for i in (1..self.leaves).rev() {
            self.maxima[i] = self.maxima[2 * i].max(self.maxima[2 * i + 1]);
        }
        for &k in &self.peaks {
            // Exclude the entire candidate resonance region, not just its
            // winning bin: an EQ bump may lift several adjacent partials.
            // Detail controls its width, with a main-lobe-size minimum.
            // A segment tree keeps queries O(log FFT size), even
            // on dense/noisy spectra and at the highest overlap setting.
            let left = self.maximum(lower[k], k.saturating_sub(guard[k]));
            let right = self.maximum(k + guard[k] + 1, upper[k]);
            let reference = left.max(right);
            self.peak_reference[k] = if reference.is_finite() {
                reference
            } else {
                db[k]
            };
        }
        // A musical fundamental can naturally exceed its upper partials. Use
        // their envelope with a conservative 1/h amplitude rolloff allowance.
        // This reference still excludes the fundamental's own level, so an
        // artificially boosted fundamental remains detectable. Search upward
        // so a loud upper harmonic cannot claim to be the foundation.
        for &k in &self.peaks {
            if db[k] < floor + 20.0
                || self.maximum(k.saturating_sub(4), (k + 5).min(db.len())) > db[k]
            {
                continue;
            }
            let mut support = 0;
            let mut foundation = f32::NEG_INFINITY;
            for h in 2..=6 {
                let center = k * h;
                if center + 2 < db.len() {
                    let partial = self.maximum(center.saturating_sub(2), center + 3);
                    if partial > db[k] - 24.0 {
                        support += 1;
                        foundation = foundation.max(partial + 20.0 * (h as f32).log10());
                    }
                }
            }
            if support >= 2 {
                self.peak_reference[k] = self.peak_reference[k].max(foundation);
                break;
            }
        }
        let first = self.peaks[0];
        let last = *self.peaks.last().unwrap();
        self.reference[..first].fill(self.peak_reference[first]);
        self.reference[last..].fill(self.peak_reference[last]);
        for pair in self.peaks.windows(2) {
            let [left, right] = [pair[0], pair[1]];
            for k in left..right {
                let mix = (self.log_bin[k] - self.log_bin[left])
                    / (self.log_bin[right] - self.log_bin[left]);
                self.reference[k] = self.peak_reference[left]
                    + mix * (self.peak_reference[right] - self.peak_reference[left]);
            }
        }
        // These bins do not have room for an independent main lobe below them.
        // Especially at low latency/high sample rates, unresolved bass must
        // not be diagnosed from distant, much quieter upper harmonics.
        self.reference[..4].copy_from_slice(&db[..4]);
        &self.reference
    }

    /// Maximum over the half-open range. Empty ranges have no reference peak.
    fn maximum(&self, lo: usize, hi: usize) -> f32 {
        if lo >= hi {
            return f32::NEG_INFINITY;
        }
        let (mut lo, mut hi) = (lo + self.leaves, hi + self.leaves);
        let mut result = f32::NEG_INFINITY;
        while lo < hi {
            if lo % 2 == 1 {
                result = result.max(self.maxima[lo]);
                lo += 1;
            }
            if hi % 2 == 1 {
                hi -= 1;
                result = result.max(self.maxima[hi]);
            }
            lo /= 2;
            hi /= 2;
        }
        result
    }
}
