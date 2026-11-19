//! Read-only GLINK allocation metadata. No descriptor index/FIFO payload read,
//! allocation, publication, transport notification or IRQ operation exists here.
use crate::pixel_modem_status::Window;
use crate::pixel_smp2p_handshake::ReadyObservation;
use personal_os_rust_server::qcom_modem_status::{
    self as smem, Directory, Error, Item, Read, Status,
};
pub const IDS: [u16; 3] = [478, 479, 480];
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Snapshot {
    directory: Directory,
    items: [Option<Item>; 3],
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Report {
    pub directory: Directory,
    pub status: Status,
    pub items: [Option<Item>; 3],
    pub metadata_passes: u32,
}
impl Report {
    pub fn present_mask(&self) -> u32 {
        self.items
            .iter()
            .enumerate()
            .fold(0, |m, (i, v)| m | if v.is_some() { 1 << i } else { 0 })
    }
}
fn snapshot(m: &impl Read) -> Result<Snapshot, Error> {
    let directory = smem::directory(m)?;
    let mut items = [None; 3];
    for (i, id) in IDS.into_iter().enumerate() {
        let d = smem::allocation_directory(m, id)?;
        if d.route != directory.route
            || d.free_uncached != directory.free_uncached
            || d.free_cached != directory.free_cached
        {
            return Err(Error::RouteChanged);
        }
        items[i] = d.item;
    }
    Ok(Snapshot { directory, items })
}
/// Caller holds the directory hardware lock. The consumed observation is
/// revalidated against live AP/peer identities and ready state on both sides
/// of two exact metadata snapshots. GLINK payload addresses are only reported.
pub(crate) fn inspect(m: &impl Read, ready: ReadyObservation) -> Result<Report, Error> {
    inspect_ref(m, &ready)
}
pub(crate) fn inspect_ref(m: &impl Read, ready: &ReadyObservation) -> Result<Report, Error> {
    let (before, _) = ready.validate(m)?;
    let one = snapshot(m)?;
    let two = snapshot(m)?;
    if one != two {
        return Err(Error::Unstable);
    }
    let (after, status) = ready.validate(m)?;
    if before != one.directory || after != one.directory {
        return Err(Error::RouteChanged);
    }
    Ok(Report {
        directory: one.directory,
        status,
        items: one.items,
        metadata_passes: 2,
    })
}
pub fn read(w: &mut Window, ready: ReadyObservation) -> Result<Report, Error> {
    w.locked(|m| inspect(m, ready))
}
