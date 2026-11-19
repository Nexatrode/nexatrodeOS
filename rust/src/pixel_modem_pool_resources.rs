//! Reserved symbols and the fixed private SCM scratch and SMC capability.
use personal_os_rust_server::qcom_modem_handlers::Fault;
use personal_os_rust_server::qcom_modem_pool_native;
use personal_os_rust_server::qcom_modem_pools::{ARG_BYTES, Call, MAPPINGS, Reservations, SCRATCH};

macro_rules! symbol {
    ($name:ident) => {
        #[used]
        #[unsafe(no_mangle)]
        static mut $name: u64 = 0;
    };
}
symbol!(modem_storage);
symbol!(modem_storage_paddr);
symbol!(modem_heap);
symbol!(modem_heap_paddr);
symbol!(modem_scm_args);
symbol!(modem_scm_args_paddr);
symbol!(ufs_dma_pool);
symbol!(ufs_dma_pool_paddr);
symbol!(ufs_smmu_tables);
symbol!(ufs_smmu_tables_paddr);

pub fn injected(generation: u64, peer_node: u32) -> Result<Reservations, Fault> {
    let actual = unsafe {
        [
            core::ptr::read_volatile(&raw const modem_storage),
            core::ptr::read_volatile(&raw const modem_storage_paddr),
            core::ptr::read_volatile(&raw const modem_heap),
            core::ptr::read_volatile(&raw const modem_heap_paddr),
            core::ptr::read_volatile(&raw const modem_scm_args),
            core::ptr::read_volatile(&raw const modem_scm_args_paddr),
        ]
    };
    for (i, m) in MAPPINGS.iter().enumerate() {
        if actual[2 * i] != m.virtual_address || actual[2 * i + 1] != m.region.base {
            return Err(Fault::Failed);
        }
    }
    Reservations::validate(generation, peer_node, MAPPINGS, &[])
}

/// Return the fixed uncached scratch mapping only after checking both injected
/// virtual and physical addresses against the owner profile.
pub fn validated_scm_args() -> Result<*mut u8, Fault> {
    let actual = unsafe {
        [
            core::ptr::read_volatile(&raw const modem_scm_args),
            core::ptr::read_volatile(&raw const modem_scm_args_paddr),
        ]
    };
    if actual != [MAPPINGS[2].virtual_address, SCRATCH.base] {
        return Err(Fault::Failed);
    }
    Ok(actual[0] as *mut u8)
}

/// Read and validate the candidate UFS backing-pool setvars. This checks only
/// the exact mapped virtual base and bounded physical extent injected by the
/// profile; the build audit is still responsible for proving the frame run,
/// and no SMMU route or live DMA authority is inferred here.
pub fn validated_ufs_dma_pool_profile()
-> Result<personal_os_rust_server::qcom_ufs_platform::DmaBackingPool, Fault> {
    let actual = unsafe {
        [
            core::ptr::read_volatile(&raw const ufs_dma_pool),
            core::ptr::read_volatile(&raw const ufs_dma_pool_paddr),
        ]
    };
    personal_os_rust_server::qcom_ufs_platform::DmaBackingPool::from_profile(
        actual[0],
        actual[1],
        personal_os_rust_server::qcom_ufs_platform::UFS_DMA_POOL_BYTES,
    )
    .map_err(|_| Fault::Failed)
}

/// Read and validate the candidate UFS SMMU table-page setvars. This token
/// describes CPU-owned table frames only; the CapDL audit proves the run is
/// contiguous, singly mapped, and separate from the DMA backing pool.
pub fn validated_ufs_smmu_table_pool_profile()
-> Result<personal_os_rust_server::qcom_ufs_platform::UfsSmmuTablePool, Fault> {
    let actual = unsafe {
        [
            core::ptr::read_volatile(&raw const ufs_smmu_tables),
            core::ptr::read_volatile(&raw const ufs_smmu_tables_paddr),
        ]
    };
    personal_os_rust_server::qcom_ufs_platform::UfsSmmuTablePool::from_profile(
        actual[0],
        actual[1],
        personal_os_rust_server::qcom_ufs_platform::UFS_SMMU_TABLE_POOL_BYTES,
    )
    .map_err(|_| Fault::Failed)
}

