//! Minimal VirtIO MMIO entropy-device driver for QEMU's modern transport.
//!
//! The driver keeps one writable descriptor outstanding and polls its used
//! ring. It is intentionally small: no features are negotiated, notifications
//! are suppressed, and callers receive bytes only after checking the device's
//! reported completion length.

use core::{
    hint::spin_loop,
    ptr::{self, NonNull},
    sync::atomic::{Ordering, fence},
};

const REG_MAGIC: usize = 0x000;
const REG_VERSION: usize = 0x004;
const REG_DEVICE_ID: usize = 0x008;
const REG_DEVICE_FEATURES: usize = 0x010;
const REG_DEVICE_FEATURES_SEL: usize = 0x014;
const REG_DRIVER_FEATURES: usize = 0x020;
const REG_DRIVER_FEATURES_SEL: usize = 0x024;
const REG_QUEUE_SEL: usize = 0x030;
const REG_QUEUE_NUM_MAX: usize = 0x034;
const REG_QUEUE_NUM: usize = 0x038;
const REG_QUEUE_READY: usize = 0x044;
const REG_QUEUE_NOTIFY: usize = 0x050;
const REG_INTERRUPT_STATUS: usize = 0x060;
const REG_INTERRUPT_ACK: usize = 0x064;
const REG_STATUS: usize = 0x070;
const REG_QUEUE_DESC_LOW: usize = 0x080;
const REG_QUEUE_DRIVER_LOW: usize = 0x090;
const REG_QUEUE_DEVICE_LOW: usize = 0x0a0;

const VIRTIO_MAGIC: u32 = 0x7472_6976;
const VIRTIO_VERSION_MODERN: u32 = 2;
const VIRTIO_DEVICE_ENTROPY: u32 = 4;
const STATUS_ACKNOWLEDGE: u32 = 1;
const STATUS_DRIVER: u32 = 2;
const STATUS_FEATURES_OK: u32 = 8;
const STATUS_DRIVER_OK: u32 = 4;
const QUEUE_SIZE: u16 = 8;
const QUEUE_DESC_OFFSET: usize = 0x000;
const QUEUE_AVAIL_OFFSET: usize = 0x100;
const QUEUE_USED_OFFSET: usize = 0x200;
const DMA_PAGE_SIZE: usize = 4096;
const MAX_POLL_ITERATIONS: usize = 50_000_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VirtioRngError {
    WrongDevice,
    QueueUnavailable,
    FeatureNegotiationFailed,
    QueueSetupFailed,
    Timeout,
    InvalidCompletion,
}

#[repr(C)]
struct Descriptor {
    address: u64,
    length: u32,
    flags: u16,
    next: u16,
}

pub struct VirtioRng {
    registers: NonNull<u32>,
    dma: NonNull<u8>,
    dma_physical: u64,
    available_index: u16,
    used_index: u16,
}

