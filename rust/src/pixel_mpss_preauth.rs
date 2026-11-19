//! Real retained IPA/RMTFS -> modem power -> SMP2P publication, before PAS4.
//! No MPSS reset release, GLINK publication or additional host DMA command.
use super::*;
use core::sync::atomic::{AtomicU8, AtomicU32, AtomicUsize, Ordering};
use personal_os_rust_server::{
    qcom_cmd_db::Database,
    sunfish_ipa_linux_pas::Resources,
    sunfish_mpss_linux_power::{self as power},
    sunfish_mpss_linux_power_native as native, sunfish_mpss_linux_smp2p as smp,
    sunfish_mpss_linux_smp2p_native as signal,
};
use sel4_microkit::{memory_region_symbol, var};
const GENERATION: u64 = 1;
const DB_BYTES: usize = 0x1078;
// 1=power preparation, 2=signaling, 3=final verified, 4=verifying, 5=terminal.
static STATE: AtomicU8 = AtomicU8::new(0);
static ERROR: AtomicU32 = AtomicU32::new(0);
static RAM: AtomicUsize = AtomicUsize::new(0);
static APCS: AtomicUsize = AtomicUsize::new(0);
pub(super) type Power = power::HoldOnce<native::Native<'static, Gate>>;
#[derive(Clone, Copy, Eq, PartialEq)]
struct Views {
    ram: *mut u32,
    apcs: *mut u32,
}
fn views() -> Result<Views, power::Error> {
    let ram = memory_region_symbol!(mpss_aop_msgram:*mut u32).as_ptr();
    let apcs = memory_region_symbol!(mpss_apcs:*mut u32).as_ptr();
    if ram.is_null()
        || apcs.is_null()
        || ram as usize % 4096 != 0
        || apcs as usize % 4096 != 0
        || *var!(mpss_aop_msgram_paddr:usize=0) != 0x0c300000
        || *var!(mpss_apcs_paddr:usize=0) != 0x17c00000
    {
        return Err(power::Error::Address);
    }
    Ok(Views { ram, apcs })
}
struct DatabaseSnapshot {
    dictionary: *const u32,
    data: *const u32,
    bytes: [u8; DB_BYTES],
}
fn word(p: *const u32, at: usize) -> u32 {
    u32::from_le(unsafe { p.add(at / 4).read_volatile() })
}
impl DatabaseSnapshot {
    fn capture(dictionary: *const u32, data: *const u32) -> Result<Self, power::Error> {
        // The parent entry and actual CapDL pin these injected addresses.
        // Keep the subviews relational here, without fabricating pointers.
        if dictionary.is_null()
            || data.is_null()
            || dictionary as usize % 4096 != 0
            || (dictionary as usize).checked_add(4096) != Some(data as usize)
        {
            return Err(power::Error::Address);
        }
        let mut s = Self {
            dictionary,
            data,
            bytes: [0; DB_BYTES],
        };
        if s.pointer() != (0x85f20000, DB_BYTES as u32) {
            return Err(power::Error::Database);
        }
        for n in 0..DB_BYTES / 4 {
            s.bytes[n * 4..n * 4 + 4].copy_from_slice(&word(data, n * 4).to_le_bytes());
        }
        s.check()?;
        Ok(s)
    }
    fn pointer(&self) -> (u32, u32) {
        (word(self.dictionary, 0xc), word(self.dictionary, 0x10))
    }
    fn check(&self) -> Result<(), power::Error> {
        if self.pointer() != (0x85f20000, DB_BYTES as u32) {
            return Err(power::Error::Database);
        }
        for _ in 0..2 {
            for n in 0..DB_BYTES / 4 {
                if self.bytes[n * 4..n * 4 + 4] != word(self.data, n * 4).to_le_bytes() {
                    return Err(power::Error::Database);
                }
            }
        }
        if self.pointer() != (0x85f20000, DB_BYTES as u32) {
            return Err(power::Error::Database);
        }
        Ok(())
    }
    fn plan(&self) -> Result<power::Plan, power::Error> {
        power::Plan::from_database(
            &Database::parse(&self.bytes).map_err(|_| power::Error::Database)?,
        )
    }
}
pub(super) struct Gate {
    database: DatabaseSnapshot,
    views: Views,
}
impl native::Gate for Gate {
    fn check(&mut self, g: u64, pre_auth: bool) -> Result<(), power::Error> {
        if g != GENERATION
            || !pre_auth && !matches!(STATE.load(Ordering::Acquire), 3 | 4)
            || pre_auth && STATE.load(Ordering::Acquire) != 1
            || !retained_generation(g, pre_auth)
            || !pixel_smmu_install::preboot_faults_clear()
        {
            return Err(power::Error::Guard);
        }
        if views()? != self.views {
            return Err(power::Error::Address);
        }
        self.database.check()
    }
    fn wait(&mut self) -> Result<(), power::Error> {
        // A work bound, not Linux's one-second scheduler timeout. Never
        // reenter RSC, clear a mailbox, reset or poll another AP endpoint.
        for _ in 0..4096 {
            core::hint::spin_loop();
        }
        Ok(())
    }
}
fn retained_generation(g: u64, pre_auth: bool) -> bool {
    #[cfg(feature = "sunfish-native-startup-mpss-auth")]
    if !pre_auth {
        return pixel_linux_rmtfs::retained_generation(g);
    }
    let _ = pre_auth;
    pixel_linux_rmtfs::preboot_generation(g)
}
fn signaling(g: u64) -> Result<smp::Observation, smp::Error> {
    if g != GENERATION
        || !matches!(STATE.load(Ordering::Acquire), 2 | 3 | 4)
        || !retained_generation(g, false)
        || !pixel_smmu_install::preboot_faults_clear()
        || !views().is_ok_and(|v| {
            v.ram as usize == RAM.load(Ordering::Acquire)
                && v.apcs as usize == APCS.load(Ordering::Acquire)
        })
    {
        return Err(smp::Error::Guard);
    }
    // Actual power/SCM owners were verified immediately before this serialized
    // handoff and stay retained with no unvote writer. The default entry stays
    // PreAuth; the opt-in auth entry reports the actual root reset lifecycle.
    #[cfg(not(feature = "sunfish-native-startup-mpss-auth"))]
    let lifecycle = smp::Lifecycle::PreAuth;
    #[cfg(feature = "sunfish-native-startup-mpss-auth")]
    let lifecycle = pixel_linux_rmtfs::lifecycle(g).ok_or(smp::Error::Guard)?;
    Ok(smp::Observation {
        generation: g,
        lifecycle,
        power: smp::Power::Active,
    })
}
#[cfg(feature = "sunfish-native-startup-mpss-auth")]
pub(super) fn verified() -> bool {
    STATE.load(Ordering::Acquire) == 3
}
#[cfg(feature = "sunfish-native-startup-mpss-auth")]
pub(super) fn verify_retained(s: &mut Startup) -> Result<(), power::Error> {
    verify(s)
}
fn healthy(s: &Startup) -> bool {
    pixel_ipa_receive::healthy(s)
        && s._domain.as_ref().is_some_and(|d| d.retained_quiet())
        && s._linux_rmtfs
            .as_ref()
            .is_some_and(|f| f.phase() == pixel_linux_rmtfs::Phase::Prepared)
        && pixel_linux_rmtfs::preboot_generation(GENERATION)
}
pub(super) fn started() -> bool {
    STATE.load(Ordering::Acquire) != 0
}
pub(super) fn fault(s: &mut Startup, code: u32) {
    if !started() {
        return;
    }
    ERROR
        .compare_exchange(0, code, Ordering::AcqRel, Ordering::Acquire)
        .ok();
    STATE.store(5, Ordering::Release);
    if let Some(f) = s._linux_rmtfs.as_mut() {
        if f.phase() != pixel_linux_rmtfs::Phase::Failed {
            f.fault(code);
        }
    }
}
pub(super) fn show(d: &Display, s: &Startup) {
    d.clear();
    d.replace_line(0, "MODEM PREAUTH PROVIDERS");
    d.replace_line(
        1,
        match STATE.load(Ordering::Acquire) {
            0 => "NOT STARTED",
            1 => "HOLD MODEM POWER",
            2 => "PUBLISH HOST SMP2P",
            3 => "PREAUTH PROVIDERS PASS",
            4 => "VERIFY RETAINED HOLDS",
            _ => "PREAUTH PROVIDERS STOP",
        },
    );
    d.hex(2, "NATIVE ERROR", ERROR.load(Ordering::Relaxed));
    if let Some(p) = s._modem_power.as_ref() {
        let r = p.report();
        let n = p.control().report();
        d.hex(3, "POWER PHASE", r.phase as u32);
        d.hex(4, "POWER ERROR", r.error.map_or(0, |e| e.code() as u32));
        d.hex(5, "AOP CHECKPOINT", n.phase as u32);
        d.hex(6, "AOP STORES", n.stores);
        d.hex(7, "AOP NOTIFICATIONS", n.notifications);
        d.hex(8, "AOP TX LENGTH", n.transmit_length);
        d.hex(9, "AOP RX LENGTH", n.receive_length);
        d.hex(10, "RAIL ATTEMPTS", r.attempted as u32);
        d.hex(11, "RAIL ACKS", r.acknowledged as u32);
        for (i, name) in ["CX FINAL", "MSS FINAL", "MX FINAL", "XO FINAL"]
            .into_iter()
            .enumerate()
        {
            d.hex(12 + i, name, r.final_values[i]);
        }
    }
    if let Some(r) = s._domain.as_ref().and_then(|d| d.modem_smp2p_report()) {
        d.hex(17, "SMP2P PHASE", r.phase as u32);
        d.hex(18, "SMP STORES SENT", r.submitted);
        d.hex(19, "SMP STORES DONE", r.completed);
        d.hex(20, "SMP NOTIFIES SENT", r.kicks_sent);
        d.hex(21, "SMP NOTIFIES DONE", r.kicks_done);
    }
    d.hex(
        23,
        "IPA RX HEALTHY",
        u32::from(pixel_ipa_receive::healthy(s)),
    );
    d.replace_line(24, "RMTFS AND SMEM RETAINED");
    d.replace_line(25, "NO MPSS RESET RELEASE");
    d.replace_line(26, "NO GLINK OR WLAN START");
    d.replace_line(28, "NO FLASH OR RETRY");
    d.replace_line(29, "RESTART WHEN DONE");
    d.flush();
}
pub(super) fn start(d: &Display, s: &mut Startup, dict: *const u32, db: *const u32) {
    // Preserve the concrete earlier provider/SMEM diagnostic on failure.
    if s._modem_smem_prepared.is_none()
        || s._linux_rmtfs
            .as_ref()
            .is_none_or(|f| f.phase() != pixel_linux_rmtfs::Phase::Prepared)
    {
        return;
    }
    if STATE
        .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        fault(s, 1);
        show(d, s);
        return;
    }
    if !healthy(s) || s._firmware_rsc.is_none() || s._modem_power.is_some() {
        fault(s, 2);
        show(d, s);
        return;
    }
    let setup = (|| {
        let v = views()?;
        let database = DatabaseSnapshot::capture(dict, db)?;
        let plan = database.plan()?;
        // SAFETY: the extended system/actual CapDL audit proves the exact
        // exclusive endpoint pages. Root serializes bit0 QMP here before
        // delegating bit14 SMP2P below, keeping all existing IPA owners live.
        let control = unsafe {
            native::Native::new(
                v.ram,
                0x0c300000,
                v.apcs,
                0x17c00000,
                65536,
                Gate { database, views: v },
            )
        }
        .map_err(|(_, e)| e)?;
        RAM.store(v.ram as usize, Ordering::Release);
        APCS.store(v.apcs as usize, Ordering::Release);
        Ok::<_, power::Error>(power::HoldOnce::new(control, GENERATION, plan))
    })();
    let power = match setup {
        Ok(p) => p,
        Err(e) => {
            fault(s, 0x100 + e.code() as u32);
            show(d, s);
            return;
        }
    };
    s._modem_power = Some(power);
    show(d, s);
    let result = s._modem_power.as_mut().unwrap().enable(
        s._firmware_rsc.as_mut().unwrap(),
        65536,
        core::hint::spin_loop,
        |r| {
            d.hex(3, "POWER PHASE", r.phase as u32);
            d.hex(10, "RAIL ATTEMPTS", r.attempted as u32);
            d.hex(11, "RAIL ACKS", r.acknowledged as u32);
            d.flush();
        },
    );
    if result.is_err() {
        fault(s, 3);
        show(d, s);
        return;
    }
    pixel_ipa_receive::poll_pending(s);
    if !healthy(s) {
        fault(s, 4);
        show(d, s);
        return;
    }
    // Verify both real holds before and after the SMP2P publication. A call
    // lease performs no SMC and never unvotes or transfers memory ownership.
    STATE.store(4, Ordering::Release);
    if verify(s).is_err() {
        fault(s, 5);
        show(d, s);
        return;
    }
    STATE.store(2, Ordering::Release);
    show(d, s);
    let v = match views() {
        Ok(v) => v,
        Err(_) => {
            fault(s, 6);
            show(d, s);
            return;
        }
    };
    let signal = match unsafe {
        signal::Native::new(
            v.apcs.add(3),
            0x17c0000c,
            signaling as fn(u64) -> Result<smp::Observation, smp::Error>,
        )
    } {
        Ok(p) => p,
        Err((_, _)) => {
            fault(s, 7);
            show(d, s);
            return;
        }
    };
    let receipt = s._modem_smem_prepared.take().unwrap();
    if s._domain
        .as_mut()
        .unwrap()
        .activate_modem_smp2p(receipt, signal)
        .is_err()
    {
        fault(s, 8);
        show(d, s);
        return;
    }
    pixel_ipa_receive::poll_pending(s);
    if !healthy(s) {
        fault(s, 9);
        show(d, s);
        return;
    }
    STATE.store(4, Ordering::Release);
    if verify(s).is_err() {
        fault(s, 10);
        show(d, s);
        return;
    }
    pixel_ipa_receive::poll_pending(s);
    if !healthy(s) {
        fault(s, 11);
        show(d, s);
        return;
    }
    STATE.store(3, Ordering::Release);
    show(d, s);
}
fn verify(s: &mut Startup) -> Result<(), power::Error> {
    let mut resources = power::Resources::new(
        s._modem_power.as_mut().unwrap(),
        &s._scm_hold,
        s._firmware_rsc.as_mut().unwrap(),
        65536,
    )?;
    let lease = resources.acquire().map_err(|_| power::Error::Lease)?;
    resources.release(lease).map_err(|_| power::Error::Lease)
}
