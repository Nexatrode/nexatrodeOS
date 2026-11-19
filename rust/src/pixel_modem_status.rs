//! Passive SMEM observer. The only writes are TCSR lock3 acquire/release.
use personal_os_rust_server::qcom_modem_status::{
    self as core, Directory, Error, Item, Read, Route, Status,
};
use personal_os_rust_server::qcom_pas_auth::AuthOnce;

pub struct Window {
    smem: *const u8,
    lock: *mut u32,
}
impl Read for Window {
    fn read8(&self, at: usize) -> u8 {
        assert!(at < core::SMEM_BYTES);
        unsafe { self.smem.add(at).read_volatile() }
    }
}
pub struct Baseline {
    pub(crate) route: Route,
    pub(crate) uncached: usize,
    pub(crate) cached: usize,
}
#[derive(Clone, Copy, Debug)]
pub struct Report {
    pub error: Option<Error>,
    pub samples: u32,
    pub directory: Option<Directory>,
    pub status: Option<Status>,
    pub ready_bit_seen: bool,
    pub auth_passed: bool,
    pub elapsed_ticks: u64,
    pub frequency: u64,
}
pub trait Clock {
    fn now(&mut self) -> u64;
    fn frequency(&self) -> u64;
}
pub(crate) fn sync() {
    #[cfg(target_arch = "aarch64")]
    unsafe {
        ::core::arch::asm!("dsb sy", options(nostack, preserves_flags));
    }
    #[cfg(not(target_arch = "aarch64"))]
    std_sync();
}
#[cfg(not(target_arch = "aarch64"))]
fn std_sync() {
    ::core::sync::atomic::fence(::core::sync::atomic::Ordering::SeqCst);
}
impl Window {
    /// # Safety
    /// Exclusive trusted-PD uncached read view of physical SMEM
    /// 0x86000000..0x86200000; lock points only to TCSR semaphore3 at0x01f43000.
    /// The passive manifest is RO. The separate status writer admits exactly
    /// one RW page. The separate fresh-GLINK experiments admit exactly five
    /// RW pages with their own bounded writers. This view never writes SMEM.
    /// No alias, other AP client or IRQ handler is admitted.
    pub unsafe fn new(smem: *const u8, lock: *mut u32) -> Self {
        Self { smem, lock }
    }
    pub(crate) fn locked<T>(
        &mut self,
        f: impl FnOnce(&Self) -> Result<T, Error>,
    ) -> Result<T, Error> {
        let mut owned = false;
        for _ in 0..65536 {
            unsafe { self.lock.write_volatile(1) };
            sync();
            if unsafe { self.lock.read_volatile() } == 1 {
                owned = true;
                break;
            }
        }
        if !owned {
            return Err(Error::Lock);
        }
        let result = f(self);
        sync();
        if unsafe { self.lock.read_volatile() } != 1 {
            return Err(Error::LockLost);
        }
        unsafe { self.lock.write_volatile(0) };
        sync();
        result
    }
    fn sample(&mut self) -> Result<(Directory, Option<Status>), Error> {
        self.locked(|this| {
            let d = core::directory(this)?;
            let status = if let Some(item) = d.item {
                let mut value = Err(Error::Unstable);
                for _ in 0..3 {
                    value = core::payload(this, item);
                    if value != Err(Error::Unstable) {
                        break;
                    }
                    sync();
                }
                Some(value?)
            } else {
                None
            };
            Ok((d, status))
        })
    }
    pub fn before_auth(&mut self) -> Result<Baseline, Error> {
        let (d, _) = self.sample()?;
        if d.item.is_some() {
            return Err(Error::BaselinePresent);
        }
        Ok(Baseline {
            route: d.route,
            uncached: d.free_uncached,
            cached: d.free_cached,
        })
    }
    pub fn revalidate(&mut self, b: &Baseline) -> Result<(), Error> {
        let (d, _) = self.sample()?;
        if d.item.is_some() {
            return Err(Error::BaselinePresent);
        }
        if d.route != b.route || d.free_uncached != b.uncached || d.free_cached != b.cached {
            return Err(Error::RouteChanged);
        }
        Ok(())
    }
    /// Consumes both the absent-before-auth baseline and successful sealed
    /// authentication session. A ready bit is evidence only, never a peer cap.
    pub fn observe(
        &mut self,
        baseline: Baseline,
        auth: AuthOnce,
        clock: &mut impl Clock,
        mut progress: impl FnMut(&Report),
    ) -> Report {
        let mut r = Report {
            error: None,
            samples: 0,
            directory: None,
            status: None,
            ready_bit_seen: false,
            auth_passed: false,
            elapsed_ticks: 0,
            frequency: clock.frequency(),
        };
        if auth.into_returned().is_err() {
            r.error = Some(Error::Auth);
            return r;
        }
        r.auth_passed = true;
        if !(1_000_000..=1_000_000_000).contains(&r.frequency) {
            r.error = Some(Error::Clock);
            return r;
        }
        let start = clock.now();
        let mut identity: Option<Item> = None;
        for poll in 0..1024 {
            r.samples = poll + 1;
            match self.sample() {
                Err(e) => {
                    r.error = Some(e);
                    break;
                }
                Ok((d, status)) => {
                    r.directory = Some(d);
                    r.status = status;
                    if d.route != baseline.route
                        || d.free_uncached < baseline.uncached
                        || d.free_cached > baseline.cached
                        || identity.is_some_and(|item| Some(item) != d.item)
                    {
                        r.error = Some(Error::RouteChanged);
                        break;
                    }
                    if d.item.is_some() {
                        identity = d.item;
                    }
                    if status.is_some_and(|s| s.fatal_bit()) {
                        r.error = Some(Error::Fatal);
                        break;
                    }
                    if status.is_some_and(|s| s.ready_bit()) {
                        r.ready_bit_seen = true;
                        break;
                    }
                }
            }
            r.elapsed_ticks = clock.now().wrapping_sub(start);
            if r.elapsed_ticks >= r.frequency * 10 {
                r.error = Some(Error::Timeout);
                break;
            }
            if poll % 100 == 0 {
                progress(&r);
            }
            // Space metadata samples by about10ms, with an independent work
            // limit so a stopped counter cannot cause an infinite wait.
            let wait = clock.now();
            let mut advanced = false;
            for _ in 0..1_000_000 {
                if clock.now().wrapping_sub(wait) >= r.frequency / 100 {
                    advanced = true;
                    break;
                }
                ::core::hint::spin_loop();
            }
            if !advanced {
                r.error = Some(Error::Clock);
                break;
            }
        }
        if !r.ready_bit_seen && r.error.is_none() {
            r.error = Some(Error::Timeout);
        }
        r.elapsed_ticks = clock.now().wrapping_sub(start);
        r
    }
}