impl VirtioRng {
    /// `registers` points at this device's 0x200-byte window, and `dma`
    /// points at two physically contiguous, uncached pages owned by this PD.
    /// `dma_physical` is the physical address of the first DMA page.
    pub unsafe fn initialize(
        registers: NonNull<u32>,
        dma: NonNull<u8>,
        dma_physical: u64,
    ) -> Result<Self, VirtioRngError> {
        if dma_physical as usize & (DMA_PAGE_SIZE - 1) != 0 {
            return Err(VirtioRngError::QueueSetupFailed);
        }

        let mut rng = Self {
            registers,
            dma,
            dma_physical,
            available_index: 0,
            used_index: 0,
        };

        if rng.read(REG_MAGIC) != VIRTIO_MAGIC
            || rng.read(REG_VERSION) != VIRTIO_VERSION_MODERN
            || rng.read(REG_DEVICE_ID) != VIRTIO_DEVICE_ENTROPY
        {
            return Err(VirtioRngError::WrongDevice);
        }

        rng.write(REG_STATUS, 0);
        for _ in 0..MAX_POLL_ITERATIONS {
            if rng.read(REG_STATUS) == 0 {
                break;
            }
            spin_loop();
        }
        if rng.read(REG_STATUS) != 0 {
            return Err(VirtioRngError::QueueSetupFailed);
        }

        rng.write(REG_STATUS, STATUS_ACKNOWLEDGE);
        rng.write(REG_STATUS, STATUS_ACKNOWLEDGE | STATUS_DRIVER);

        // VirtIO entropy devices currently define no feature bits.
        rng.write(REG_DEVICE_FEATURES_SEL, 0);
        let _features_low = rng.read(REG_DEVICE_FEATURES);
        rng.write(REG_DEVICE_FEATURES_SEL, 1);
        let _features_high = rng.read(REG_DEVICE_FEATURES);
        rng.write(REG_DRIVER_FEATURES_SEL, 0);
        rng.write(REG_DRIVER_FEATURES, 0);
        rng.write(REG_DRIVER_FEATURES_SEL, 1);
        rng.write(REG_DRIVER_FEATURES, 0);
        rng.write(
            REG_STATUS,
            STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_FEATURES_OK,
        );
        if rng.read(REG_STATUS) & STATUS_FEATURES_OK == 0 {
            return Err(VirtioRngError::FeatureNegotiationFailed);
        }

        rng.write(REG_QUEUE_SEL, 0);
        if rng.read(REG_QUEUE_NUM_MAX) < u32::from(QUEUE_SIZE) {
            return Err(VirtioRngError::QueueUnavailable);
        }
        if rng.read(REG_QUEUE_READY) != 0 {
            return Err(VirtioRngError::QueueSetupFailed);
        }

        unsafe { rng.clear_queue_memory() };
        let descriptor_address = dma_physical + QUEUE_DESC_OFFSET as u64;
        let available_address = dma_physical + QUEUE_AVAIL_OFFSET as u64;
        let used_address = dma_physical + QUEUE_USED_OFFSET as u64;
        rng.write(REG_QUEUE_NUM, u32::from(QUEUE_SIZE));
        rng.write_address(REG_QUEUE_DESC_LOW, descriptor_address);
        rng.write_address(REG_QUEUE_DRIVER_LOW, available_address);
        rng.write_address(REG_QUEUE_DEVICE_LOW, used_address);
        rng.write(REG_QUEUE_READY, 1);
        fence(Ordering::SeqCst);
        if rng.read(REG_QUEUE_READY) != 1 {
            return Err(VirtioRngError::QueueSetupFailed);
        }

        rng.write(
            REG_STATUS,
            STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_FEATURES_OK | STATUS_DRIVER_OK,
        );
        if rng.read(REG_STATUS) & STATUS_DRIVER_OK == 0 {
            return Err(VirtioRngError::QueueSetupFailed);
        }
        Ok(rng)
    }

    pub fn fill(&mut self, output: &mut [u8]) -> Result<(), VirtioRngError> {
        let mut completed = 0usize;
        while completed < output.len() {
            let request_length = (output.len() - completed).min(DMA_PAGE_SIZE);
            self.post_buffer(0, request_length)?;
            let actual_length = self.wait_for_completion(request_length)?;
            unsafe {
                for index in 0..actual_length {
                    output[completed + index] =
                        ptr::read_volatile(self.dma.as_ptr().add(DMA_PAGE_SIZE + index));
                }
            }
            completed += actual_length;
        }
        Ok(())
    }

