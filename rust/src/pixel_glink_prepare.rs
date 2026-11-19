//! Standalone pre-AUTH memory preparation. Never start firmware or a GLINK
//! transport. Allocate only fresh478/479 after the exact AP-only baseline;
//! any partial failure retains all memory until a full platform restart.
use crate::{
    pixel_modem_status::{Window, sync},
    pixel_smp2p_handshake::Handshake,
};
use personal_os_rust_server::{
    qcom_modem_status::{self as smem, Directory, Error, Item, Read},
    qcom_pas_auth::GlinkMemoryPermit,
};
pub const PART: usize = 0x11d000;
pub const WINDOW: usize = 0x5000;
pub const START: usize = 0x188;
pub const DESCRIPTOR: Item = Item {
    offset: PART + START + 16,
    size: 32,
};
pub const TX: Item = Item {
    offset: PART + START + 48 + 16,
    size: 0x4000,
};
pub const END: usize = TX.offset + TX.size - PART;
#[derive(Clone, Copy, Debug)]
pub struct Report {
    pub descriptor: Item,
    pub tx: Item,
    pub free_before: u32,
    pub free_after: u32,
    pub written: u32,
    pub guard_bytes: u32,
    pub verification_passes: u32,
}
pub(crate) trait Write {
    fn byte(&mut self, at: usize, value: u8);
    fn cursor(&mut self, value: u32);
}
fn expected(at: usize) -> u8 {
    let (base, id, size) = if at < START + 48 {
        (START, 478u16, 32u32)
    } else {
        (START + 48, 479u16, 0x4000u32)
    };
    match at - base {
        0 => 0xa5,
        1 => 0xa5,
        2 => id.to_le_bytes()[0],
        3 => id.to_le_bytes()[1],
        n @ 4..=7 => size.to_le_bytes()[n - 4],
        _ => 0,
    }
}
fn allocations(m: &impl Read, before: Directory, committed: bool) -> Result<(), Error> {
    for (id, item) in [(478, DESCRIPTOR), (479, TX)] {
        let d = smem::allocation_directory(m, id)?;
        if d.route != before.route
            || d.free_cached != before.free_cached
            || d.free_uncached != if committed { END } else { START }
            || d.item != if committed { Some(item) } else { None }
        {
            return Err(Error::Publication);
        }
    }
    if smem::allocation_directory(m, 480)?.item.is_some() {
        return Err(Error::BaselinePresent);
    }
    Ok(())
}
pub(crate) fn stage(
    m: &impl Read,
    io: &mut impl Write,
    h: &Handshake,
    _permit: GlinkMemoryPermit,
) -> Result<Report, Error> {
    let before = h.preauth_directory(m)?;
    if before.free_uncached != START {
        return Err(Error::Bounds);
    }
    allocations(m, before, false)?;
    // Snapshot only the explicitly admitted five pages, and later compare
    // every byte outside the fresh allocations and free-cursor publication.
    let mut guards = [0; WINDOW];
    for (i, v) in guards.iter_mut().enumerate() {
        *v = m.read8(PART + i);
    }
    for at in START..END {
        io.byte(PART + at, expected(at));
    }
    sync();
    for _ in 0..2 {
        for at in START..END {
            if m.read8(PART + at) != expected(at) {
                return Err(Error::Publication);
            }
        }
        sync();
    }
    // Both initialized allocation headers/data precede one aligned cursor
    // commit. Do not mutate existing descriptors or reset existing indices.
    io.cursor(END as u32);
    sync();
    let after = h.preauth_directory(m)?;
    if after.route != before.route
        || after.free_uncached != END
        || after.free_cached != before.free_cached
    {
        return Err(Error::Publication);
    }
    allocations(m, before, true)?;
    let mut guard_bytes = 0;
    for (i, v) in guards.iter().enumerate() {
        if !(12..16).contains(&i) && !(START..END).contains(&i) {
            guard_bytes += 1;
            if m.read8(PART + i) != *v {
                return Err(Error::Publication);
            }
        }
    }
    for at in START..END {
        if m.read8(PART + at) != expected(at) {
            return Err(Error::Publication);
        }
    }
    sync();
    Ok(Report {
        descriptor: DESCRIPTOR,
        tx: TX,
        free_before: START as u32,
        free_after: END as u32,
        written: (END - START + 4) as u32,
        guard_bytes,
        verification_passes: 3,
    })
}
/// Read-only last check before the private boot wrapper's one AUTH call.
pub(crate) fn revalidate(m: &impl Read, h: &Handshake) -> Result<(), Error> {
    let d = h.preauth_directory(m)?;
    if d.free_uncached != END {
        return Err(Error::Publication);
    }
    allocations(m, d, true)?;
    for at in START..END {
        if m.read8(PART + at) != expected(at) {
            return Err(Error::Publication);
        }
    }
    Ok(())
}
struct NativeWrites {
    ap: *mut u8,
    tail: *mut u8,
}
impl Write for NativeWrites {
    fn byte(&mut self, at: usize, value: u8) {
        assert!((PART + START..PART + END).contains(&at));
        let offset = at - PART;
        let p = if offset < 4096 {
            unsafe { self.ap.add(offset) }
        } else {
            unsafe { self.tail.add(offset - 4096) }
        };
        unsafe { p.write_volatile(value) }
    }
    fn cursor(&mut self, value: u32) {
        assert_eq!(value, END as u32);
        unsafe { self.ap.add(12).cast::<u32>().write_volatile(value.to_le()) }
    }
}
pub struct Preparation {
    writes: NativeWrites,
    used: bool,
}
impl Preparation {
    /// # Safety
    /// ap is exclusively uncached RW physical0x8611d000..0x8611e000; tail is
    /// uncached RW0x8611e000..0x86122000. No aliases, IRQ, other AP client or
    /// authenticated modem is admitted. This instance and its consumed permit
    /// authorize this one fixed preparation only, never packet publication.
    pub unsafe fn new(ap: *mut u8, tail: *mut u8) -> Self {
        Self {
            writes: NativeWrites { ap, tail },
            used: false,
        }
    }
    pub fn run(
        &mut self,
        w: &mut Window,
        h: &Handshake,
        permit: GlinkMemoryPermit,
    ) -> Result<Report, Error> {
        if self.used {
            return Err(Error::Outbound);
        }
        self.used = true;
        w.locked(|m| stage(m, &mut self.writes, h, permit))
    }
}
