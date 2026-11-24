//! Disposable dynamic-lock transaction policy. Caller identity comes from
//! capabilities; the trusted manager owns the epoch and the preparation lease.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u64)]
pub enum Phase { Evicted = 0, Armed = 1, Prepared = 2, Active = 3 }

pub struct VaultLifecycle { epoch: u64, phase: Phase }
impl VaultLifecycle {
    pub const fn new() -> Self { Self { epoch: 0, phase: Phase::Evicted } }
    pub fn epoch(&self) -> u64 { self.epoch }
    pub fn phase(&self) -> Phase { self.phase }
    pub fn evict(&mut self, epoch: u64) -> bool {
        if epoch == 0 || epoch < self.epoch { return false; }
        self.epoch = epoch; self.phase = Phase::Evicted; true
    }
    pub fn arm(&mut self, epoch: u64) -> bool {
        if epoch == 0 || epoch != self.epoch || self.phase != Phase::Evicted { return false; }
        self.phase = Phase::Armed; true
    }
    pub fn may_prepare(&self, epoch: u64) -> bool { epoch == self.epoch && self.phase == Phase::Armed }
    pub fn prepared(&mut self, epoch: u64) -> bool {
        if !self.may_prepare(epoch) { return false; }
        self.phase = Phase::Prepared; true
    }
    pub fn commit(&mut self, epoch: u64) -> bool {
        if epoch != self.epoch || self.phase != Phase::Prepared { return false; }
        self.phase = Phase::Active; true
    }
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn vault_lifecycle_requires_arm_prepare_then_commit() {
        let mut s = VaultLifecycle::new();
        assert!(!s.arm(0)); assert!(!s.commit(0)); assert!(s.evict(1));
        assert!(!s.prepared(1)); assert!(!s.commit(1)); assert!(s.arm(1));
        assert!(!s.arm(1)); assert!(!s.commit(1)); assert!(s.prepared(1));
        assert!(!s.prepared(1)); assert!(s.commit(1)); assert!(!s.commit(1));
        assert!(!s.may_prepare(1)); assert_eq!(s.phase(), Phase::Active);
    }
    #[test] fn vault_lifecycle_relock_cancels_every_inflight_phase() {
        for phase in [Phase::Armed, Phase::Prepared, Phase::Active] {
            let mut s = VaultLifecycle::new(); assert!(s.evict(5)); assert!(s.arm(5));
            if phase != Phase::Armed { assert!(s.prepared(5)); }
            if phase == Phase::Active { assert!(s.commit(5)); }
            assert!(s.evict(6)); assert_eq!(s.phase(), Phase::Evicted);
            assert!(!s.prepared(5)); assert!(!s.commit(5)); assert!(!s.arm(5)); assert!(!s.evict(5));
            assert_eq!(s.epoch(), 6); assert!(s.arm(6)); assert!(s.prepared(6)); assert!(s.commit(6));
        }
    }
    #[test] fn vault_lifecycle_same_epoch_abort_evicts_and_requires_new_arm() {
        let mut s = VaultLifecycle::new(); assert!(s.evict(1)); assert!(s.arm(1)); assert!(s.prepared(1));
        assert!(s.evict(1)); assert!(!s.commit(1)); assert!(!s.prepared(1)); assert!(s.arm(1));
    }
}
