//! Narrow volatile stores for the cold modem SMEM allocation extension.
#![deny(unsafe_op_in_unsafe_fn)]
use crate::sunfish_mpss_linux_smem::{Error, ITEMS, Kind, PART, Payload, Store};
use crate::sunfish_mpss_linux_smp2p::{self as smp, Field, SmpIo, Write};
use core::marker::PhantomData;

pub const FIRST_PAGE: usize = 0x11f000;
pub const PAYLOAD_PAGES: usize = 5;
pub trait Gate {
    /// Revalidate the real retained cold pre-auth generation, live mappings
    /// and IPA/domain health. Constant gates are suitable only in host tests.
    fn check(&mut self, generation: u64) -> Result<(), Error>;
}
impl Gate for fn(u64) -> Result<(), Error> {
    fn check(&mut self, generation: u64) -> Result<(), Error> {
        self(generation)
    }
}
pub struct Native<'a, G: Gate> {
    metadata: *mut u32,
    pages: *mut u32,
    gate: G,
    next_header: usize,
    next_item: usize,
    next_byte: usize,
    failed: bool,
    smp_claimed: bool,
    smp_init: usize,
    smp_version: u32,
    smp_features: u32,
    smp_negotiated: bool,
    smp_flags: u32,
    smp_ipa: u32,
    smp_power_stage: u8,
    #[cfg(feature = "sunfish-linux-mpss-glink")]
    glink_claimed: bool,
    #[cfg(feature = "sunfish-linux-mpss-glink")]
    glink_rx: Option<crate::qcom_modem_status::Item>,
    _retained: PhantomData<&'a mut [u8]>,
}
fn barrier() {
    #[cfg(target_arch = "aarch64")]
    unsafe {
        core::arch::asm!("dsb sy", options(nostack, preserves_flags));
    }
    #[cfg(not(target_arch = "aarch64"))]
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
}
impl<'a, G: Gate> Native<'a, G> {
    /// # Safety
    /// Delegate only to the same serialized owner that retains the real
    /// IPA-497 allocation and its actual TCSR lock-3 view. `smem` is the whole
    /// uncached NX physical 86000000/2 MiB CPU view; `metadata` is its sole RW
    /// page at8611d000 and `pages` is its sole RW five-page span at8611f000.
    /// These pointers are subviews of that same mapping, with no extra aliases
    /// or independent AP writers. The original IPA metadata backend may be
    /// accessed only through the extension's exclusive borrow of its parent.
    /// Retain the mappings/rights on all outcomes. Gate binds real cold host
    /// ownership before MPSS authentication, not just copied addresses/flags.
    /// Nothing may start MPSS, reset, remap or unvote during this preparation.
    /// After preparation the same serialized SMP2P Controller retains this
    /// writer. Its runtime GLINK delegation additionally requires the actual
    /// AUTH receipt and live generation/power Signals; it grants no new CPU
    /// mapping or write access to remote-owned FIFO data/cursors.
    pub unsafe fn new(
        smem: *const u8,
        smem_pa: u64,
        metadata: *mut u32,
        metadata_pa: u64,
        pages: *mut u32,
        pages_pa: u64,
        gate: G,
    ) -> Result<Self, (G, Error)> {
        let b = smem as usize;
        let m = metadata as usize;
        let p = pages as usize;
        if b == 0
            || b % 4096 != 0
            || m % 4096 != 0
            || p % 4096 != 0
            || smem_pa != 0x86000000
            || metadata_pa != 0x8611d000
            || pages_pa != 0x8611f000
            || b.checked_add(0x200000).is_none()
            || b.checked_add(PART) != Some(m)
            || b.checked_add(FIRST_PAGE) != Some(p)
        {
            return Err((gate, Error::Layout));
        }
        Ok(Self {
            metadata,
            pages,
            gate,
            next_header: 0,
            next_item: 0,
            next_byte: 0,
            failed: false,
            smp_claimed: false,
            smp_init: 0,
            smp_version: 2,
            smp_features: 1,
            smp_negotiated: false,
            smp_flags: 0,
            smp_ipa: 0,
            smp_power_stage: 0,
            #[cfg(feature = "sunfish-linux-mpss-glink")]
            glink_claimed: false,
            #[cfg(feature = "sunfish-linux-mpss-glink")]
            glink_rx: None,
            _retained: PhantomData,
        })
    }
    fn allowed(&self, r: &Store) -> bool {
        let headers = [PART + 0x2030, PART + 0x2198, PART + 0x21c8];
        let frees = [0x2198, 0x21c8, 0x61d8];
        if self.next_header < 12 {
            let item = self.next_header / 4;
            let field = self.next_header % 4;
            let (off, value, kind) = match field {
                0 => (
                    headers[item],
                    (u32::from(ITEMS[item].0) << 16) | 0xa5a5,
                    Kind::Header,
                ),
                1 => (
                    headers[item] + 4,
                    ((ITEMS[item].1.size + 7) & !7) as u32,
                    Kind::Header,
                ),
                2 => (
                    headers[item] + 8,
                    if item == 0 { 4 } else { 0 },
                    Kind::Header,
                ),
                _ => (PART + 12, frees[item], Kind::Cursor),
            };
            r.offset() == off && r.value() == value && r.kind() == kind
        } else {
            self.next_item < 3
                && r.kind() == Kind::Payload
                && r.value() == 0
                && r.offset() == ITEMS[self.next_item].1.offset + self.next_byte
        }
    }
}

