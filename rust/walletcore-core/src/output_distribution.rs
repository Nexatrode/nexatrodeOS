//! Compact cumulative RingCT distribution for memory-constrained decoy selection.
//! Values are delta-varints with absolute checkpoints, allowing binary search
//! without retaining a u64 per block.
use alloc::vec::Vec;
use rand_core::{CryptoRng, RngCore};

const STRIDE: usize = 256;

pub struct OutputDistribution {
    start: u64,
    base: u64,
    last: u64,
    count: usize,
    max_bytes: usize,
    deltas: Vec<u8>,
    checkpoints: Vec<(u64, usize)>,
}

/// Collect consecutive bounded node responses without exposing an incomplete
/// distribution to decoy selection. The caller must recheck its chain anchor.
pub struct DistributionDownload {
    next: u64,
    end: u64,
    max_bytes: usize,
    distribution: Option<OutputDistribution>,
}

impl DistributionDownload {
    pub fn new(end: u64, max_bytes: usize) -> Option<Self> {
        if end == 0 || end == u64::MAX || max_bytes == 0 { return None; }
        Some(Self { next: 0, end, max_bytes, distribution: None })
    }

    pub fn next_range(&self) -> Option<(u64, u64)> {
        if self.next > self.end { return None; }
        Some((self.next, self.next.saturating_add(1023).min(self.end)))
    }

    pub fn accept(&mut self, from: u64, to: u64, response: &[u8]) -> Option<()> {
        if self.next_range()? != (from, to) { return None; }
        let previous = self.distribution.as_ref().map(|d| d.last);
        let (start, base, values) = crate::parse_output_distribution_chunk(response, from, to, previous)?;
        if !values.is_empty() {
            if let Some(distribution) = self.distribution.as_mut() {
                distribution.append(start, base, &values)?;
            } else {
                if base != 0 { return None; }
                let mut distribution = OutputDistribution::new(start, base, self.max_bytes);
                distribution.append(start, base, &values)?;
                self.distribution = Some(distribution);
            }
        } else if self.distribution.is_some() {
            return None;
        }
        self.next = to.checked_add(1)?;
        Some(())
    }

    pub fn finish(self) -> Option<OutputDistribution> {
        if self.next <= self.end { return None; }
        self.distribution.filter(|distribution| !distribution.is_empty())
    }
}

impl OutputDistribution {
    pub fn new(start: u64, base: u64, max_bytes: usize) -> Self {
        Self { start, base, last: base, count: 0, max_bytes, deltas: Vec::new(), checkpoints: Vec::new() }
    }

    pub fn len(&self) -> usize { self.count }
    pub fn is_empty(&self) -> bool { self.count == 0 }

    /// Append a consecutive cumulative chunk. Validate and reserve its full
    /// encoded size before mutating the existing distribution.
    pub fn append(&mut self, from: u64, previous: u64, values: &[u64]) -> Option<()> {
        if self.start.checked_add(u64::try_from(self.count).ok()?)? != from || previous != self.last || values.is_empty() { return None; }
        let new_count = self.count.checked_add(values.len())?;
        self.start.checked_add(u64::try_from(new_count).ok()?)?;
        let mut encoded = Vec::new();
        if self.deltas.len().checked_add(values.len())? > self.max_bytes { return None; }
        encoded.try_reserve_exact(values.len()).ok()?;
        let mut last = self.last;
        for &value in values {
            let mut delta = value.checked_sub(last)?;
            loop {
                let low = (delta & 0x7f) as u8;
                delta >>= 7;
                if encoded.len() == encoded.capacity() { encoded.try_reserve(1).ok()?; }
                encoded.push(low | if delta != 0 { 0x80 } else { 0 });
                if self.deltas.len().checked_add(encoded.len())? > self.max_bytes { return None; }
                if delta == 0 { break; }
            }
            last = value;
        }
        if self.deltas.len().checked_add(encoded.len())? > self.max_bytes { return None; }
        self.deltas.try_reserve(encoded.len()).ok()?;
        let checkpoints_needed = new_count.saturating_add(STRIDE - 1) / STRIDE;
        self.checkpoints.try_reserve(checkpoints_needed.saturating_sub(self.checkpoints.len())).ok()?;
        let mut offset = 0usize;
        let mut prior = self.last;
        for (i, &value) in values.iter().enumerate() {
            if (self.count + i) % STRIDE == 0 { self.checkpoints.push((prior, self.deltas.len() + offset)); }
            while *encoded.get(offset)? & 0x80 != 0 { offset += 1; }
            offset += 1;
            prior = value;
        }
        self.deltas.extend_from_slice(&encoded);
        self.count = new_count;
        self.last = last;
        Some(())
    }

