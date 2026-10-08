//! ST FTS FIFO packets observed on the Pixel 4a touch controller.
//!
//! The controller returns eight bytes for a FIFO read (`0x86`). The event
//! high nibble identifies enter/motion/leave; bytes 2-4 pack X/Y in the same
//! layout decoded by the sunfish Linux driver. Bus access is separate: this
//! module does not initialize the controller or consume its FIFO.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContactKind {
    Down,
    Move,
    Up,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Contact {
    pub kind: ContactKind,
    pub id: u8,
    pub x: u16,
    pub y: u16,
}

pub fn decode_fts_event(packet: [u8; 8]) -> Option<Contact> {
    let kind = match packet[0] >> 4 {
        1 => ContactKind::Down,
        2 => ContactKind::Move,
        3 => ContactKind::Up,
        _ => return None,
    };
    // The low nibble is the tool type. A normal finger is 1 in the live
    // trace; other tool types need their own policy before routing to Slint.
    if packet[1] & 0x0f != 1 {
        return None;
    }
    let x = (u16::from(packet[3] & 0x0f) << 8) | u16::from(packet[2]);
    let y = (u16::from(packet[4]) << 4) | u16::from(packet[3] >> 4);
    if x >= 1080 || y >= 2340 {
        return None;
    }
    Some(Contact {
        kind,
        id: packet[1] >> 4,
        x,
        y,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_contact_and_release_from_live_pixel_trace() {
        // Captured together with Android ABS_MT_POSITION_X=0x1df,
        // ABS_MT_POSITION_Y=0x590 and BTN_TOUCH transitions 1 -> 0.
        let down = [0x13, 0x01, 0xdf, 0x01, 0x59, 0x0a, 0x21, 0x00];
        let up = [0x33, 0x01, 0xdf, 0x01, 0x59, 0x10, 0x20, 0x00];
        assert_eq!(
            decode_fts_event(down),
            Some(Contact {
                kind: ContactKind::Down,
                id: 0,
                x: 479,
                y: 1424,
            })
        );
        assert_eq!(
            decode_fts_event(up).map(|event| event.kind),
            Some(ContactKind::Up)
        );
    }
}
