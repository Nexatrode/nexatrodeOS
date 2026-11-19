//! Fixed Sunfish AOP MSGRAM and doorbell adapter for standalone probes.
//!
//! These mappings do not grant IRQ, SCM, firmware, modem memory, or DMA access.

use personal_os_rust_server::qcom_qmp::{Descriptor, QMP_DESCRIPTOR_WORDS};
use personal_os_rust_server::qcom_qmp_link::ApWrite;
use personal_os_rust_server::qcom_qmp_tx::{ModemOffPacket, ModemOnPacket};

const MSGRAM_BYTES: u32 = 4096;
const SNAPSHOT_ATTEMPTS: usize = 8;
const RX_OFFSET: usize = 0xc0;
const TX_OFFSET: usize = 0x128;
const DOORBELL_WORD: usize = 0x0c / 4;
const DOORBELL_MASK: u32 = 1;

#[derive(Clone, Copy, Debug)]
pub struct Snapshot {
    pub descriptor: Descriptor,
    pub receive_length: u32,
    pub transmit_length: u32,
}

#[derive(Debug)]
pub struct ReadError {
    pub code: u32,
    pub descriptor: Descriptor,
}

pub struct Mailbox {
    msgram: *mut u32,
    doorbell: *mut u32,
}

impl Mailbox {
    /// # Safety
    /// Both pointers must map uncached, writable 4 KiB device windows for
    /// Sunfish MSGRAM (0x0c300000) and the doorbell page (0x17c00000).
    /// The caller must be the exclusive AP mailbox endpoint for this boot.
    pub unsafe fn new(msgram: *mut u32, doorbell: *mut u32) -> Self {
        Self { msgram, doorbell }
    }

    fn descriptor_words(&self) -> [u32; QMP_DESCRIPTOR_WORDS] {
        let mut words = [0u32; QMP_DESCRIPTOR_WORDS];
        for (index, word) in words.iter_mut().enumerate() {
            *word = u32::from_le(unsafe { self.msgram.add(index).read_volatile() });
        }
        device_barrier();
        words
    }

    fn lengths(&self) -> (u32, u32) {
        let receive = u32::from_le(unsafe { self.msgram.add(RX_OFFSET / 4).read_volatile() });
        let transmit = u32::from_le(unsafe { self.msgram.add(TX_OFFSET / 4).read_volatile() });
        device_barrier();
        (receive, transmit)
    }

    pub fn snapshot(&self) -> Result<Snapshot, ReadError> {
        let mut words = self.descriptor_words();
        for _ in 0..SNAPSHOT_ATTEMPTS {
            let descriptor = Descriptor::from_words(&words);
            let layout = descriptor.validate(MSGRAM_BYTES).map_err(|_| ReadError {
                code: 0x11,
                descriptor,
            })?;
            // Refuse unexpected geometry before dereferencing either length
            // word. Pin this first write-capable probe to the observed handoff.
            if layout.receive.offset() != RX_OFFSET as u32
                || layout.receive.size() != 0x68
                || layout.transmit.offset() != TX_OFFSET as u32
                || layout.transmit.size() != 0x2d8
            {
                return Err(ReadError {
                    code: 0x12,
                    descriptor,
                });
            }
            let (receive_length, transmit_length) = self.lengths();
            let next = self.descriptor_words();
            if words == next && (receive_length, transmit_length) == self.lengths() {
                return Ok(Snapshot {
                    descriptor,
                    receive_length,
                    transmit_length,
                });
            }
            words = next;
        }
        Err(ReadError {
            code: 0x10,
            descriptor: Descriptor::from_words(&words),
        })
    }

    pub fn notify(&self, writes: [Option<ApWrite>; 2]) {
        // The protocol engine can name only AP-owned descriptor words 4, 6,
        // 9, and 11. Neither mailbox length nor payload is ever written here.
        for write in writes.into_iter().flatten() {
            unsafe {
                self.msgram
                    .add(write.word_index())
                    .write_volatile(write.value().to_le());
            }
        }
        self.ring();
    }

    /// Publish only the typed, fixed OFF packet. The fresh preflight refuses
    /// an in-flight packet or disconnected channel. Full zero-padded payload
    /// precedes length; length readback precedes the ordered doorbell. A failed
    /// readback is terminal: do not erase, retry, or overwrite shared state.
    #[allow(dead_code)] // Deliberately unused by the handshake-only binary.
    pub fn publish_off(&self, packet: &ModemOffPacket) -> Result<(), u32> {
        self.publish(packet.words())
    }

    /// Separate fixed ON packet; does not perform modem power sequencing.
    #[allow(dead_code)]
    pub fn publish_on(&self, packet: &ModemOnPacket) -> Result<(), u32> {
        self.publish(packet.words())
    }

    fn publish(&self, words: &[u32; 25]) -> Result<(), u32> {
        let snapshot = self.snapshot().map_err(|error| error.code)?;
        if !snapshot.descriptor.fully_connected()
            || snapshot.receive_length != 0
            || snapshot.transmit_length != 0
        {
            return Err(0x31);
        }
        for (index, word) in words.iter().enumerate().skip(1) {
            unsafe {
                self.msgram
                    .add(TX_OFFSET / 4 + index)
                    .write_volatile(word.to_le());
            }
        }
        unsafe {
            device_barrier();
            self.msgram
                .add(TX_OFFSET / 4)
                .write_volatile(words[0].to_le());
            device_barrier();
        }
        let published = u32::from_le(unsafe { self.msgram.add(TX_OFFSET / 4).read_volatile() });
        if published != words[0] {
            return Err(0x30);
        }
        self.ring();
        Ok(())
    }

    fn ring(&self) {
        unsafe {
            device_barrier();
            self.doorbell
                .add(DOORBELL_WORD)
                .write_volatile(DOORBELL_MASK.to_le());
            device_barrier();
        }
    }
}

#[inline(always)]
fn device_barrier() {
    #[cfg(target_arch = "aarch64")]
    unsafe {
        core::arch::asm!("dsb sy", options(nostack, preserves_flags))
    };
    // Allows the actual adapter's address/write-footprint tests on host RAM.
    // These tests cannot validate ordering of real device transactions.
    #[cfg(not(target_arch = "aarch64"))]
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
}
