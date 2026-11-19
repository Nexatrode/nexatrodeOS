//! Optional wall-clock measurements scoped to owner dispatch.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct IoTiming {
    pub total: [u64; 4],
    pub calls: [u32; 4],
    #[cfg(feature = "sunfish-tftp-publish-op-observer")]
    pub detail_total: [u64; 6],
    #[cfg(feature = "sunfish-tftp-publish-op-observer")]
    pub detail_calls: [u32; 6],
    #[cfg(feature = "sunfish-tftp-barrier-domain-observer")]
    pub barrier_domain_total: [u64; 2],
    #[cfg(feature = "sunfish-tftp-barrier-domain-observer")]
    pub barrier_domain_calls: [u32; 2],
    enabled: bool,
}

impl IoTiming {
    pub fn enable(&mut self, enabled: bool) { self.enabled = enabled; }
    pub fn enabled(&self) -> bool { self.enabled }
    pub fn record(&mut self, category: usize, elapsed: u64) {
        if self.enabled {
            self.total[category] = self.total[category].saturating_add(elapsed);
            self.calls[category] = self.calls[category].saturating_add(1);
        }
    }
    #[cfg(feature = "sunfish-tftp-publish-op-observer")]
    pub fn record_detail(&mut self, category: usize, elapsed: u64) {
        if self.enabled {
            self.detail_total[category] = self.detail_total[category].saturating_add(elapsed);
            self.detail_calls[category] = self.detail_calls[category].saturating_add(1);
        }
    }
    #[cfg(feature = "sunfish-tftp-barrier-domain-observer")]
    pub fn record_barrier_domains(&mut self, samples: [Option<u64>; 4]) {
        if !self.enabled { return; }
        for i in 0..2 {
            if let (Some(start), Some(end)) = (samples[i * 2], samples[i * 2 + 1]) {
                self.barrier_domain_total[i] = self.barrier_domain_total[i]
                    .saturating_add(end.wrapping_sub(start));
                self.barrier_domain_calls[i] = self.barrier_domain_calls[i].saturating_add(1);
            }
        }
    }
    #[cfg(feature = "sunfish-tftp-barrier-domain-observer")]
    pub fn barrier_domain_observation(&self, before: &Self) -> [u64; 2] {
        core::array::from_fn(|i| {
            if self.barrier_domain_calls[i] == u32::MAX
                || self.barrier_domain_calls[i] <= before.barrier_domain_calls[i]
            { u64::MAX } else { self.barrier_domain_total[i] }
        })
    }
}
