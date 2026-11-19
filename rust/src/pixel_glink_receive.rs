//! One bounded, read-only descriptor/header snapshot after the sealed ready
//! gate. It neither consumes a packet nor changes any GLINK transport state.
use crate::{
    pixel_glink_inventory as inventory,
    pixel_modem_status::{Window, sync},
    pixel_smp2p_handshake::ReadyObservation,
};
use personal_os_rust_server::qcom_modem_status::{Error, Item, Read};
pub const FIFO_BYTES: usize = 0x4000;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Header {
    pub command: u16,
    pub param1: u16,
    pub param2: u32,
}
impl Header {
    pub fn version_offer(self) -> bool {
        self.command == 0 && self.param1 == 1
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Snapshot {
    words: [u32; 8],
    available: u32,
    header: Option<Header>,
}
#[derive(Clone, Copy, Debug)]
pub struct Report {
    pub inventory: inventory::Report,
    pub words: [u32; 8],
    pub available: u32,
    pub header: Option<Header>,
    pub content_passes: u32,
}
fn contents(m: &impl Read, items: &[Option<Item>; 3]) -> Result<Snapshot, Error> {
    let descriptor = items[0].ok_or(Error::ItemSize)?;
    let rx = items[2].ok_or(Error::ItemSize)?;
    if descriptor.size != 32
        || rx.size != FIFO_BYTES
        || items[1].is_some_and(|v| v.size != FIFO_BYTES)
    {
        return Err(Error::ItemSize);
    }
    if descriptor.offset % 4 != 0 || rx.offset % 8 != 0 {
        return Err(Error::Bounds);
    }
    let mut words = [0; 8];
    for (i, v) in words.iter_mut().enumerate() {
        let at = descriptor.offset + 4 * i;
        *v = u32::from_le_bytes([
            m.read8(at),
            m.read8(at + 1),
            m.read8(at + 2),
            m.read8(at + 3),
        ]);
    }
    // The fixed observed transport uses16KiB FIFOs. These are bounds checks,
    // not ownership claims; AP TX may still be absent. Reserved words are
    // compared verbatim and never interpreted or modified.
    if words[..4]
        .iter()
        .any(|v| *v as usize >= FIFO_BYTES || v % 8 != 0)
    {
        return Err(Error::Bounds);
    }
    let tail = words[2] as usize;
    let head = words[3] as usize;
    let available = (head + FIFO_BYTES - tail) % FIFO_BYTES;
    sync(); // Observe the remote-published head before its referenced bytes.
    let header = if available >= 8 {
        let mut bytes = [0; 8];
        for (i, v) in bytes.iter_mut().enumerate() {
            *v = m.read8(rx.offset + (tail + i) % FIFO_BYTES);
        }
        Some(Header {
            command: u16::from_le_bytes(bytes[..2].try_into().unwrap()),
            param1: u16::from_le_bytes(bytes[2..4].try_into().unwrap()),
            param2: u32::from_le_bytes(bytes[4..8].try_into().unwrap()),
        })
    } else {
        None
    };
    Ok(Snapshot {
        words,
        available: available as u32,
        header,
    })
}
/// Hold lock3 for directory consistency; it does not serialize FIFO writers.
/// Exact descriptor/header repetitions and ready/identity rechecks establish
/// only a point-in-time readout, never a persistent ownership/epoch capability.
pub(crate) fn inspect(m: &impl Read, ready: ReadyObservation) -> Result<Report, Error> {
    inspect_ref(m, &ready)
}
pub(crate) fn inspect_ref(m: &impl Read, ready: &ReadyObservation) -> Result<Report, Error> {
    let before = inventory::inspect_ref(m, ready)?;
    let one = contents(m, &before.items)?;
    sync();
    let two = contents(m, &before.items)?;
    if one != two {
        return Err(Error::Unstable);
    }
    sync();
    let after = inventory::inspect_ref(m, ready)?;
    if before != after {
        return Err(Error::RouteChanged);
    }
    Ok(Report {
        inventory: after,
        words: one.words,
        available: one.available,
        header: one.header,
        content_passes: 2,
    })
}
pub fn read(w: &mut Window, ready: ReadyObservation) -> Result<Report, Error> {
    w.locked(|m| inspect(m, ready))
}
