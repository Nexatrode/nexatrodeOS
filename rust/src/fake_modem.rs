#![no_std]
#![no_main]

mod modem_protocol;

use modem_protocol::{
    REPLY_BAD_REQUEST, REPLY_OK, REQUEST_RING, REQUEST_SMS, SMS_MAX_BYTES,
};
use sel4_microkit::{
    Channel, ChannelSet, Handler, Infallible, MessageInfo, MessageRegisterValue, debug_print,
    debug_println, get_mr, protection_domain,
};

const CLIENT_CHANNEL: Channel = Channel::new(0);

#[protection_domain]
fn init() -> FakeModem {
    debug_println!("MODEM|READY: fake telephony stub online");
    FakeModem {
        rings: 0,
        sms_count: 0,
    }
}

struct FakeModem {
    rings: MessageRegisterValue,
    sms_count: MessageRegisterValue,
}

impl FakeModem {
    fn handle_ring(&mut self) -> MessageInfo {
        self.rings = self.rings.wrapping_add(1);
        debug_println!("MODEM|RING");
        MessageInfo::new(REPLY_OK, 0)
    }

    fn handle_sms(&mut self, request: MessageInfo) -> MessageInfo {
        if request.count() != 3 {
            return MessageInfo::new(REPLY_BAD_REQUEST, 0);
        }
        let length = get_mr(0) as usize;
        if length == 0 || length > SMS_MAX_BYTES {
            return MessageInfo::new(REPLY_BAD_REQUEST, 0);
        }
        let mut body = [0u8; SMS_MAX_BYTES];
        let first = get_mr(1).to_le_bytes();
        let second = get_mr(2).to_le_bytes();
        body[..8].copy_from_slice(&first);
        body[8..].copy_from_slice(&second);
        for byte in &body[..length] {
            if !byte.is_ascii_alphanumeric() {
                return MessageInfo::new(REPLY_BAD_REQUEST, 0);
            }
        }
        self.sms_count = self.sms_count.wrapping_add(1);
        debug_print!("MODEM|SMS IN|");
        for byte in &body[..length] {
            debug_print!("{}", *byte as char);
        }
        debug_println!();
        MessageInfo::new(REPLY_OK, 0)
    }
}

impl Handler for FakeModem {
    type Error = Infallible;

    fn notified(&mut self, _channels: ChannelSet) -> Result<(), Self::Error> {
        debug_println!("MODEM|ERROR: unexpected notification");
        Ok(())
    }

    fn protected(
        &mut self,
        channel: Channel,
        request: MessageInfo,
    ) -> Result<MessageInfo, Self::Error> {
        if channel != CLIENT_CHANNEL {
            return Ok(MessageInfo::new(REPLY_BAD_REQUEST, 0));
        }
        Ok(match request.label() {
            REQUEST_RING if request.count() == 0 => self.handle_ring(),
            REQUEST_SMS => self.handle_sms(request),
            _ => MessageInfo::new(REPLY_BAD_REQUEST, 0),
        })
    }
}
