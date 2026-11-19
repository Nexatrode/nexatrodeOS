//! Guest entropy and wall-clock adapters for certificate-validated TLS.
//! The importing protection domain supplies the two channel constants.
use core::num::NonZeroU32;
use sel4_microkit::{get_mr, MessageInfo};

getrandom::register_custom_getrandom!(fill_random);

fn fill_random(destination: &mut [u8]) -> Result<(), getrandom::Error> {
    for offset in (0..destination.len()).step_by(16) {
        let reply = super::TLS_ENTROPY_CHANNEL.pp_call(MessageInfo::new(0x401, 0));
        if reply.label() != 0x400 || reply.count() != 2 {
            destination.fill(0);
            return Err(getrandom::Error::from(NonZeroU32::new(getrandom::Error::CUSTOM_START).unwrap()));
        }
        let mut sample = [0u8; 16];
        sample[..8].copy_from_slice(&get_mr(0).to_le_bytes());
        sample[8..].copy_from_slice(&get_mr(1).to_le_bytes());
        let count = (destination.len() - offset).min(16);
        destination[offset..offset + count].copy_from_slice(&sample[..count]);
        sample.fill(0);
    }
    Ok(())
}

pub fn unix_time() -> Option<u64> {
    let reply = super::TLS_CLOCK_CHANNEL.pp_call(MessageInfo::new(0x205, 0));
    if reply.label() != 0x100 || reply.count() != 1 { return None; }
    // Reject an unset RTC; rustls additionally checks the certificate's dates.
    let seconds = get_mr(0);
    (seconds >= 1_577_836_800).then_some(seconds)
}
