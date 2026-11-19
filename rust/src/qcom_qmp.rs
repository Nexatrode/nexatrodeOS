//! Bounded Qualcomm AOP QMP mailbox wire format.
//!
//! Reference: Lineage kernel 28f9290ae067, drivers/mailbox/msm_qmp.c and
//! drivers/soc/qcom/peripheral-loader.c. No Linux implementation is imported.
//! This module decodes copied descriptor words and encodes one named modem
//! notification in caller-independent RAM. It performs no MMIO or doorbell
//! writes, and a valid descriptor does not establish mailbox ownership.

pub const QMP_MAGIC: u32 = 0x4d41_494c;
pub const QMP_VERSION: u32 = 1;
pub const QMP_DESCRIPTOR_WORDS: usize = 16;
pub const QMP_DESCRIPTOR_BYTES: u32 = 64;
pub const QMP_DOWN: u32 = 0xffff_0000;
pub const QMP_UP: u32 = 0x0000_ffff;
pub const MODEM_NOTIFICATION_BYTES: usize = 96;
pub const MODEM_PACKET_BYTES: usize = MODEM_NOTIFICATION_BYTES + 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QmpError {
    InvalidMagic,
    UnsupportedVersion,
    UnsupportedFeatures,
    InvalidState,
    UnconfiguredMailbox,
    UnalignedMailbox,
    MailboxOutsideWindow,
    DescriptorOverlap,
    MailboxOverlap,
    InvalidPacketLength,
    MailboxTooSmall,
}