    pub fn get(&self, index: usize) -> Option<u64> {
        if index >= self.count { return None; }
        let (mut value, mut pos) = *self.checkpoints.get(index / STRIDE)?;
        for _ in (index / STRIDE * STRIDE)..=index {
            let mut delta = 0u64;
            let mut shift = 0u32;
            loop {
                let byte = *self.deltas.get(pos)?; pos += 1;
                delta = delta.checked_add(u64::from(byte & 0x7f).checked_shl(shift)?)?;
                if byte & 0x80 == 0 { break; }
                shift = shift.checked_add(7)?;
            }
            value = value.checked_add(delta)?;
        }
        Some(value)
    }

    /// First block whose cumulative count is greater than `output_index`.
    pub fn block_for_output(&self, output_index: u64) -> Option<u64> {
        if output_index < self.base || output_index >= self.last { return None; }
        let (mut low, mut high) = (0usize, self.count);
        while low < high {
            let mid = low + (high - low) / 2;
            if self.get(mid)? <= output_index { low = mid + 1; } else { high = mid; }
        }
        self.start.checked_add(u64::try_from(low).ok()?)
    }

    /// Build the initial sorted get_outs batch using the reference wallet's
    /// gamma age distribution. Includes the real input in the same batch.
    /// A partial distribution cannot be used to approximate an entire chain.
    pub fn initial_decoy_batch<R: RngCore + CryptoRng>(&self, rng: &mut R, real: u64) -> Option<Vec<u64>> {
        let mut candidates = self.sample_candidates(rng, real, &[], 15)?;
        candidates.push(real);
        candidates.sort_unstable();
        Some(candidates)
    }

    /// Replacements must exclude every prior request, including locked and
    /// invalid candidates. The caller retains that set across retry rounds.
    pub fn replacement_decoy_batch<R: RngCore + CryptoRng>(
        &self, rng: &mut R, real: u64, requested: &[u64], needed: usize,
    ) -> Option<Vec<u64>> {
        if !requested.contains(&real) { return None; }
        let mut candidates = self.sample_candidates(rng, real, requested, needed)?;
        candidates.sort_unstable();
        Some(candidates)
    }

