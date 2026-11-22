//! Validate batched node outputs before building one input ring.
use alloc::vec::Vec;
use rand_core::{CryptoRng, RngCore};
use monero_wallet::ed25519::CompressedPoint;
use crate::{OutputDistribution, RingMember, RING_LEN, parse_get_outs_batch, valid_ring_points};

/// Own the randomness across network round trips so replacement requests use
/// a continuous stream rather than restarting the selection sequence.
pub struct DecoyRingSession {
    rng: rand_chacha::ChaCha20Rng,
    collector: DecoyRingCollector,
}

impl DecoyRingSession {
    pub fn start(distribution: &OutputDistribution, seed: zeroize::Zeroizing<[u8; 32]>, real: RingMember) -> Option<Self> {
        use rand_core::SeedableRng;
        let mut rng = rand_chacha::ChaCha20Rng::from_seed(*seed);
        let collector = DecoyRingCollector::start(distribution, &mut rng, real)?;
        Some(Self { rng, collector })
    }

    pub fn request_indices(&self) -> &[u64] { self.collector.request_indices() }
    pub fn accept(&mut self, response: &[u8]) -> Option<()> { self.collector.accept(response) }
    pub fn is_complete(&self) -> bool { self.collector.is_complete() }
    pub fn request_replacements(&mut self, distribution: &OutputDistribution) -> Option<&[u64]> {
        self.collector.request_replacements(distribution, &mut self.rng)
    }
    pub fn finish(self) -> Option<Vec<RingMember>> { self.collector.finish() }
}

pub struct DecoyRingCollector {
    real: RingMember,
    requested: Vec<u64>,
    in_flight: Vec<u64>,
    accepted: Vec<RingMember>,
    real_verified: bool,
}

impl DecoyRingCollector {
    pub fn start<R: RngCore + CryptoRng>(distribution: &OutputDistribution, rng: &mut R, real: RingMember) -> Option<Self> {
        let in_flight = distribution.initial_decoy_batch(rng, real.0)?;
        Some(Self { real, requested: in_flight.clone(), in_flight, accepted: Vec::new(), real_verified: false })
    }

    pub fn request_indices(&self) -> &[u64] { &self.in_flight }
    pub fn is_complete(&self) -> bool {
        self.real_verified && self.in_flight.is_empty() && self.accepted.len() == usize::from(RING_LEN)
    }

    /// Apply a complete response atomically. Unusable decoys are not retained,
    /// while a missing or different real output invalidates the whole batch.
    pub fn accept(&mut self, response: &[u8]) -> Option<()> {
        if self.in_flight.is_empty() { return None; }
        let outputs = parse_get_outs_batch(response, self.in_flight.len())?;
        let mut additions = Vec::new();
        let mut real_verified = self.real_verified;
        for (index, output) in self.in_flight.iter().copied().zip(outputs) {
            if index == self.real.0 {
                if output != Some((self.real.1, self.real.2)) { return None; }
                real_verified = true;
            }
            let Some((key_bytes, commitment_bytes)) = output else { continue; };
            let Some(key) = CompressedPoint::from(key_bytes).decompress() else { continue; };
            let Some(commitment) = CompressedPoint::from(commitment_bytes).decompress() else { continue; };
            if !valid_ring_points(key, commitment) {
                if index == self.real.0 { return None; }
                continue;
            }
            additions.push((index, key_bytes, commitment_bytes));
        }
        if !real_verified { return None; }
        self.accepted.extend(additions);
        self.real_verified = real_verified;
        self.in_flight.clear();
        Some(())
    }

    pub fn request_replacements<R: RngCore + CryptoRng>(&mut self, distribution: &OutputDistribution, rng: &mut R) -> Option<&[u64]> {
        if !self.real_verified || !self.in_flight.is_empty() || self.accepted.len() >= usize::from(RING_LEN) { return None; }
        let needed = usize::from(RING_LEN).checked_sub(self.accepted.len())?;
        let next = distribution.replacement_decoy_batch(rng, self.real.0, &self.requested, needed)?;
        if self.requested.len().checked_add(next.len())? > 160 { return None; }
        self.requested.extend_from_slice(&next);
        self.in_flight = next;
        Some(&self.in_flight)
    }

    pub fn finish(mut self) -> Option<Vec<RingMember>> {
        if !self.is_complete() { return None; }
        self.accepted.sort_unstable_by_key(|member| member.0);
        Some(self.accepted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{format, string::ToString};
    use curve25519_dalek::constants::ED25519_BASEPOINT_POINT;
    use rand_chacha::ChaCha20Rng;
    use rand_core::SeedableRng;

    fn response(indices: &[u64], real: u64, point: &[u8; 32], locked: Option<u64>, mismatch: bool) -> alloc::string::String {
        let hex: alloc::string::String = point.iter().map(|byte| format!("{byte:02x}")).collect();
        let entries: Vec<_> = indices.iter().map(|index| {
            let key = if mismatch && *index == real { "00".repeat(32) } else { hex.clone() };
            serde_json::json!({"key": key, "mask": hex, "unlocked": Some(*index) != locked})
        }).collect();
        serde_json::json!({"status":"OK","outs":entries}).to_string()
    }

    #[test]
    fn checks_real_output_and_replaces_locked_candidates() {
        let mut distribution = OutputDistribution::new(0, 0, 30_000);
        distribution.append(0, 0, &(1..=10_000).map(|i| i * 20).collect::<Vec<_>>()).unwrap();
        let point = ED25519_BASEPOINT_POINT.compress().to_bytes();
        let real = 100_000;
        let mut rng = ChaCha20Rng::from_seed([9; 32]);
        let mut collector = DecoyRingCollector::start(&distribution, &mut rng, (real, point, point)).unwrap();
        let first = collector.request_indices().to_vec();
        assert!(collector.accept(response(&first, real, &point, Some(real), false).as_bytes()).is_none());
        assert!(collector.accept(br#"{"status":"OK","outs":[]}"#).is_none());
        assert!(collector.accept(response(&first, real, &point, None, true).as_bytes()).is_none());
        assert_eq!(collector.request_indices(), first);
        let locked = *first.iter().find(|index| **index != real).unwrap();
        collector.accept(response(&first, real, &point, Some(locked), false).as_bytes()).unwrap();
        let replacements = collector.request_replacements(&distribution, &mut rng).unwrap().to_vec();
        assert_eq!(replacements.len(), 1);
        assert!(!first.contains(&replacements[0]));
        collector.accept(response(&replacements, real, &point, None, false).as_bytes()).unwrap();
        let ring = collector.finish().unwrap();
        assert_eq!(ring.len(), 16);
        assert!(!ring.iter().any(|entry| entry.0 == locked));
        assert_eq!(ring.iter().filter(|entry| entry.0 == real).count(), 1);
    }
}
