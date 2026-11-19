//! Typed fixed modem load-state notifications over a negotiated QMP channel.
//!
//! Reference: matched Lineage peripheral-loader.c::pil_notify_aop and
//! msm_qmp.c::qmp_send_data. This is a bounded transport operation, not modem
//! power sequencing. Length clearing indicates consumption, not semantic
//! command acceptance. ON and OFF have separate constructors/packet types;
//! neither accepts an arbitrary payload or supplies retry/RX clearing.

use crate::qcom_qmp::{
    Descriptor, MODEM_NOTIFICATION_BYTES, MODEM_PACKET_BYTES, MailboxLayout, ModemLoadState,
};

#[derive(Debug, Eq, PartialEq)]
pub struct ModemOffPacket([u32; MODEM_PACKET_BYTES / 4]);
#[derive(Debug, Eq, PartialEq)]
pub struct ModemOnPacket([u32; MODEM_PACKET_BYTES / 4]);
impl ModemOnPacket {
    pub const fn words(&self) -> &[u32; MODEM_PACKET_BYTES / 4] {
        &self.0
    }
}

impl ModemOffPacket {
    /// Host-endian words: length first, then zero-padded payload. The MMIO
    /// adapter must publish payload first, barrier, then length and doorbell.
    pub const fn words(&self) -> &[u32; MODEM_PACKET_BYTES / 4] {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransferStage {
    Ready,
    InFlight,
    Consumed,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransferProgress {
    Wait,
    Consumed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransferError {
    InvalidDescriptor,
    NotConnected,
    GeometryChanged,
    NonemptyMailbox,
    NoCapacity,
    WrongStage,
    UnexpectedLength,
    RemoteData,
    PollLimit,
}

impl TransferError {
    pub const fn code(self) -> u32 {
        match self {
            Self::InvalidDescriptor => 0x21,
            Self::NotConnected => 0x22,
            Self::GeometryChanged => 0x23,
            Self::NonemptyMailbox => 0x24,
            Self::NoCapacity => 0x25,
            Self::WrongStage => 0x26,
            Self::UnexpectedLength => 0x27,
            Self::RemoteData => 0x28,
            Self::PollLimit => 0x29,
        }
    }
}

struct Notification {
    layout: MailboxLayout,
    window_bytes: u32,
    remaining_polls: u32,
    stage: TransferStage,
    load_state: ModemLoadState,
}

impl Notification {
    fn new(
        descriptor: Descriptor,
        window_bytes: u32,
        polls: u32,
        receive_length: u32,
        transmit_length: u32,
        load_state: ModemLoadState,
    ) -> Result<Self, TransferError> {
        let layout = descriptor
            .validate(window_bytes)
            .map_err(|_| TransferError::InvalidDescriptor)?;
        if !descriptor.fully_connected() {
            return Err(TransferError::NotConnected);
        }
        if receive_length != 0 || transmit_length != 0 {
            return Err(TransferError::NonemptyMailbox);
        }
        if layout.transmit.size() < MODEM_PACKET_BYTES as u32 {
            return Err(TransferError::NoCapacity);
        }
        if polls == 0 {
            return Err(TransferError::PollLimit);
        }
        Ok(Self {
            layout,
            window_bytes,
            remaining_polls: polls,
            stage: TransferStage::Ready,
            load_state,
        })
    }

    pub const fn stage(&self) -> TransferStage {
        self.stage
    }

    fn validate(&self, d: Descriptor, receive_length: u32) -> Result<(), TransferError> {
        let layout = d
            .validate(self.window_bytes)
            .map_err(|_| TransferError::InvalidDescriptor)?;
        if layout != self.layout {
            return Err(TransferError::GeometryChanged);
        }
        if !d.fully_connected() {
            return Err(TransferError::NotConnected);
        }
        if receive_length != 0 {
            return Err(TransferError::RemoteData);
        }
        Ok(())
    }

    /// Return the packet at most once, after rechecking fresh channel state.
    /// The caller must publish these words before poll(); no I/O is performed
    /// here, and synthetic poll inputs are not hardware evidence.
    pub fn start(
        &mut self,
        d: Descriptor,
        receive_length: u32,
        transmit_length: u32,
    ) -> Result<[u32; MODEM_PACKET_BYTES / 4], TransferError> {
        let result = (|| {
            if self.stage != TransferStage::Ready {
                return Err(TransferError::WrongStage);
            }
            self.validate(d, receive_length)?;
            if transmit_length != 0 {
                return Err(TransferError::NonemptyMailbox);
            }
            let bytes = self
                .layout
                .modem_load_state(self.load_state)
                .map_err(|_| TransferError::NoCapacity)?;
            let mut words = [0; MODEM_PACKET_BYTES / 4];
            for (word, chunk) in words.iter_mut().zip(bytes.chunks_exact(4)) {
                *word = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            }
            self.stage = TransferStage::InFlight;
            Ok(words)
        })();
        if result.is_err() {
            self.stage = TransferStage::Failed;
        }
        result
    }

    pub fn poll(
        &mut self,
        d: Descriptor,
        receive_length: u32,
        transmit_length: u32,
    ) -> Result<TransferProgress, TransferError> {
        let result = (|| {
            if self.stage != TransferStage::InFlight {
                return Err(TransferError::WrongStage);
            }
            self.validate(d, receive_length)?;
            if self.remaining_polls == 0 {
                return Err(TransferError::PollLimit);
            }
            self.remaining_polls -= 1;
            match transmit_length {
                0 => {
                    self.stage = TransferStage::Consumed;
                    Ok(TransferProgress::Consumed)
                }
                length if length == MODEM_NOTIFICATION_BYTES as u32 => Ok(TransferProgress::Wait),
                _ => Err(TransferError::UnexpectedLength),
            }
        })();
        if result.is_err() {
            self.stage = TransferStage::Failed;
        }
        result
    }
}

macro_rules! typed_notification {
    ($name:ident, $packet:ident, $state:ident) => {
        pub struct $name(Notification);
        impl $name {
            pub fn new(
                descriptor: Descriptor,
                window_bytes: u32,
                polls: u32,
                receive_length: u32,
                transmit_length: u32,
            ) -> Result<Self, TransferError> {
                Notification::new(
                    descriptor,
                    window_bytes,
                    polls,
                    receive_length,
                    transmit_length,
                    ModemLoadState::$state,
                )
                .map(Self)
            }
            pub const fn stage(&self) -> TransferStage {
                self.0.stage()
            }
            pub fn start(
                &mut self,
                descriptor: Descriptor,
                receive_length: u32,
                transmit_length: u32,
            ) -> Result<$packet, TransferError> {
                self.0
                    .start(descriptor, receive_length, transmit_length)
                    .map($packet)
            }
            pub fn poll(
                &mut self,
                descriptor: Descriptor,
                receive_length: u32,
                transmit_length: u32,
            ) -> Result<TransferProgress, TransferError> {
                self.0.poll(descriptor, receive_length, transmit_length)
            }
        }
    };
}
typed_notification!(OffNotification, ModemOffPacket, Off);
typed_notification!(OnNotification, ModemOnPacket, On);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::qcom_qmp::{QMP_DOWN, QMP_MAGIC, QMP_UP};

    fn connected() -> Descriptor {
        Descriptor::from_words(&[
            QMP_MAGIC, 1, 0, QMP_UP, QMP_UP, QMP_UP, QMP_UP, 0x68, 0xc0, QMP_UP, QMP_UP, QMP_UP,
            QMP_UP, 0x2d8, 0x128, 0,
        ])
    }

    #[test]
    fn separate_on_type_encodes_only_fixed_on_and_never_retries_or_accepts_remote_data() {
        let d = connected();
        let mut on = OnNotification::new(d, 4096, 1, 0, 0).unwrap();
        let packet = on.start(d, 0, 0).unwrap();
        let bytes: std::vec::Vec<_> = packet.words()[1..]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        let expected = b"{class: image, res: load_state, name: modem, val: on}";
        assert_eq!(&bytes[..expected.len()], expected);
        assert!(bytes[expected.len()..].iter().all(|b| *b == 0));
        assert_eq!(packet.words()[0], 96);
        assert_eq!(on.poll(d, 0, 96), Ok(TransferProgress::Wait));
        assert_eq!(on.poll(d, 0, 96), Err(TransferError::PollLimit));
        assert_eq!(on.start(d, 0, 0), Err(TransferError::WrongStage));
        let mut on = OnNotification::new(d, 4096, 1, 0, 0).unwrap();
        on.start(d, 0, 0).unwrap();
        assert_eq!(on.poll(d, 1, 0), Err(TransferError::RemoteData));
        let mut on = OnNotification::new(d, 4096, 1, 0, 0).unwrap();
        on.start(d, 0, 0).unwrap();
        assert_eq!(on.poll(d, 0, 0), Ok(TransferProgress::Consumed));
    }

    #[test]
    fn sends_only_fixed_off_packet_and_waits_for_length_consumption() {
        let d = connected();
        let mut tx = OffNotification::new(d, 4096, 10, 0, 0).unwrap();
        let packet = tx.start(d, 0, 0).unwrap();
        assert_eq!(packet.words()[0], 96);
        let bytes: std::vec::Vec<u8> = packet.words()[1..]
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect();
        let text = b"{class: image, res: load_state, name: modem, val: off}";
        assert_eq!(&bytes[..text.len()], text);
        assert!(bytes[text.len()..].iter().all(|b| *b == 0));
        assert_eq!(tx.poll(d, 0, 96), Ok(TransferProgress::Wait));
        assert_eq!(tx.stage(), TransferStage::InFlight);
        assert_eq!(tx.poll(d, 0, 0), Ok(TransferProgress::Consumed));
        assert_eq!(tx.stage(), TransferStage::Consumed);
    }

    #[test]
    fn refuses_disconnected_busy_undersized_or_zero_budget_start() {
        let mut d = connected();
        d.local.channel_ack = QMP_DOWN;
        assert!(matches!(
            OffNotification::new(d, 4096, 10, 0, 0),
            Err(TransferError::NotConnected)
        ));
        for (rx, tx) in [(4, 0), (0, 96)] {
            assert!(matches!(
                OffNotification::new(connected(), 4096, 10, rx, tx),
                Err(TransferError::NonemptyMailbox)
            ));
        }
        d = connected();
        d.local.mailbox_size = 96; // length word requires another four bytes.
        assert!(matches!(
            OffNotification::new(d, 4096, 10, 0, 0),
            Err(TransferError::NoCapacity)
        ));
        assert!(matches!(
            OffNotification::new(connected(), 4096, 0, 0, 0),
            Err(TransferError::PollLimit)
        ));
    }

    #[test]
    fn does_not_overwrite_or_retry_an_in_flight_packet() {
        let d = connected();
        let mut tx = OffNotification::new(d, 4096, 10, 0, 0).unwrap();
        tx.start(d, 0, 0).unwrap();
        assert_eq!(tx.start(d, 0, 96), Err(TransferError::WrongStage));
        assert_eq!(tx.stage(), TransferStage::Failed);
        assert_eq!(tx.start(d, 0, 0), Err(TransferError::WrongStage));
    }

    #[test]
    fn fresh_state_is_required_before_packet_publication() {
        let d = connected();
        let mut tx = OffNotification::new(d, 4096, 10, 0, 0).unwrap();
        assert_eq!(tx.start(d, 0, 4), Err(TransferError::NonemptyMailbox));
        let mut tx = OffNotification::new(d, 4096, 10, 0, 0).unwrap();
        let mut changed = d;
        changed.local.mailbox_offset = 0x400;
        assert_eq!(tx.start(changed, 0, 0), Err(TransferError::GeometryChanged));
    }

    #[test]
    fn unexpected_remote_data_lengths_geometry_or_channel_close_are_terminal() {
        let d = connected();
        let mut changed = d;
        changed.remote.channel = QMP_DOWN;
        let mut moved = d;
        moved.local.mailbox_offset = 0x400;
        let mut invalid = d;
        invalid.version = 2;
        for (snapshot, rx, len, error) in [
            (d, 4, 96, TransferError::RemoteData),
            (d, 0, 4, TransferError::UnexpectedLength),
            (changed, 0, 0, TransferError::NotConnected),
            (moved, 0, 0, TransferError::GeometryChanged),
            (invalid, 0, 0, TransferError::InvalidDescriptor),
        ] {
            let mut tx = OffNotification::new(d, 4096, 10, 0, 0).unwrap();
            tx.start(d, 0, 0).unwrap();
            assert_eq!(tx.poll(snapshot, rx, len), Err(error));
            assert_eq!(tx.stage(), TransferStage::Failed);
            assert_eq!(tx.poll(d, 0, 0), Err(TransferError::WrongStage));
        }
    }

    #[test]
    fn consumption_has_a_finite_budget_without_automatic_retry() {
        let d = connected();
        let mut tx = OffNotification::new(d, 4096, 2, 0, 0).unwrap();
        tx.start(d, 0, 0).unwrap();
        assert_eq!(tx.poll(d, 0, 96), Ok(TransferProgress::Wait));
        assert_eq!(tx.poll(d, 0, 96), Ok(TransferProgress::Wait));
        assert_eq!(tx.poll(d, 0, 0), Err(TransferError::PollLimit));
        assert_eq!(tx.stage(), TransferStage::Failed);
    }
}