pub struct Smc;
fn sync() {
    #[cfg(target_arch = "aarch64")]
    unsafe {
        core::arch::asm!("dsb sy", options(nostack, preserves_flags));
    }
    #[cfg(not(target_arch = "aarch64"))]
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
}

impl personal_os_rust_server::qcom_scm_assign::Transport for Smc {
    fn invoke(&mut self, words: [u64; 8]) -> Result<[u64; 8], Fault> {
        let args = sel4::sys::seL4_ARM_SMCContext {
            x0: words[0],
            x1: words[1],
            x2: words[2],
            x3: words[3],
            x4: words[4],
            x5: words[5],
            x6: words[6],
            x7: words[7],
        };
        let mut reply = sel4::sys::seL4_ARM_SMCContext::default();
        sync();
        let error = sel4::with_ipc_buffer_mut(|ipc| {
            ipc.inner_mut().seL4_ARM_SMC_Call(7, &args, &mut reply)
        });
        sync();
        if error != 0 {
            return Err(Fault::Uncertain);
        }
        Ok([
            reply.x0, reply.x1, reply.x2, reply.x3, reply.x4, reply.x5, reply.x6, reply.x7,
        ])
    }
}

struct CounterDelay;
impl personal_os_rust_server::qcom_scm_assign::Delay for CounterDelay {
    fn wait_ms(&mut self, milliseconds: u32) -> Result<(), Fault> {
        #[cfg(target_arch = "aarch64")]
        {
            let frequency: u64;
            let start: u64;
            unsafe {
                core::arch::asm!("mrs {v}, cntfrq_el0", v = out(reg) frequency, options(nostack, preserves_flags));
                core::arch::asm!("isb", "mrs {v}, cntpct_el0", v = out(reg) start, options(nostack, preserves_flags));
            }
            if !(1_000_000..=1_000_000_000).contains(&frequency) || milliseconds == 0 {
                return Err(Fault::Uncertain);
            }
            let ticks = frequency
                .checked_mul(u64::from(milliseconds))
                .and_then(|value| value.checked_add(999))
                .map(|value| value / 1000)
                .ok_or(Fault::Uncertain)?;
            for _ in 0..50_000_000 {
                let now: u64;
                unsafe {
                    core::arch::asm!("isb", "mrs {v}, cntpct_el0", v = out(reg) now, options(nostack, preserves_flags));
                }
                if now.wrapping_sub(start) >= ticks {
                    return Ok(());
                }
                core::hint::spin_loop();
            }
            Err(Fault::Uncertain)
        }
        #[cfg(not(target_arch = "aarch64"))]
        {
            let _ = milliseconds;
            Err(Fault::Uncertain)
        }
    }
}

struct LifecycleCheck<'a>(&'a mut dyn FnMut() -> Result<(), Fault>);
impl personal_os_rust_server::qcom_scm_assign::Lifecycle for LifecycleCheck<'_> {
    fn revalidate(&mut self) -> Result<(), Fault> {
        (self.0)()
    }
}

fn invoke_assign(
    call: &Call,
    lifecycle: &mut dyn FnMut() -> Result<(), Fault>,
) -> Result<[u64; 8], Fault> {
    let mut smc = Smc;
    let mut delay = CounterDelay;
    personal_os_rust_server::qcom_scm_assign::invoke_checked(
        &mut smc,
        &mut delay,
        &mut LifecycleCheck(lifecycle),
        *call.words(),
    )
}

/// Invoke the platform's serialized SCM capability for one sealed assignment.
/// The owner adapter supplies only calls generated from a validated MSA plan.
pub fn invoke_scm(
    call: &Call,
    lifecycle: &mut dyn FnMut() -> Result<(), Fault>,
) -> Result<[u64; 8], Fault> {
    invoke_assign(call, lifecycle)
}

impl qcom_modem_pool_native::Smc for Smc {
    fn invoke(&mut self, call: &Call) -> Result<[u64; 8], Fault> {
        <Self as personal_os_rust_server::qcom_scm_assign::Transport>::invoke(self, *call.words())
    }
}

impl personal_os_rust_server::qcom_msa_native::Scm for Smc {
    fn invoke(
        &mut self,
        call: &Call,
        lifecycle: &mut dyn FnMut() -> Result<(), Fault>,
    ) -> Result<[u64; 8], Fault> {
        invoke_assign(call, lifecycle)
    }
}

const _: () = assert!(ARG_BYTES <= 4096);