#[cfg(feature = "sunfish-linux-mpss-glink")]
impl<G: Gate> Native<'_, G> {
    fn glink_live(&mut self) -> Result<(), smp::GlinkError> {
        if self.failed || !self.smp_claimed || self.smp_init != smp::INIT.len() {
            self.failed = true;
            return Err(smp::GlinkError::Consumed);
        }
        Ok(())
    }
}
#[cfg(feature = "sunfish-linux-mpss-glink")]
impl<G: Gate> smp::GlinkPayload for Native<'_, G> {
    fn claim_glink(&mut self) -> Result<(), smp::GlinkError> {
        self.glink_live()?;
        if self.glink_claimed {
            self.failed = true;
            return Err(smp::GlinkError::Consumed);
        }
        self.failed = true;
        let at = ITEMS[1].1.offset - FIRST_PAGE;
        for n in [1usize, 2, 4, 5, 6, 7] {
            if u32::from_le(unsafe { self.pages.add(at / 4 + n).read_volatile() }) != 0 {
                return Err(smp::GlinkError::Cursor);
            }
        }
        self.glink_claimed = true;
        self.failed = false;
        Ok(())
    }
    fn bind_glink_rx(&mut self, extent: &smp::GlinkRx) -> Result<(), smp::GlinkError> {
        self.glink_live()?;
        self.failed = true;
        let item = extent.item();
        if self.glink_rx.is_some_and(|old| old != item)
            || item.offset < PART + crate::sunfish_mpss_linux_smem::FINAL_FREE
            || item.size < 8
            || item.size % 8 != 0
            || item.offset % 8 != 0
            || item
                .offset
                .checked_add(item.size)
                .is_none_or(|e| e > PART + crate::sunfish_mpss_linux_smem::PART_BYTES)
        {
            return Err(smp::GlinkError::Layout);
        }
        self.glink_rx = Some(item);
        self.failed = false;
        Ok(())
    }
    fn glink_descriptor(&mut self, word: usize) -> Result<u32, smp::GlinkError> {
        self.glink_live()?;
        self.failed = true;
        if word >= 8 {
            return Err(smp::GlinkError::Layout);
        }
        let at = ITEMS[1].1.offset - FIRST_PAGE;
        barrier();
        let value = u32::from_le(unsafe { self.pages.add(at / 4 + word).read_volatile() });
        self.failed = false;
        Ok(value)
    }
    fn glink_store(&mut self, r: &smp::GlinkWrite<'_>) -> Result<(), smp::GlinkError> {
        use crate::sunfish_mpss_linux_smp2p::GlinkField as Kind;
        self.glink_live()?;
        if !self.glink_claimed {
            self.failed = true;
            return Err(smp::GlinkError::Predecessor);
        }
        self.failed = true;
        match r.kind() {
            Kind::Tx => {
                if r.bytes().is_empty()
                    || r.offset()
                        .checked_add(r.bytes().len())
                        .is_none_or(|e| e > ITEMS[2].1.size)
                {
                    return Err(smp::GlinkError::Layout);
                }
                let at = ITEMS[2].1.offset - FIRST_PAGE + r.offset();
                let p = self.pages.cast::<u8>();
                for (n, v) in r.bytes().iter().enumerate() {
                    unsafe { p.add(at + n).write_volatile(*v) }
                }
            }
            Kind::TxHead | Kind::RxTail => {
                let (word, length) = if r.kind() == Kind::TxHead {
                    (1, ITEMS[2].1.size)
                } else {
                    (2, self.glink_rx.ok_or(smp::GlinkError::NotReady)?.size)
                };
                if r.offset() != word
                    || !r.bytes().is_empty()
                    || r.value() % 8 != 0
                    || r.value() as usize >= length
                {
                    return Err(smp::GlinkError::Cursor);
                }
                let at = ITEMS[1].1.offset - FIRST_PAGE;
                unsafe {
                    self.pages
                        .add(at / 4 + word)
                        .write_volatile(r.value().to_le())
                };
            }
        }
        barrier();
        self.failed = false;
        Ok(())
    }
}
impl<G: Gate> Payload for Native<'_, G> {
    fn guard(&mut self, generation: u64) -> Result<(), Error> {
        if self.failed || self.smp_claimed {
            return Err(Error::Consumed);
        }
        self.failed = true;
        self.gate.check(generation)?;
        self.failed = false;
        Ok(())
    }
    fn store(&mut self, r: &Store) -> Result<(), Error> {
        if self.failed || self.smp_claimed || !self.allowed(r) {
            self.failed = true;
            return Err(Error::Store);
        }
        self.failed = true; // uncertain native store cannot be replayed
        let at = r.offset();
        let p = if r.kind() == Kind::Cursor {
            // The only admitted store in the old page is its free cursor.
            unsafe { self.metadata.add(3) }
        } else {
            if at < FIRST_PAGE
                || at
                    .checked_add(4)
                    .is_none_or(|e| e > FIRST_PAGE + PAYLOAD_PAGES * 4096)
                || at % 4 != 0
            {
                return Err(Error::Layout);
            }
            unsafe { self.pages.add((at - FIRST_PAGE) / 4) }
        };
        // All offsets/value/order were checked before reaching hardware.
        if self.next_header < 12 {
            self.next_header += 1;
        } else {
            self.next_byte += 4;
            if self.next_byte == ITEMS[self.next_item].1.size {
                self.next_item += 1;
                self.next_byte = 0;
            }
        }
        barrier();
        unsafe { p.write_volatile(r.value().to_le()) };
        barrier();
        if u32::from_le(unsafe { p.read_volatile() }) != r.value() {
            return Err(Error::Readback);
        }
        self.failed = false;
        Ok(())
    }
}
impl<G: Gate> SmpIo for Native<'_, G> {
    fn claim_smp2p(&mut self) -> Result<(), smp::Error> {
        if self.failed || self.smp_claimed || self.next_header != 12 || self.next_item != 3 {
            self.failed = true;
            return Err(smp::Error::Predecessor);
        }
        self.smp_claimed = true;
        Ok(())
    }
    fn smp_store(&mut self, r: &Write) -> Result<(), smp::Error> {
        if self.failed || !self.smp_claimed {
            self.failed = true;
            return Err(smp::Error::Consumed);
        }
        self.failed = true;
        let valid = if self.smp_init < smp::INIT.len() {
            r.field() == Field::Init && (r.at(), r.value()) == smp::INIT[self.smp_init]
        } else {
            match r.field() {
                Field::Version => {
                    !self.smp_negotiated
                        && self.smp_version == 2
                        && r.at() == 4
                        && r.value() == 0x101
                }
                Field::Features => {
                    !self.smp_negotiated
                        && r.at() == 4
                        && r.value() & 255 == self.smp_version
                        && r.value() >> 8 <= self.smp_features
                }
                Field::RestartAck => {
                    self.smp_negotiated
                        && self.smp_features == 1
                        && r.at() == 16
                        && (r.value() == 0 || r.value() == 2)
                        && r.value() != self.smp_flags
                }
                Field::IpaEnabled => {
                    self.smp_negotiated
                        && self.smp_power_stage == 0
                        && r.at() == 56
                        && (r.value() == 0 || r.value() == 2)
                }
                Field::IpaValid => {
                    self.smp_negotiated
                        && self.smp_power_stage == 1
                        && r.at() == 56
                        && r.value() == self.smp_ipa | 1
                }
                Field::Init => false,
            }
        };
        if !valid || r.at() % 4 != 0 || r.at().checked_add(4).is_none_or(|n| n > smp::BYTES) {
            return Err(smp::Error::Store);
        }
        if r.field() == Field::Init {
            self.smp_init += 1;
        } else {
            match r.field() {
                Field::Version => self.smp_version = 1,
                Field::Features => {
                    self.smp_features = r.value() >> 8;
                    self.smp_negotiated = true;
                }
                Field::RestartAck => self.smp_flags = r.value(),
                Field::IpaEnabled | Field::IpaValid => {
                    self.smp_ipa = r.value();
                    self.smp_power_stage += 1;
                }
                Field::Init => unreachable!(),
            }
        }
        let at = smp::AP.offset + r.at() - FIRST_PAGE;
        // Exact AP428 words only; never the master-kernel value at offset36,
        // other items, allocation cursors, reserved bytes or page neighbours.
        let p = unsafe { self.pages.add(at / 4) };
        unsafe { p.write_volatile(r.value().to_le()) };
        barrier();
        if u32::from_le(unsafe { p.read_volatile() }) != r.value() {
            return Err(smp::Error::Readback);
        }
        self.failed = false;
        Ok(())
    }
}