/// The unchanged Sunfish seL4 build explicitly exports the physical counter
/// to EL0 (CONFIG_EXPORT_PCNT_USER=1); virtual-counter export is disabled.
pub struct PhysicalCounter {
    frequency: u64,
}
impl PhysicalCounter {
    #[cfg(target_arch = "aarch64")]
    pub fn new() -> Result<Self, Error> {
        let frequency: u64;
        unsafe {
            ::core::arch::asm!("mrs {v}, cntfrq_el0",v=out(reg)frequency,options(nostack,preserves_flags));
        }
        if !(1_000_000..=1_000_000_000).contains(&frequency) {
            return Err(Error::Clock);
        }
        let mut c = Self { frequency };
        let first = c.now();
        for _ in 0..100000 {
            if c.now() != first {
                return Ok(c);
            }
        }
        Err(Error::Clock)
    }
}
impl Clock for PhysicalCounter {
    fn frequency(&self) -> u64 {
        self.frequency
    }
    fn now(&mut self) -> u64 {
        #[cfg(target_arch = "aarch64")]
        {
            let value: u64;
            unsafe {
                ::core::arch::asm!("isb","mrs {v}, cntpct_el0",v=out(reg)value,options(nostack,preserves_flags));
            }
            value
        }
        #[cfg(not(target_arch = "aarch64"))]
        {
            unreachable!("physical counter only exists on target")
        }
    }
}
