//! QMP link/channel negotiation without payload transmission.
//!
//! Native drivers execute only the named AP-owned descriptor writes returned
//! here, then issue one ordered doorbell. No mailbox length, payload, remote
//! state, or remote-owned acknowledgement field is writable through this API.
//! The caller supplies exclusive AP endpoint ownership; descriptor validation
//! alone cannot establish ownership. Poll limits are work bounds, not timers.

use crate::qcom_qmp::{Descriptor, MailboxLayout, QMP_DOWN, QMP_UP};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApWrite {
    RemoteLinkAck,
    LocalLink,
    RemoteChannelAck(u32),
    LocalChannel(u32),
}

impl ApWrite {
    pub const fn word_index(self) -> usize {
        match self {
            Self::RemoteLinkAck => 4,
            Self::LocalLink => 9,
            Self::RemoteChannelAck(_) => 6,
            Self::LocalChannel(_) => 11,
        }
    }

    pub const fn value(self) -> u32 {
        match self {
            Self::RemoteLinkAck | Self::LocalLink => QMP_UP,
            Self::RemoteChannelAck(value) | Self::LocalChannel(value) => value,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Step {
    Notify([Option<ApWrite>; 2]),
    Wait,
    Connected,
    Complete,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Stage {
    Start,
    LinkAck,
    ChannelAck,
    Connected,
    DisconnectAck,
    Complete,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LinkError {
    InvalidDescriptor,
    InitialState,
    NonemptyMailbox,
    GeometryChanged,
    RemoteLinkLost,
    LocalStateChanged,
    PollLimit,
    AlreadyFailed,
}

impl LinkError {
    pub const fn code(self) -> u32 {
        match self {
            Self::InvalidDescriptor => 1,
            Self::InitialState => 2,
            Self::NonemptyMailbox => 3,
            Self::GeometryChanged => 4,
            Self::RemoteLinkLost => 5,
            Self::LocalStateChanged => 6,
            Self::PollLimit => 7,
            Self::AlreadyFailed => 8,
        }
    }
}

pub struct LinkHandshake {
    layout: MailboxLayout,
    window_bytes: u32,
    polls_per_stage: u32,
    polls: u32,
    stage: Stage,
    close_after_open: bool,
}

impl LinkHandshake {
    pub fn new(
        descriptor: Descriptor,
        window_bytes: u32,
        polls_per_stage: u32,
        receive_length: u32,
        transmit_length: u32,
    ) -> Result<Self, LinkError> {
        let layout = descriptor
            .validate(window_bytes)
            .map_err(|_| LinkError::InvalidDescriptor)?;
        if polls_per_stage == 0 {
            return Err(LinkError::PollLimit);
        }
        if receive_length != 0 || transmit_length != 0 {
            return Err(LinkError::NonemptyMailbox);
        }
        if descriptor.remote.link != QMP_UP
            || descriptor.remote.link_ack != QMP_DOWN
            || descriptor.remote.channel != QMP_DOWN
            || descriptor.remote.channel_ack != QMP_DOWN
            || descriptor.local.link != QMP_DOWN
            || descriptor.local.link_ack != QMP_DOWN
            || descriptor.local.channel != QMP_DOWN
            || descriptor.local.channel_ack != QMP_DOWN
        {
            return Err(LinkError::InitialState);
        }
        Ok(Self {
            layout,
            window_bytes,
            polls_per_stage,
            polls: 0,
            stage: Stage::Start,
            close_after_open: true,
        })
    }

    /// Open and hold the channel for a separate bounded packet operation.
    /// Unlike new(), this requires an explicit disconnect after that operation.
    pub fn open(
        descriptor: Descriptor,
        window_bytes: u32,
        polls_per_stage: u32,
        receive_length: u32,
        transmit_length: u32,
    ) -> Result<Self, LinkError> {
        let mut handshake = Self::new(
            descriptor,
            window_bytes,
            polls_per_stage,
            receive_length,
            transmit_length,
        )?;
        handshake.close_after_open = false;
        Ok(handshake)
    }

    /// Close only a held connection with empty mailboxes and a fresh, valid
    /// descriptor. A packet operation must complete before calling this.
    pub fn disconnect(
        &mut self,
        descriptor: Descriptor,
        receive_length: u32,
        transmit_length: u32,
    ) -> Result<Step, LinkError> {
        if self.stage != Stage::Connected {
            let error = if self.stage == Stage::Failed {
                LinkError::AlreadyFailed
            } else {
                LinkError::LocalStateChanged
            };
            self.stage = Stage::Failed;
            return Err(error);
        }
        // Reuse all layout, link, channel, and empty-mailbox validation.
        self.advance(descriptor, receive_length, transmit_length)?;
        self.change_stage(Stage::DisconnectAck);
        Ok(Step::Notify([Some(ApWrite::LocalChannel(QMP_DOWN)), None]))
    }

    pub const fn stage(&self) -> Stage {
        self.stage
    }

    /// Advance from a stable copied descriptor and its two length words.
    /// Returned writes must be completed before the next call. An error is
    /// terminal; the caller must not retry or clear shared state automatically.
    pub fn advance(
        &mut self,
        descriptor: Descriptor,
        receive_length: u32,
        transmit_length: u32,
    ) -> Result<Step, LinkError> {
        let result = self.advance_inner(descriptor, receive_length, transmit_length);
        if result.is_err() {
            self.stage = Stage::Failed;
        }
        result
    }

    fn advance_inner(
        &mut self,
        d: Descriptor,
        receive_length: u32,
        transmit_length: u32,
    ) -> Result<Step, LinkError> {
        if self.stage == Stage::Failed {
            return Err(LinkError::AlreadyFailed);
        }
        let layout = d
            .validate(self.window_bytes)
            .map_err(|_| LinkError::InvalidDescriptor)?;
        if layout != self.layout {
            return Err(LinkError::GeometryChanged);
        }
        if receive_length != 0 || transmit_length != 0 {
            return Err(LinkError::NonemptyMailbox);
        }
        if d.remote.link != QMP_UP {
            return Err(LinkError::RemoteLinkLost);
        }
        if self.stage == Stage::Complete {
            return Ok(Step::Complete);
        }
        if self.polls >= self.polls_per_stage {
            return Err(LinkError::PollLimit);
        }
        self.polls += 1;
        if self.stage != Stage::Start && (d.local.link != QMP_UP || d.remote.link_ack != QMP_UP) {
            return Err(LinkError::LocalStateChanged);
        }

        match self.stage {
            Stage::Start => {
                // Revalidate the complete initial state immediately before the
                // first writes, rather than trusting only new()'s snapshot.
                Self::new(
                    d,
                    self.window_bytes,
                    self.polls_per_stage,
                    receive_length,
                    transmit_length,
                )?;
                self.change_stage(Stage::LinkAck);
                Ok(Step::Notify([
                    Some(ApWrite::RemoteLinkAck),
                    Some(ApWrite::LocalLink),
                ]))
            }
            Stage::LinkAck => {
                if d.local.channel != QMP_DOWN {
                    return Err(LinkError::LocalStateChanged);
                }
                if d.local.link_ack != QMP_UP {
                    return Ok(Step::Wait);
                }
                self.change_stage(Stage::ChannelAck);
                let ack = (d.remote.channel != d.remote.channel_ack)
                    .then_some(ApWrite::RemoteChannelAck(d.remote.channel));
                Ok(Step::Notify([ack, Some(ApWrite::LocalChannel(QMP_UP))]))
            }
            Stage::ChannelAck => {
                if d.local.link_ack != QMP_UP || d.local.channel != QMP_UP {
                    return Err(LinkError::LocalStateChanged);
                }
                if d.remote.channel != d.remote.channel_ack {
                    return Ok(Step::Notify([
                        Some(ApWrite::RemoteChannelAck(d.remote.channel)),
                        None,
                    ]));
                }
                if !d.fully_connected() {
                    return Ok(Step::Wait);
                }
                if !self.close_after_open {
                    self.change_stage(Stage::Connected);
                    return Ok(Step::Connected);
                }
                self.change_stage(Stage::DisconnectAck);
                Ok(Step::Notify([Some(ApWrite::LocalChannel(QMP_DOWN)), None]))
            }
            Stage::Connected => {
                if !d.fully_connected() {
                    return Err(LinkError::LocalStateChanged);
                }
                Ok(Step::Connected)
            }
            Stage::DisconnectAck => {
                if d.local.link_ack != QMP_UP || d.local.channel != QMP_DOWN {
                    return Err(LinkError::LocalStateChanged);
                }
                if d.remote.channel != d.remote.channel_ack {
                    return Ok(Step::Notify([
                        Some(ApWrite::RemoteChannelAck(d.remote.channel)),
                        None,
                    ]));
                }
                if d.local.channel_ack != QMP_DOWN {
                    return Ok(Step::Wait);
                }
                self.change_stage(Stage::Complete);
                Ok(Step::Complete)
            }
            Stage::Complete => Ok(Step::Complete),
            Stage::Failed => Err(LinkError::AlreadyFailed),
        }
    }

    fn change_stage(&mut self, stage: Stage) {
        self.stage = stage;
        self.polls = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::qcom_qmp::QMP_MAGIC;

    // Observed geometry and visible state from aop-handoff.json. Reserved
    // word was not displayed by the device; zero here is test-only padding.
    fn handoff() -> [u32; 16] {
        [
            QMP_MAGIC, 1, 0, QMP_UP, QMP_DOWN, QMP_DOWN, QMP_DOWN, 0x68, 0xc0, QMP_DOWN, QMP_DOWN,
            QMP_DOWN, QMP_DOWN, 0x2d8, 0x128, 0,
        ]
    }

    fn descriptor(words: &[u32; 16]) -> Descriptor {
        Descriptor::from_words(words)
    }

    fn apply(words: &mut [u32; 16], step: Step) {
        if let Step::Notify(writes) = step {
            for write in writes.into_iter().flatten() {
                assert!([4, 6, 9, 11].contains(&write.word_index()));
                assert!([QMP_DOWN, QMP_UP].contains(&write.value()));
                words[write.word_index()] = write.value();
            }
        }
    }

    #[test]
    fn observed_handoff_negotiates_opens_and_disconnects_without_payload_writes() {
        let mut words = handoff();
        let mut h = LinkHandshake::new(descriptor(&words), 4096, 10, 0, 0).unwrap();
        let step = h.advance(descriptor(&words), 0, 0).unwrap();
        assert_eq!(
            step,
            Step::Notify([Some(ApWrite::RemoteLinkAck), Some(ApWrite::LocalLink)])
        );
        apply(&mut words, step);
        assert_eq!(h.advance(descriptor(&words), 0, 0), Ok(Step::Wait));
        words[10] = QMP_UP; // AOP acknowledges AP link.
        let step = h.advance(descriptor(&words), 0, 0).unwrap();
        apply(&mut words, step);
        assert_eq!(h.stage(), Stage::ChannelAck);
        words[12] = QMP_UP; // AOP acknowledges AP channel.
        words[5] = QMP_UP; // AOP opens its channel.
        let step = h.advance(descriptor(&words), 0, 0).unwrap();
        assert_eq!(
            step,
            Step::Notify([Some(ApWrite::RemoteChannelAck(QMP_UP)), None])
        );
        apply(&mut words, step);
        let step = h.advance(descriptor(&words), 0, 0).unwrap();
        assert_eq!(h.stage(), Stage::DisconnectAck);
        apply(&mut words, step);
        assert_eq!(h.advance(descriptor(&words), 0, 0), Ok(Step::Wait));
        words[12] = QMP_DOWN;
        words[5] = QMP_DOWN;
        let step = h.advance(descriptor(&words), 0, 0).unwrap();
        apply(&mut words, step);
        assert_eq!(h.advance(descriptor(&words), 0, 0), Ok(Step::Complete));
        assert_eq!(h.stage(), Stage::Complete);
        assert_eq!(
            (words[7], words[8], words[13], words[14]),
            (0x68, 0xc0, 0x2d8, 0x128)
        );
    }

    #[test]
    fn refuses_active_unclear_or_nonempty_handoffs() {
        for index in [4, 5, 6, 9, 10, 11, 12] {
            let mut words = handoff();
            words[index] = QMP_UP;
            assert!(matches!(
                LinkHandshake::new(descriptor(&words), 4096, 10, 0, 0),
                Err(LinkError::InitialState)
            ));
        }
        for (rx, tx) in [(4, 0), (0, 4)] {
            assert!(matches!(
                LinkHandshake::new(descriptor(&handoff()), 4096, 10, rx, tx),
                Err(LinkError::NonemptyMailbox)
            ));
        }
    }

    #[test]
    fn detects_changed_geometry_and_remote_reset_without_recovery_writes() {
        for (index, value, error) in [
            (14, 0x400, LinkError::GeometryChanged),
            (3, QMP_DOWN, LinkError::RemoteLinkLost),
            (1, 2, LinkError::InvalidDescriptor),
        ] {
            let mut words = handoff();
            let mut h = LinkHandshake::new(descriptor(&words), 4096, 10, 0, 0).unwrap();
            let step = h.advance(descriptor(&words), 0, 0).unwrap();
            apply(&mut words, step);
            words[index] = value;
            assert_eq!(h.advance(descriptor(&words), 0, 0), Err(error));
            assert_eq!(h.stage(), Stage::Failed);
            assert_eq!(
                h.advance(descriptor(&handoff()), 0, 0),
                Err(LinkError::AlreadyFailed)
            );
        }
    }

    #[test]
    fn rechecks_initial_state_and_rejects_unexpected_payloads() {
        let mut words = handoff();
        let mut h = LinkHandshake::new(descriptor(&words), 4096, 10, 0, 0).unwrap();
        words[11] = QMP_UP;
        assert_eq!(
            h.advance(descriptor(&words), 0, 0),
            Err(LinkError::InitialState)
        );
        let mut h = LinkHandshake::new(descriptor(&handoff()), 4096, 10, 0, 0).unwrap();
        assert_eq!(
            h.advance(descriptor(&handoff()), 4, 0),
            Err(LinkError::NonemptyMailbox)
        );
    }

    #[test]
    fn missing_ack_exhausts_a_finite_stage_budget() {
        let mut words = handoff();
        let mut h = LinkHandshake::new(descriptor(&words), 4096, 2, 0, 0).unwrap();
        let step = h.advance(descriptor(&words), 0, 0).unwrap();
        apply(&mut words, step);
        assert_eq!(h.advance(descriptor(&words), 0, 0), Ok(Step::Wait));
        assert_eq!(h.advance(descriptor(&words), 0, 0), Ok(Step::Wait));
        assert_eq!(
            h.advance(descriptor(&words), 0, 0),
            Err(LinkError::PollLimit)
        );
    }

    #[test]
    fn maximum_budget_does_not_overflow_its_poll_counter() {
        let mut words = handoff();
        let mut h = LinkHandshake::new(descriptor(&words), 4096, u32::MAX, 0, 0).unwrap();
        let step = h.advance(descriptor(&words), 0, 0).unwrap();
        apply(&mut words, step);
        h.polls = u32::MAX - 1;
        assert_eq!(h.advance(descriptor(&words), 0, 0), Ok(Step::Wait));
        assert_eq!(
            h.advance(descriptor(&words), 0, 0),
            Err(LinkError::PollLimit)
        );
    }

    #[test]
    fn acknowledges_early_remote_open_and_allows_remote_channel_to_remain_open() {
        let mut words = handoff();
        let mut h = LinkHandshake::new(descriptor(&words), 4096, 10, 0, 0).unwrap();
        let step = h.advance(descriptor(&words), 0, 0).unwrap();
        apply(&mut words, step);
        words[10] = QMP_UP;
        words[5] = QMP_UP;
        let step = h.advance(descriptor(&words), 0, 0).unwrap();
        assert_eq!(
            step,
            Step::Notify([
                Some(ApWrite::RemoteChannelAck(QMP_UP)),
                Some(ApWrite::LocalChannel(QMP_UP))
            ])
        );
        apply(&mut words, step);
        words[12] = QMP_UP;
        let step = h.advance(descriptor(&words), 0, 0).unwrap();
        apply(&mut words, step);
        words[12] = QMP_DOWN;
        // Like Linux LOCAL_DISCONNECTING, success requires the AP channel's
        // close acknowledgement. AOP need not close its own channel as well.
        assert_eq!(h.advance(descriptor(&words), 0, 0), Ok(Step::Complete));
    }

    #[test]
    fn held_channel_waits_for_an_explicit_empty_mailbox_disconnect() {
        let mut words = handoff();
        let mut h = LinkHandshake::open(descriptor(&words), 4096, 10, 0, 0).unwrap();
        let step = h.advance(descriptor(&words), 0, 0).unwrap();
        apply(&mut words, step);
        words[10] = QMP_UP;
        words[5] = QMP_UP;
        let step = h.advance(descriptor(&words), 0, 0).unwrap();
        apply(&mut words, step);
        words[12] = QMP_UP;
        assert_eq!(h.advance(descriptor(&words), 0, 0), Ok(Step::Connected));
        assert_eq!(words[11], QMP_UP);
        assert_eq!(h.advance(descriptor(&words), 0, 0), Ok(Step::Connected));
        let step = h.disconnect(descriptor(&words), 0, 0).unwrap();
        apply(&mut words, step);
        assert_eq!(words[11], QMP_DOWN);
        words[12] = QMP_DOWN;
        assert_eq!(h.advance(descriptor(&words), 0, 0), Ok(Step::Complete));
    }

    #[test]
    fn cannot_disconnect_before_open_or_with_unconsumed_transmit() {
        let mut words = handoff();
        let mut h = LinkHandshake::open(descriptor(&words), 4096, 10, 0, 0).unwrap();
        assert_eq!(
            h.disconnect(descriptor(&words), 0, 0),
            Err(LinkError::LocalStateChanged)
        );
        let mut h = LinkHandshake::open(descriptor(&words), 4096, 10, 0, 0).unwrap();
        let step = h.advance(descriptor(&words), 0, 0).unwrap();
        apply(&mut words, step);
        words[10] = QMP_UP;
        words[5] = QMP_UP;
        let step = h.advance(descriptor(&words), 0, 0).unwrap();
        apply(&mut words, step);
        words[12] = QMP_UP;
        assert_eq!(h.advance(descriptor(&words), 0, 0), Ok(Step::Connected));
        assert_eq!(
            h.disconnect(descriptor(&words), 0, 96),
            Err(LinkError::NonemptyMailbox)
        );
    }
}