impl QmpError {
    pub const fn code(self) -> u32 {
        match self {
            Self::InvalidMagic => 1,
            Self::UnsupportedVersion => 2,
            Self::UnsupportedFeatures => 3,
            Self::InvalidState => 4,
            Self::UnconfiguredMailbox => 5,
            Self::UnalignedMailbox => 6,
            Self::MailboxOutsideWindow => 7,
            Self::DescriptorOverlap => 8,
            Self::MailboxOverlap => 9,
            Self::InvalidPacketLength => 10,
            Self::MailboxTooSmall => 11,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChannelDescriptor {
    pub link: u32,
    pub link_ack: u32,
    pub channel: u32,
    pub channel_ack: u32,
    /// Includes the four-byte published packet-length word.
    pub mailbox_size: u32,
    /// Relative to the descriptor, which is at MSGRAM offset zero on Sunfish.
    pub mailbox_offset: u32,
}

impl ChannelDescriptor {
    fn from_words(words: &[u32]) -> Self {
        Self {
            link: words[0],
            link_ack: words[1],
            channel: words[2],
            channel_ack: words[3],
            mailbox_size: words[4],
            mailbox_offset: words[5],
        }
    }

    fn validate(self, window_bytes: u32) -> Result<MailboxSpan, QmpError> {
        for state in [self.link, self.link_ack, self.channel, self.channel_ack] {
            if state != QMP_UP && state != QMP_DOWN {
                return Err(QmpError::InvalidState);
            }
        }
        let offset = self.mailbox_offset;
        let size = self.mailbox_size;
        if size == 0 {
            return Err(QmpError::UnconfiguredMailbox);
        }
        if offset % 4 != 0 || size % 4 != 0 {
            return Err(QmpError::UnalignedMailbox);
        }
        if offset < QMP_DESCRIPTOR_BYTES {
            return Err(QmpError::DescriptorOverlap);
        }
        if size < 4
            || offset
                .checked_add(size)
                .is_none_or(|end| end > window_bytes)
        {
            return Err(QmpError::MailboxOutsideWindow);
        }
        Ok(MailboxSpan { offset, size })
    }

    pub const fn fully_connected(self) -> bool {
        self.link == QMP_UP
            && self.link_ack == QMP_UP
            && self.channel == QMP_UP
            && self.channel_ack == QMP_UP
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Descriptor {
    pub magic: u32,
    pub version: u32,
    pub features: u32,
    /// Remote AOP side: its mailbox is AP receive storage.
    pub remote: ChannelDescriptor,
    /// Local AP side: its mailbox is AP transmit storage.
    pub local: ChannelDescriptor,
    pub reserved: u32,
}

impl Descriptor {
    pub fn from_words(words: &[u32; QMP_DESCRIPTOR_WORDS]) -> Self {
        Self {
            magic: words[0],
            version: words[1],
            features: words[2],
            remote: ChannelDescriptor::from_words(&words[3..9]),
            local: ChannelDescriptor::from_words(&words[9..15]),
            reserved: words[15],
        }
    }

    pub fn validate(self, window_bytes: u32) -> Result<MailboxLayout, QmpError> {
        if self.magic != QMP_MAGIC {
            return Err(QmpError::InvalidMagic);
        }
        if self.version != QMP_VERSION {
            return Err(QmpError::UnsupportedVersion);
        }
        if self.features != 0 {
            return Err(QmpError::UnsupportedFeatures);
        }
        let receive = self.remote.validate(window_bytes)?;
        let transmit = self.local.validate(window_bytes)?;
        if receive.offset < transmit.offset + transmit.size
            && transmit.offset < receive.offset + receive.size
        {
            return Err(QmpError::MailboxOverlap);
        }
        Ok(MailboxLayout { receive, transmit })
    }

    /// Protocol state only; does not prove exclusive AP ownership or a fresh peer.
    pub const fn fully_connected(self) -> bool {
        self.remote.fully_connected() && self.local.fully_connected()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MailboxSpan {
    offset: u32,
    size: u32,
}

impl MailboxSpan {
    pub const fn offset(self) -> u32 {
        self.offset
    }
    pub const fn size(self) -> u32 {
        self.size
    }

    pub fn validate_packet_length(self, length: u32) -> Result<(), QmpError> {
        if length % 4 != 0 || length > self.size - 4 {
            return Err(QmpError::InvalidPacketLength);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MailboxLayout {
    pub receive: MailboxSpan,
    pub transmit: MailboxSpan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModemLoadState {
    On,
    Off,
}

impl MailboxLayout {
    /// Encode the PIL load-state notification. This is not a power-sequence
    /// command or modem-ready acknowledgement. All unused bytes are zeroed.
    pub fn modem_load_state(
        self,
        state: ModemLoadState,
    ) -> Result<[u8; MODEM_PACKET_BYTES], QmpError> {
        if self.transmit.size < MODEM_PACKET_BYTES as u32 {
            return Err(QmpError::MailboxTooSmall);
        }
        let message: &[u8] = match state {
            ModemLoadState::On => b"{class: image, res: load_state, name: modem, val: on}",
            ModemLoadState::Off => b"{class: image, res: load_state, name: modem, val: off}",
        };
        let mut packet = [0u8; MODEM_PACKET_BYTES];
        packet[..4].copy_from_slice(&(MODEM_NOTIFICATION_BYTES as u32).to_le_bytes());
        packet[4..4 + message.len()].copy_from_slice(message);
        Ok(packet)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words() -> [u32; 16] {
        [
            QMP_MAGIC, 1, 0, QMP_UP, QMP_UP, QMP_UP, QMP_UP, 256, 64, QMP_UP, QMP_UP, QMP_UP,
            QMP_UP, 256, 320, 0,
        ]
    }

    #[test]
    fn decodes_remote_and_ap_geometry_and_each_state() {
        let raw = words();
        let descriptor = Descriptor::from_words(&raw);
        let layout = descriptor.validate(4096).unwrap();
        assert_eq!(
            layout.receive,
            MailboxSpan {
                offset: 64,
                size: 256
            }
        );
        assert_eq!(
            layout.transmit,
            MailboxSpan {
                offset: 320,
                size: 256
            }
        );
        assert!(descriptor.fully_connected());
        for index in [3, 4, 5, 6, 9, 10, 11, 12] {
            let mut changed = raw;
            changed[index] = QMP_DOWN;
            let disconnected = Descriptor::from_words(&changed);
            assert!(disconnected.validate(4096).is_ok());
            assert!(!disconnected.fully_connected());
            changed[index] = 0x1234;
            assert_eq!(
                Descriptor::from_words(&changed).validate(4096),
                Err(QmpError::InvalidState)
            );
        }
    }

    #[test]
    fn rejects_unknown_protocol_before_mailbox_access() {
        for (index, value, error) in [
            (0, 0, QmpError::InvalidMagic),
            (1, 2, QmpError::UnsupportedVersion),
            (2, 1, QmpError::UnsupportedFeatures),
        ] {
            let mut raw = words();
            raw[index] = value;
            assert_eq!(Descriptor::from_words(&raw).validate(4096), Err(error));
        }
    }

    #[test]
    fn rejects_bad_ranges_for_both_mailboxes() {
        for base in [3, 9] {
            for (size, offset, error) in [
                (0, 64, QmpError::UnconfiguredMailbox),
                (7, 64, QmpError::UnalignedMailbox),
                (4, 65, QmpError::UnalignedMailbox),
                (4, 60, QmpError::DescriptorOverlap),
                (256, 3968, QmpError::MailboxOutsideWindow),
                (256, 0xffff_fffc, QmpError::MailboxOutsideWindow),
            ] {
                let mut raw = words();
                raw[base + 4] = size;
                raw[base + 5] = offset;
                assert_eq!(Descriptor::from_words(&raw).validate(4096), Err(error));
            }
        }
        let mut raw = words();
        raw[14] = 316;
        assert_eq!(
            Descriptor::from_words(&raw).validate(4096),
            Err(QmpError::MailboxOverlap)
        );
        assert!(Descriptor::from_words(&words()).validate(576).is_ok());
        assert_eq!(
            Descriptor::from_words(&words()).validate(575),
            Err(QmpError::MailboxOutsideWindow)
        );
    }

    #[test]
    fn length_word_cannot_extend_beyond_its_own_mailbox() {
        let layout = Descriptor::from_words(&words()).validate(4096).unwrap();
        for span in [layout.receive, layout.transmit] {
            assert_eq!(span.validate_packet_length(0), Ok(()));
            assert_eq!(span.validate_packet_length(252), Ok(()));
            for length in [1, 253, 256, u32::MAX] {
                assert_eq!(
                    span.validate_packet_length(length),
                    Err(QmpError::InvalidPacketLength)
                );
            }
        }
    }

    #[test]
    fn encodes_only_named_modem_messages_without_stack_padding() {
        let layout = Descriptor::from_words(&words()).validate(4096).unwrap();
        for (state, expected) in [
            (
                ModemLoadState::On,
                "{class: image, res: load_state, name: modem, val: on}",
            ),
            (
                ModemLoadState::Off,
                "{class: image, res: load_state, name: modem, val: off}",
            ),
        ] {
            let packet = layout.modem_load_state(state).unwrap();
            assert_eq!(&packet[..4], &96u32.to_le_bytes());
            assert_eq!(&packet[4..4 + expected.len()], expected.as_bytes());
            assert!(packet[4 + expected.len()..].iter().all(|byte| *byte == 0));
        }
        let mut raw = words();
        raw[13] = 96;
        assert_eq!(
            Descriptor::from_words(&raw)
                .validate(4096)
                .unwrap()
                .modem_load_state(ModemLoadState::On),
            Err(QmpError::MailboxTooSmall)
        );
        raw[13] = 100;
        assert!(
            Descriptor::from_words(&raw)
                .validate(4096)
                .unwrap()
                .modem_load_state(ModemLoadState::On)
                .is_ok()
        );
    }
}
