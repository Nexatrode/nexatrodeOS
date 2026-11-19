//! Bounded UFSHCI interrupt dispatch for the Sunfish owner.
//!
//! One Microkit notification causes at most one UFSHCI status snapshot. The
//! controller status is read and write-one-to-clear acknowledged by
//! `Host::handle_interrupt` before the kernel IRQ capability is acknowledged.
//! A failed MMIO snapshot leaves the kernel IRQ masked and returns an error.
#![forbid(unsafe_code)]

use crate::qcom_ufs_host::{self as host, Interrupt, RegisterIo};

/// Candidate UFS profile route: GIC SPI 265 is Microkit IRQ number 297.
pub const UFS_MICROKIT_IRQ_NUMBER: usize = 297;
/// Channel ID declared by the candidate system description.
pub const UFS_IRQ_CHANNEL_ID: usize = 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    WrongChannel,
    Host(host::Error),
    KernelAcknowledge,
}

pub trait KernelAcknowledger {
    /// Re-arm only the UFS capability. Implementations must reject other IDs.
    fn acknowledge(&mut self, channel_id: usize) -> Result<(), ()>;
}

/// Handle one UFS interrupt notification and return its bounded status
/// snapshot. This function never polls or drains a second snapshot. On MMIO
/// failure it deliberately does not acknowledge the kernel IRQ, keeping the
/// route masked while the caller quarantines the owner.
pub fn dispatch_one<I: RegisterIo, A: KernelAcknowledger>(
    channel_id: usize,
    host: &mut host::Host,
    io: &mut I,
    ack: &mut A,
) -> Result<Interrupt, Error> {
    if channel_id != UFS_IRQ_CHANNEL_ID {
        return Err(Error::WrongChannel);
    }

    let snapshot = host.handle_interrupt(io).map_err(Error::Host)?;
    if snapshot.errors != 0 {
        // Keep a fatal controller condition from immediately reasserting while
        // the owner is quarantined. If masking fails, leave the kernel IRQ
        // masked as well and retain the controller state for diagnosis.
        io.write32(host::REG_INTERRUPT_ENABLE, 0)
            .map_err(Error::Host)?;
    }
    ack.acknowledge(UFS_IRQ_CHANNEL_ID)
        .map_err(|_| Error::KernelAcknowledge)?;
    Ok(snapshot)
}