    fn sample_candidates<R: RngCore + CryptoRng>(&self, rng: &mut R, real: u64, excluded: &[u64], needed: usize) -> Option<Vec<u64>> {
        const RING: usize = 16;
        const LOCK_WINDOW: usize = 10;
        const YEAR_BLOCKS: usize = 365 * 24 * 60 * 60 / 120;
        if needed == 0 || needed >= RING || excluded.len() > 160 || self.base != 0 || self.count <= LOCK_WINDOW { return None; }
        let highest = self.get(self.count - LOCK_WINDOW)?;
        if highest.saturating_sub(60) < RING as u64 || real >= highest { return None; }
        let blocks = self.count.min(YEAR_BLOCKS);
        let initial = self.get(self.count.saturating_sub(blocks + 1))?;
        let per_second = self.last.checked_sub(initial)? as f64 / (blocks * 120) as f64;
        if per_second <= 0.0 { return None; }
        let mut candidates = Vec::with_capacity(needed);
        for _ in 0..(10 * RING) {
            if candidates.len() == needed { break; }
            let offset = (monero_wallet::sample_decoy_age(rng) * per_second) as u64;
            if offset >= highest { continue; }
            let target = highest - 1 - offset;
            let (mut low, mut high) = (0usize, self.count);
            while low < high {
                let mid = low + (high - low) / 2;
                if self.get(mid)? < target { low = mid + 1; } else { high = mid; }
            }
            if low >= self.count { continue; }
            let previous = if low == 0 { 0 } else { self.get(low - 1)? };
            let count = self.get(low)?.checked_sub(previous)?;
            if count == 0 { continue; }
            let selected = previous.checked_add(rng.next_u64() % count)?;
            if selected != real && !excluded.contains(&selected) && !candidates.contains(&selected) { candidates.push(selected); }
        }
        if candidates.len() != needed { return None; }
        Some(candidates)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    #[test]
    fn chunked_distribution_maps_outputs_and_rejects_invalid_appends() {
        let mut d = OutputDistribution::new(20, 3, 4096);
        let values: Vec<u64> = (0..600).map(|i| 3 + i / 3).collect();
        d.append(20, 3, &values[..300]).unwrap();
        d.append(320, values[299], &values[300..]).unwrap();
        for (i, value) in values.iter().enumerate() { assert_eq!(d.get(i), Some(*value)); }
        for index in 3..values[599] {
            let expected = values.iter().position(|value| *value > index).unwrap();
            assert_eq!(d.block_for_output(index), Some(20 + expected as u64));
        }
        assert_eq!(d.block_for_output(2), None);
        assert_eq!(d.block_for_output(values[599]), None);
        assert!(d.append(620, values[599], &[values[599] - 1]).is_none());
        assert_eq!(d.len(), 600);
        assert!(OutputDistribution::new(0, 0, 1).append(0, 0, &[1, 2]).is_none());
    }

    #[test]
    fn decoy_batch_contains_real_once_and_distinct_sorted_candidates() {
        use rand_chacha::ChaCha20Rng;
        use rand_core::SeedableRng;
        let values: Vec<u64> = (1..=10_000).map(|i| i * 20).collect();
        let mut d = OutputDistribution::new(0, 0, 30_000);
        d.append(0, 0, &values).unwrap();
        let mut rng = ChaCha20Rng::from_seed([7; 32]);
        let batch = d.initial_decoy_batch(&mut rng, 100_000).unwrap();
        assert_eq!(batch.len(), 16);
        assert_eq!(batch.iter().filter(|index| **index == 100_000).count(), 1);
        assert!(batch.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(batch.iter().all(|index| *index < d.get(d.len() - 10).unwrap()));
        let replacements = d.replacement_decoy_batch(&mut rng, 100_000, &batch, 3).unwrap();
        assert_eq!(replacements.len(), 3);
        assert!(replacements.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(replacements.iter().all(|index| !batch.contains(index)));
        assert!(d.replacement_decoy_batch(&mut rng, 100_000, &[], 1).is_none());
        assert!(d.initial_decoy_batch(&mut rng, u64::MAX).is_none());
        let mut partial = OutputDistribution::new(500, 100, 100);
        partial.append(500, 100, &[101, 102]).unwrap();
        assert!(partial.initial_decoy_batch(&mut rng, 101).is_none());
    }

    #[test]
    fn download_rejects_skips_and_mismatched_chunk_boundaries() {
        let response = |start: u64, base: u64, values: Vec<u64>| {
            serde_json::json!({"result":{"status":"OK","distributions":[{"amount":0,"base":base,"binary":false,"compress":false,"start_height":start,"distribution":values}]}}).to_string()
        };
        let mut download = DistributionDownload::new(1025, 4096).unwrap();
        let first = response(0, 0, (1..=1024).collect());
        assert!(download.accept(1, 1023, first.as_bytes()).is_none());
        assert_eq!(download.next_range(), Some((0, 1023)));
        download.accept(0, 1023, first.as_bytes()).unwrap();
        assert!(download.accept(1024, 1025, response(1024, 1023, alloc::vec![1025, 1026]).as_bytes()).is_none());
        assert_eq!(download.next_range(), Some((1024, 1025)));
        download.accept(1024, 1025, response(1024, 1024, alloc::vec![1025, 1026]).as_bytes()).unwrap();
        let complete = download.finish().unwrap();
        assert_eq!(complete.len(), 1026);
        assert_eq!(complete.get(1025), Some(1026));
        let mut small = DistributionDownload::new(1, 1).unwrap();
        assert!(small.accept(0, 1, response(0, 0, alloc::vec![1, 2]).as_bytes()).is_none());
        assert_eq!(small.next_range(), Some((0, 1)));
        assert!(small.finish().is_none());
    }

    #[test]
    fn compact_distribution_handles_a_long_chain_in_bounded_space() {
        const BLOCKS: u64 = 4_000_000;
        let mut d = OutputDistribution::new(0, 0, 14 * 1024 * 1024);
        let mut start = 0u64;
        let mut last = 0u64;
        while start < BLOCKS {
            let count = (BLOCKS - start).min(1024);
            let values: Vec<u64> = (1..=count).map(|step| last + step * 3).collect();
            d.append(start, last, &values).unwrap();
            last = *values.last().unwrap();
            start += count;
        }
        assert_eq!(d.len(), BLOCKS as usize);
        assert_eq!(d.get(BLOCKS as usize - 1), Some(BLOCKS * 3));
        assert_eq!(d.block_for_output(3_000_000), Some(1_000_000));
        assert!(d.deltas.len() + d.checkpoints.len() * core::mem::size_of::<(u64, usize)>() < 5 * 1024 * 1024);
    }
}
