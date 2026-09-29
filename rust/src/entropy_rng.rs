#![no_std]
#![no_main]

mod entropy_protocol;
mod virtio_rng;

use entropy_protocol::{REPLY_OK, REPLY_UNAVAILABLE, REQUEST_BYTES, SAMPLE_BYTES};
use sel4_microkit::{
    Channel, ChannelSet, Handler, Infallible, MessageInfo, MessageRegisterValue, debug_println,
    memory_region_symbol, protection_domain, set_mr,
};
use virtio_rng::VirtioRng;

const CLIENT_CHANNEL: Channel = Channel::new(0);
const DMA_PHYSICAL_ADDRESS: u64 = 0x5fdd_c000;

#[protection_domain]
fn init() -> EntropyService {
    let registers = memory_region_symbol!(rng_regs: *mut u32);
    let dma = memory_region_symbol!(rng_dma: *mut u8);
    // Both mappings are private to this service and the DMA window is uncached.
    let rng = match unsafe { VirtioRng::initialize(registers, dma, DMA_PHYSICAL_ADDRESS) } {
        Ok(rng) => {
            debug_println!("ENTROPY|READY: QEMU VirtIO RNG initialized");
            Some(rng)
        }
        Err(error) => {
            debug_println!("ENTROPY|ERROR: VirtIO RNG init failed: {:?}", error);
            None
        }
    };
    EntropyService { rng }
}

struct EntropyService {
    rng: Option<VirtioRng>,
}

impl Handler for EntropyService {
    type Error = Infallible;

    fn notified(&mut self, _channels: ChannelSet) -> Result<(), Self::Error> {
        debug_println!("ENTROPY|ERROR: unexpected notification");
        Ok(())
    }

    fn protected(
        &mut self,
        channel: Channel,
        request: MessageInfo,
    ) -> Result<MessageInfo, Self::Error> {
        if channel != CLIENT_CHANNEL || request.label() != REQUEST_BYTES || request.count() != 0 {
            return Ok(MessageInfo::new(REPLY_UNAVAILABLE, 0));
        }
        let mut bytes = [0u8; SAMPLE_BYTES];
        let fill_result = self.rng.as_mut().map(|rng| rng.fill(&mut bytes));
        match fill_result {
            Some(Ok(())) => {}
            Some(Err(error)) => {
                debug_println!("ENTROPY|ERROR: device request failed: {:?}", error);
                // A timed-out request may still own the single ring descriptor.
                // Retire this instance instead of submitting a duplicate descriptor.
                self.rng = None;
                return Ok(MessageInfo::new(REPLY_UNAVAILABLE, 0));
            }
            None => return Ok(MessageInfo::new(REPLY_UNAVAILABLE, 0)),
        }
        let first = u64::from_le_bytes(bytes[..8].try_into().expect("eight byte field"));
        let second = u64::from_le_bytes(bytes[8..].try_into().expect("eight byte field"));
        set_mr(0, first as MessageRegisterValue);
        set_mr(1, second as MessageRegisterValue);
        Ok(MessageInfo::new(REPLY_OK, 2))
    }
}
