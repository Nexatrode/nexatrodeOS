//! Narrow volatile backend for qcom_glink_fifo; never maps or locates hardware.
//! Construct only from authorized, validated mappings in the modem owner PD.
#![deny(unsafe_op_in_unsafe_fn)]

use crate::qcom_glink_fifo::{Barrier, DESCRIPTOR_BYTES, FIFO_BYTES, Fault, Io, Observation};

pub trait Gate {
    /// Revalidate live ready/fatal state, trusted generation and exact mapped
    /// allocation/route identity. A constant value is suitable ONLY for tests.
    fn observe(&mut self) -> Result<Observation, Fault>;
    /// Stable diagnostic supplied by the trusted live gate. Zero means no
    /// detailed cause is available; nonzero values are diagnostic only and
    /// never relax the fail-closed transport gate.
    fn diagnostic(&self) -> u32 {
        0
    }
}

#[derive(Clone, Copy)]
pub struct Mappings {
    pub descriptor: *mut u32,
    pub tx: *mut u8,
    pub rx: *const u8,
    /// Exact APCS IPC doorbell register (base + 0xc), NOT the block base.
    pub doorbell: *mut u32,
}

pub struct NativeIo<G> {
    mappings: Mappings,
    gate: G,
}

fn span(start: usize, size: usize) -> Result<(usize, usize), Fault> {
    if start == 0 {
        return Err(Fault);
    }
    Ok((start, start.checked_add(size).ok_or(Fault)?))
}

impl<G: Gate> NativeIo<G> {
    /// # Safety
    /// Mappings must remain valid for this object's entire lifetime: exact
    /// 32-byte SMEM item478, 16KiB AP-TX item479, 16KiB peer-TX item480, and the
    /// authorized sunfish APCS doorbell register accepting bit12. SMEM must be
    /// uncached/coherent with the peer and support volatile byte accesses and
    /// aligned 32-bit cursor accesses. Doorbell must use a Device mapping.
    /// Regions must be physically disjoint, with no writable aliases, other AP
    /// clients, or concurrent IRQ/reset/mapping handlers. Peer may modify only
    /// its own cursors and published/free RX bytes under the GLINK ring protocol.
    /// Gate must revalidate these identities and lifecycle status on each call;
    /// it cannot derive the local generation from peer packet data. The native
    /// owner must serialize local reset and memory ownership changes, and keep
    /// mappings/pools retained after uncertainty until proven quiescence.
    pub unsafe fn new(mappings: Mappings, gate: G) -> Result<Self, Fault> {
        if mappings.descriptor as usize % 4 != 0
            || mappings.tx as usize % 8 != 0
            || mappings.rx as usize % 8 != 0
            || mappings.doorbell as usize % 4 != 0
        {
            return Err(Fault);
        }
        let spans = [
            span(mappings.descriptor as usize, DESCRIPTOR_BYTES)?,
            span(mappings.tx as usize, FIFO_BYTES)?,
            span(mappings.rx as usize, FIFO_BYTES)?,
            span(mappings.doorbell as usize, 4)?,
        ];
        for (index, &(start, end)) in spans.iter().enumerate() {
            if spans[..index]
                .iter()
                .any(|&(other, other_end)| start < other_end && other < end)
            {
                return Err(Fault);
            }
        }
        Ok(Self { mappings, gate })
    }
}

fn bounds(offset: usize, size: usize) -> Result<(), Fault> {
    if offset.checked_add(size).is_none_or(|end| end > FIFO_BYTES) {
        return Err(Fault);
    }
    Ok(())
}

impl<G: Gate> Io for NativeIo<G> {
    fn diagnostic(&self) -> u32 {
        self.gate.diagnostic()
    }
    fn observe(&mut self) -> Result<Observation, Fault> {
        self.gate.observe()
    }
    fn descriptor(&mut self, word: usize) -> Result<u32, Fault> {
        if word >= 8 {
            return Err(Fault);
        }
        // SAFETY: constructor contract retains an aligned descriptor mapping.
        Ok(u32::from_le(unsafe {
            self.mappings.descriptor.add(word).read_volatile()
        }))
    }
    fn read_rx(&mut self, offset: usize, bytes: &mut [u8]) -> Result<(), Fault> {
        bounds(offset, bytes.len())?;
        for (index, byte) in bytes.iter_mut().enumerate() {
            // SAFETY: complete access bounds checked against the retained map.
            *byte = unsafe { self.mappings.rx.add(offset + index).read_volatile() };
        }
        Ok(())
    }
    fn read_tx(&mut self, offset: usize, bytes: &mut [u8]) -> Result<(), Fault> {
        bounds(offset, bytes.len())?;
        for (index, byte) in bytes.iter_mut().enumerate() {
            // SAFETY: complete access bounds checked against the retained map.
            *byte = unsafe { self.mappings.tx.add(offset + index).read_volatile() };
        }
        Ok(())
    }
    fn write_tx(&mut self, offset: usize, bytes: &[u8]) -> Result<(), Fault> {
        bounds(offset, bytes.len())?;
        for (index, byte) in bytes.iter().enumerate() {
            // SAFETY: constructor grants exclusive AP-TX writes, within bounds.
            unsafe { self.mappings.tx.add(offset + index).write_volatile(*byte) };
        }
        Ok(())
    }
    fn store_tx_head(&mut self, value: u32) -> Result<(), Fault> {
        if value as usize >= FIFO_BYTES || value % 8 != 0 {
            return Err(Fault);
        }
        // SAFETY: word1 is the one AP-owned TX cursor, aligned and retained.
        unsafe {
            self.mappings
                .descriptor
                .add(1)
                .write_volatile(value.to_le())
        };
        Ok(())
    }
    fn store_rx_tail(&mut self, value: u32) -> Result<(), Fault> {
        if value as usize >= FIFO_BYTES || value % 8 != 0 {
            return Err(Fault);
        }
        // SAFETY: word2 is the one AP-owned RX cursor, aligned and retained.
        unsafe {
            self.mappings
                .descriptor
                .add(2)
                .write_volatile(value.to_le())
        };
        Ok(())
    }
    fn barrier(&mut self, _kind: Barrier) -> Result<(), Fault> {
        // Full system scope is conservative for this uncached SMEM/APCS port.
        // Volatile alone provides neither peer ordering nor device completion.
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
        #[cfg(target_arch = "aarch64")]
        // SAFETY: synchronization instruction touches no Rust-owned operands.
        unsafe {
            core::arch::asm!("dsb sy", options(nostack, preserves_flags));
        }
        #[cfg(not(target_arch = "aarch64"))]
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
        Ok(())
    }
    fn notify_peer(&mut self) -> Result<(), Fault> {
        // SAFETY: constructor admits only the exact APCS IPC register/bit.
        unsafe { self.mappings.doorbell.write_volatile((1u32 << 12).to_le()) };
        Ok(())
    }
}
