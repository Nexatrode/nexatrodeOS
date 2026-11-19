//! One-shot AP status publication, not a GLINK or modem recovery service.
//! Only the fixed first host0:1 partition page may be written. An item is
//! allocated under lock3 after full staging/INFO gates and before AUTH. Only
//! notification and reads follow AUTH; the post-start free cursor is not used
//! to choose a writable page.
use crate::pixel_modem_status::{Baseline, Clock, Report as StatusReport, Window, sync};
use personal_os_rust_server::qcom_modem_status::{
    self as smem, Directory, Error, Item, Read, Route, Status,
};
use personal_os_rust_server::qcom_pas_auth::{AuthOnce, StatusPermit};
const PART: usize = 0x11d000;
const PART_SIZE: usize = 0x7c000;
const AP_ID: u16 = 428;
const BLOCK: usize = 16 + 344;

pub struct Handshake {
    page: *mut u8,
    doorbell: *mut u32,
    generation: u64,
    used: bool,
    prepared: bool,
    ap: Option<Item>,
    last: Option<Directory>,
    ready: Option<ReadyObservation>,
}
/// One-use evidence for a read-only directory snapshot in this diagnostic.
/// This is not a production lifecycle/GLINK-write or DMA capability.
pub struct ReadyObservation {
    generation: u64,
    route: Route,
    ap: Item,
    peer: Item,
}
impl ReadyObservation {
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) fn validate(&self, m: &impl Read) -> Result<(Directory, Status), Error> {
        let d = smem::directory(m)?;
        if d.route != self.route || d.item != Some(self.peer) {
            return Err(Error::RouteChanged);
        }
        let a = smem::directory_for(m, AP_ID)?;
        if a.route != d.route
            || a.free_uncached != d.free_uncached
            || a.free_cached != d.free_cached
            || a.item != Some(self.ap)
            || !Handshake::exact(m, self.ap.offset, &ap_payload())
        {
            return Err(Error::Outbound);
        }
        let s = smem::payload(m, self.peer)?;
        if s.fatal_bit() {
            return Err(Error::Fatal);
        }
        if !s.ready_bit() || s.features != 0 {
            return Err(Error::ReadyLost);
        }
        Ok((d, s))
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Report {
    pub status: StatusReport,
    pub ap_offset: u32,
    pub published: bool,
    pub doorbells: u32,
    pub peer_negotiated: bool,
}
pub fn check_initial(w: &mut Window, b: &Baseline) -> Result<(), Error> {
    check_initial_shape(b)?;
    w.locked(|m| {
        let d = smem::directory_for(m, AP_ID)?;
        if d.route != b.route
            || d.free_uncached != b.uncached
            || d.free_cached != b.cached
            || d.item.is_some()
        {
            return Err(Error::Outbound);
        }
        Ok(())
    })
}
fn ap_payload() -> [u8; 340] {
    let mut b = [0; 340];
    b[..4].copy_from_slice(&0x504d5324u32.to_le_bytes());
    // Feature0 disables optional SSR ACK; this diagnostic cannot recover peers.
    b[4] = 1;
    b[10..12].copy_from_slice(&1u16.to_le_bytes());
    b[12..14].copy_from_slice(&16u16.to_le_bytes());
    b[14..16].copy_from_slice(&1u16.to_le_bytes());
    b[20..33].copy_from_slice(b"master-kernel");
    b
}
impl Handshake {
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// # Safety
    /// page is exclusively the uncached RW page at0x8611d000. doorbell is the
    /// existing uncached APCS page at0x17c00000. No aliases/other client/IRQ.
    pub unsafe fn new(page: *mut u8, doorbell: *mut u32) -> Self {
        // Legacy probes use the first runtime generation.
        unsafe { Self::new_for_generation_unchecked(page, doorbell, 1) }
    }

    /// Construct the handshake bound to one post-init/runtime generation.
    ///
    /// # Safety
    /// `page` and `doorbell` have the same exclusive mapping requirements as
    /// [`Handshake::new`]. The caller must use the same generation for the
    /// IPA post-init proof and the eventual native owner attachment.
    pub unsafe fn new_for_generation(
        page: *mut u8,
        doorbell: *mut u32,
        generation: u64,
    ) -> Result<Self, Error> {
        if generation == 0 {
            return Err(Error::Outbound);
        }
        // SAFETY: forwarded unchanged from this function's caller contract.
        Ok(unsafe { Self::new_for_generation_unchecked(page, doorbell, generation) })
    }

    unsafe fn new_for_generation_unchecked(
        page: *mut u8,
        doorbell: *mut u32,
        generation: u64,
    ) -> Self {
        Self {
            page,
            doorbell,
            generation,
            used: false,
            prepared: false,
            ap: None,
            last: None,
            ready: None,
        }
    }
    pub fn take_ready_observation(&mut self) -> Result<ReadyObservation, Error> {
        self.ready.take().ok_or(Error::ReadyLost)
    }
    pub fn last_directory(&self) -> Option<Directory> {
        self.last
    }
    pub fn ap_offset(&self) -> u32 {
        self.ap.map_or(0, |i| i.offset as u32)
    }
    pub fn prepared(&self) -> bool {
        self.prepared
    }
    pub(crate) fn preauth_directory(&self, m: &impl Read) -> Result<Directory, Error> {
        if !self.prepared || self.used || self.ready.is_some() {
            return Err(Error::Outbound);
        }
        let d = smem::directory(m)?;
        if d.item.is_some() {
            return Err(Error::BaselinePresent);
        }
        if d.route.offset != PART
            || d.route.size != PART_SIZE
            || d.route.cacheline != 32
            || d.free_cached != PART_SIZE
        {
            return Err(Error::RouteChanged);
        }
        self.verify_ap(m, d)?;
        Ok(d)
    }
    /// Consume the sealed staged-image/INFO/fresh-metadata permit once. The
    /// private partition must still be exactly empty under lock at publication.
    pub fn prepare(
        &mut self,
        w: &mut Window,
        b: &Baseline,
        _permit: StatusPermit,
    ) -> Result<(), Error> {
        if self.used || self.ap.is_some() {
            return Err(Error::Outbound);
        }
        self.used = true;
        check_initial_shape(b)?;
        w.locked(|m| {
            let d = smem::directory(m)?;
            self.last = Some(d);
            if d.route != b.route
                || d.free_uncached != b.uncached
                || d.free_cached != b.cached
                || d.item.is_some()
            {
                return Err(Error::BaselinePresent);
            }
            self.publish(m, d)?;
            let committed = smem::directory(m)?;
            self.last = Some(committed);
            self.verify_ap(m, committed)?;
            Ok(())
        })?;
        self.prepared = true;
        self.used = false;
        Ok(())
    }
    unsafe fn write8(&self, at: usize, v: u8) {
        assert!((PART..PART + 4096).contains(&at));
        unsafe { self.page.add(at - PART).write_volatile(v) }
    }
    unsafe fn write32(&self, at: usize, v: u32) {
        assert!(at % 4 == 0 && at >= PART && at + 4 <= PART + 4096);
        unsafe {
            self.page
                .add(at - PART)
                .cast::<u32>()
                .write_volatile(v.to_le())
        }
    }
    fn exact(m: &impl Read, at: usize, b: &[u8]) -> bool {
        b.iter().enumerate().all(|(i, v)| m.read8(at + i) == *v)
    }
    fn verify_ap(&self, m: &impl Read, d: Directory) -> Result<(), Error> {
        let ap = smem::directory_for(m, AP_ID)?;
        if ap.route != d.route
            || ap.free_uncached != d.free_uncached
            || ap.free_cached != d.free_cached
            || ap.item != self.ap
        {
            return Err(Error::Outbound);
        }
        let item = self.ap.ok_or(Error::Outbound)?;
        if !Self::exact(m, item.offset, &ap_payload()) {
            return Err(Error::Outbound);
        }
        Ok(())
    }
    fn publish(&mut self, m: &impl Read, d: Directory) -> Result<(), Error> {
        let existing = smem::directory_for(m, AP_ID)?;
        if self.ap.is_some()
            || existing.item.is_some()
            || existing != (Directory { item: None, ..d })
        {
            return Err(Error::Outbound);
        }
        let cursor = d.free_uncached;
        if d.route.offset != PART
            || d.route.size != PART_SIZE
            || d.route.cacheline != 32
            || cursor % 8 != 0
            || cursor < 32
            || cursor + BLOCK > 4096
            || cursor + BLOCK >= d.free_cached
        {
            return Err(Error::Bounds);
        }
        let header = PART + cursor;
        let item = Item {
            offset: header + 16,
            size: 340,
        };
        self.ap = Some(item); // Any partial write is terminal; never reissue.
        let mut block = [0; BLOCK];
        block[..2].copy_from_slice(&0xa5a5u16.to_le_bytes());
        block[2..4].copy_from_slice(&AP_ID.to_le_bytes());
        block[4..8].copy_from_slice(&344u32.to_le_bytes());
        block[8..10].copy_from_slice(&4u16.to_le_bytes());
        block[16..356].copy_from_slice(&ap_payload());
        block[20] = 0; // Version is the final payload publication store.
        for (i, v) in block.iter().enumerate() {
            unsafe { self.write8(header + i, *v) }
        }
        sync();
        if !Self::exact(m, header, &block) {
            return Err(Error::Publication);
        }
        // Publish only the directory free cursor, preserving every other field.
        unsafe { self.write32(PART + 12, (cursor + BLOCK) as u32) }
        sync();
        let committed = smem::directory_for(m, AP_ID)?;
        if committed.item != Some(item)
            || committed.free_uncached != cursor + BLOCK
            || committed.free_cached != d.free_cached
            || committed.route != d.route
        {
            return Err(Error::Publication);
        }
        unsafe { self.write8(item.offset + 4, 1) }
        sync();
        if !Self::exact(m, item.offset, &ap_payload()) {
            return Err(Error::Publication);
        }
        Ok(())
    }
    fn kick(&self) {
        sync();
        unsafe { self.doorbell.add(3).write_volatile((1u32 << 14).to_le()) }
        sync();
    }
    pub fn observe(
        &mut self,
        w: &mut Window,
        b: Baseline,
        auth: AuthOnce,
        clock: &mut impl Clock,
        mut progress: impl FnMut(&Report),
    ) -> Report {
        let mut r = Report {
            status: StatusReport {
                error: None,
                samples: 0,
                directory: None,
                status: None,
                ready_bit_seen: false,
                auth_passed: false,
                elapsed_ticks: 0,
                frequency: clock.frequency(),
            },
            ap_offset: self.ap_offset(),
            published: self.prepared,
            doorbells: 0,
            peer_negotiated: false,
        };
        if self.used || !self.prepared {
            r.status.error = Some(Error::Outbound);
            return r;
        }
        self.used = true;
        if auth.into_status_returned().is_err() {
            r.status.error = Some(Error::Auth);
            return r;
        }
        r.status.auth_passed = true;
        if check_initial_shape(&b).is_err() {
            r.status.error = Some(Error::Outbound);
            return r;
        }
        if !(1_000_000..=1_000_000_000).contains(&r.status.frequency) {
            r.status.error = Some(Error::Clock);
            return r;
        }
        let start = clock.now();
        let mut peer: Option<Item> = None;
        for poll in 0..1024 {
            r.status.samples = poll + 1;
            let sampled = w.locked(|m| {
                let d = smem::directory(m)?;
                self.last = Some(d);
                r.status.directory = Some(d);
                if d.route != b.route
                    || d.free_uncached < b.uncached
                    || d.free_cached > b.cached
                    || peer.is_some_and(|item| d.item != Some(item))
                {
                    return Err(Error::RouteChanged);
                }
                let s = if let Some(item) = d.item {
                    Some(smem::payload(m, item)?)
                } else {
                    None
                };
                r.status.status = s;
                if s.is_some_and(|s| s.fatal_bit()) {
                    return Err(Error::Fatal);
                }
                self.verify_ap(m, d)?;
                Ok((d, s, s.is_some() && r.doorbells == 0))
            });
            match sampled {
                Err(e) => {
                    r.status.error = Some(e);
                    break;
                }
                Ok((d, s, kick)) => {
                    r.status.directory = Some(d);
                    r.status.status = s;
                    if d.item.is_some() {
                        peer = d.item;
                    }
                    if kick {
                        self.kick();
                        r.doorbells += 1;
                        r.published = true;
                        r.ap_offset = self.ap.unwrap().offset as u32;
                    }
                    r.peer_negotiated = r.published && s.is_some_and(|s| s.features == 0);
                    if r.peer_negotiated && s.is_some_and(|s| s.ready_bit()) {
                        r.status.ready_bit_seen = true;
                        self.ready = Some(ReadyObservation {
                            generation: self.generation,
                            route: d.route,
                            ap: self.ap.unwrap(),
                            peer: d.item.unwrap(),
                        });
                        break;
                    }
                }
            }
            r.status.elapsed_ticks = clock.now().wrapping_sub(start);
            if r.status.elapsed_ticks >= r.status.frequency * 10 {
                r.status.error = Some(Error::Timeout);
                break;
            }
            if poll % 100 == 0 {
                progress(&r)
            }
            let wait = clock.now();
            let mut advanced = false;
            for _ in 0..1_000_000 {
                if clock.now().wrapping_sub(wait) >= r.status.frequency / 100 {
                    advanced = true;
                    break;
                }
                core::hint::spin_loop();
            }
            if !advanced {
                r.status.error = Some(Error::Clock);
                break;
            }
        }
        if !r.status.ready_bit_seen && r.status.error.is_none() {
            r.status.error = Some(Error::Timeout)
        }
        r.status.elapsed_ticks = clock.now().wrapping_sub(start);
        r
    }
}
fn check_initial_shape(b: &Baseline) -> Result<(), Error> {
    if b.route.offset != PART
        || b.route.size != PART_SIZE
        || b.route.cacheline != 32
        || b.uncached != 32
        || b.cached != PART_SIZE
    {
        Err(Error::Outbound)
    } else {
        Ok(())
    }
}