    fn post_buffer(&mut self, output_offset: usize, length: usize) -> Result<(), VirtioRngError> {
        if length == 0 || length > u32::MAX as usize || output_offset + length > DMA_PAGE_SIZE {
            return Err(VirtioRngError::InvalidCompletion);
        }
        let descriptor = unsafe {
            self.dma
                .as_ptr()
                .add(QUEUE_DESC_OFFSET)
                .cast::<Descriptor>()
        };
        let descriptor_address = self.dma_physical + DMA_PAGE_SIZE as u64 + output_offset as u64;
        unsafe {
            ptr::write_volatile(ptr::addr_of_mut!((*descriptor).address), descriptor_address);
            ptr::write_volatile(ptr::addr_of_mut!((*descriptor).length), length as u32);
            ptr::write_volatile(ptr::addr_of_mut!((*descriptor).flags), 2); // Device-writable.
            ptr::write_volatile(ptr::addr_of_mut!((*descriptor).next), 0);
            // VIRTQ_AVAIL_F_NO_INTERRUPT; this service polls the used ring.
            ptr::write_volatile(self.dma.as_ptr().add(QUEUE_AVAIL_OFFSET).cast::<u16>(), 1);
            let ring_slot = usize::from(self.available_index % QUEUE_SIZE);
            ptr::write_volatile(
                self.dma
                    .as_ptr()
                    .add(QUEUE_AVAIL_OFFSET + 4 + ring_slot * 2)
                    .cast::<u16>(),
                0,
            );
            fence(Ordering::Release);
            self.available_index = self.available_index.wrapping_add(1);
            ptr::write_volatile(
                self.dma.as_ptr().add(QUEUE_AVAIL_OFFSET + 2).cast::<u16>(),
                self.available_index,
            );
        }
        fence(Ordering::SeqCst);
        self.write(REG_QUEUE_NOTIFY, 0);
        Ok(())
    }

    fn wait_for_completion(&mut self, requested: usize) -> Result<usize, VirtioRngError> {
        let used_index_pointer =
            unsafe { self.dma.as_ptr().add(QUEUE_USED_OFFSET + 2).cast::<u16>() };
        let next_index = self.used_index.wrapping_add(1);
        let mut observed = self.used_index;
        for _ in 0..MAX_POLL_ITERATIONS {
            observed = unsafe { ptr::read_volatile(used_index_pointer) };
            fence(Ordering::Acquire);
            if observed != self.used_index {
                break;
            }
            spin_loop();
        }
        if observed != next_index {
            return Err(VirtioRngError::Timeout);
        }

        let used_slot = usize::from(self.used_index % QUEUE_SIZE);
        let used_entry = unsafe { self.dma.as_ptr().add(QUEUE_USED_OFFSET + 4 + used_slot * 8) };
        fence(Ordering::Acquire);
        let descriptor_id = unsafe { ptr::read_volatile(used_entry.cast::<u32>()) };
        let actual_length = unsafe { ptr::read_volatile(used_entry.add(4).cast::<u32>()) };
        self.used_index = observed;

        if descriptor_id != 0 || actual_length == 0 || actual_length as usize > requested {
            return Err(VirtioRngError::InvalidCompletion);
        }
        let pending_interrupt = self.read(REG_INTERRUPT_STATUS);
        if pending_interrupt != 0 {
            self.write(REG_INTERRUPT_ACK, pending_interrupt);
        }
        Ok(actual_length as usize)
    }

    unsafe fn clear_queue_memory(&mut self) {
        for offset in 0..(2 * DMA_PAGE_SIZE) {
            unsafe { ptr::write_volatile(self.dma.as_ptr().add(offset), 0) };
        }
    }

    fn read(&self, offset: usize) -> u32 {
        unsafe { ptr::read_volatile(self.registers.as_ptr().add(offset / 4)) }
    }

    fn write(&mut self, offset: usize, value: u32) {
        unsafe { ptr::write_volatile(self.registers.as_ptr().add(offset / 4), value) };
        fence(Ordering::SeqCst);
    }

    fn write_address(&mut self, low_offset: usize, address: u64) {
        self.write(low_offset, address as u32);
        self.write(low_offset + 4, (address >> 32) as u32);
    }
}
