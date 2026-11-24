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

/// Four disjoint outer I/O categories for one diagnostic queue-to-arm window.
#[cfg(feature="sunfish-registry-prearm-io-trace")]
#[derive(Clone,Copy,Debug,Default,PartialEq,Eq)]
pub struct WindowTiming { total:[u64;4], seen:bool, enabled:bool }
#[cfg(feature="sunfish-registry-prearm-io-trace")]
impl WindowTiming {
    pub fn begin(&mut self){self.total=[0;4];self.seen=true;self.enabled=true;}
    pub fn finish(&mut self){self.enabled=false;}
    pub fn enabled(&self)->bool{self.enabled}
    pub fn record(&mut self,category:usize,ticks:u64){if self.enabled{self.total[category]=self.total[category].saturating_add(ticks);}}
    pub fn snapshot(&self)->Option<[u64;12]>{self.seen.then(||[self.total[0],self.total[1],self.total[2],self.total[3],0,0,0,0,0,0,0,0])}
}
#[cfg(all(test,feature="sunfish-registry-prearm-io-trace"))]
#[test]
fn prearm_window_counter_is_independent_of_owner_scope_and_freezes_without_resetting_evidence(){
    let mut owner=IoTiming::default();let mut window=WindowTiming::default();assert_eq!(window.snapshot(),None);
    window.begin();owner.enable(false);owner.record(0,7);window.record(0,7);assert_eq!(owner.total,[0;4]);assert_eq!(window.snapshot().unwrap()[0],7);
    owner.enable(true);owner.record(1,9);window.record(1,9);window.finish();window.record(1,99);assert_eq!(&window.snapshot().unwrap()[..4],&[7,9,0,0]);assert_eq!(owner.total[1],9);
    window.begin();assert_eq!(&window.snapshot().unwrap()[..4],&[0;4]);window.record(2,u64::MAX);window.record(2,1);assert_eq!(window.snapshot().unwrap()[2],u64::MAX);
}
