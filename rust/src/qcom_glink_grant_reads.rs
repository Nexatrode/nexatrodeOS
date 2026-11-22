//! Correlate exact IPCRTR offers with validated peer-tail advances. Distances
//! are relative to the sampled tail and include any older unpublished-to-peer bytes.
use crate::qcom_glink_qmi::{MAX_DATAGRAM_BYTES, RX_SLOTS};

#[derive(Clone, Copy, Default, Debug)]
pub struct BufferOffer {
    pub id: u32,
    pub size: u32,
    pub command: u8,
    pub published: Option<u64>,
    pub last_short: Option<u64>,
    pub first_read: Option<u64>,
    remaining: usize,
    #[cfg(feature = "sunfish-oack-grant-outcome")]
    pub peer_read: bool,
}

impl BufferOffer {
    fn observe(&mut self, consumed: usize, start: Option<u64>, end: Option<u64>) {
        if self.remaining == 0 { return; }
        self.remaining = self.remaining.saturating_sub(consumed);
        if self.remaining == 0 {
            self.first_read = end;
            #[cfg(feature = "sunfish-oack-grant-outcome")]
            { self.peer_read = true; }
        } else if start.is_some() {
            self.last_short = start;
        }
    }

    pub fn order(&self, absent: Option<u64>, visible: Option<u64>) -> u32 {
        if self.first_read.zip(absent).is_some_and(|(read, absent)| read < absent) { 1 }
        else if self.last_short.zip(visible).is_some_and(|(short, visible)| short > visible) { 2 }
        else if self.first_read.is_some() && absent.is_some() && visible.is_some() { 3 }
        else { 0 }
    }
}

#[derive(Default, Debug)]
pub struct GrantBufferReads {
    pub requested: Option<u32>,
    pub backing_id: Option<u32>,
    pub absent: Option<u64>,
    pub visible: Option<u64>,
    pub decoded: Option<u64>,
    pub matched: bool,
    pub fits: [Option<BufferOffer>; RX_SLOTS],
}

/// One fixed receive-window request and the buffer inventory at original OACK publication.
#[cfg(feature = "sunfish-oack-retry-close")]
#[derive(Clone, Copy)]
pub struct PendingTftp {
    pub opcode: u16,
    pub bytes: u16,
    pub path_kind: u8,
    pub exact_wrq: bool,
    pub first_repeat: bool,
}

#[cfg(feature = "sunfish-oack-retry-close")]
impl PendingTftp {
    fn read(record: &[u8]) -> Option<Self> {
        use super::word;
        if record.len() < 50 || record[0] != 9 || word(record, 12) != 0 { return None; }
        let header = super::RegistrationHeader::read(record, false, None);
        if header.qrtr != Some([1, 0x1f, crate::qcom_tftp_server_check::SERVICE_PORT]) { return None; }
        let packet = record.get(16..16usize.checked_add(word(record, 8) as usize)?)?;
        let body = packet.get(32..32usize.checked_add(word(packet, 20) as usize)?)?;
        let opcode = u16::from_be_bytes(body.get(..2)?.try_into().ok()?);
        Some(Self { opcode, bytes: u16::try_from(body.len()).ok()?,
            path_kind: header.tftp_request.map_or(0, |request| request.kind),
            exact_wrq: body == crate::qcom_tftp_server_check::REQUEST, first_repeat: false })
    }
}

#[cfg(feature = "sunfish-oack-grant-outcome")]
#[derive(Default)]
pub struct OackGrantTrace {
    pub request_offset: Option<u32>,
    pub request_at: Option<u64>,
    pub initial_granted: Option<bool>,
    pub intent: Option<u32>,
    pub reserved_at: Option<u64>,
    pub advertisement: Option<BufferOffer>,
    pub ack: Option<BufferOffer>,
    pub ack_granted: Option<bool>,
    pub oack_captured: bool,
    pub oack_at: Option<u64>,
    #[cfg(feature = "sunfish-first-oack-spare-buffer")]
    pub primed_offer: Option<BufferOffer>,
    pub available: [Option<BufferOffer>; RX_SLOTS],
    pub read_at_oack: u32,
    pub states_before_writer: u32,
    pub rx_at_oack: u32,
    #[cfg(feature = "sunfish-oack-grant-ack-turn")]
    pub pending_clear: Option<u64>,
    #[cfg(feature = "sunfish-oack-grant-ack-turn")]
    pub used: [Option<super::WrqWindowHeader>; RX_SLOTS],
    #[cfg(feature = "sunfish-oack-grant-ack-turn")]
    pub use_prefix_bytes: [u32; RX_SLOTS],
    #[cfg(feature = "sunfish-oack-retry-close")]
    pub pending_tftp: [Option<PendingTftp>; RX_SLOTS],
    #[cfg(feature = "sunfish-oack-grant-ack-turn")]
    pending_tail: u32,
    #[cfg(feature = "sunfish-oack-grant-ack-turn")]
    pending_remaining: u32,
    waiting: bool,
}

#[derive(Default)]
pub(super) struct GrantReadLedger {
    pub clock_armed: bool,
    live: [Option<BufferOffer>; RX_SLOTS],
    pub request: GrantBufferReads,
    #[cfg(feature = "sunfish-oack-grant-outcome")]
    pub outcome: OackGrantTrace,
    #[cfg(feature = "sunfish-oack-grant-outcome")]
    pub oack_pool: Option<([Option<u32>; RX_SLOTS], u32)>,
}

impl GrantReadLedger {
    #[cfg(feature = "sunfish-first-oack-spare-buffer")]
    pub fn capture_primed(&mut self, id: u32) {
        if self.outcome.primed_offer.is_none() {
            self.outcome.primed_offer = self.live.iter().flatten().find(|offer| offer.id == id).copied();
        }
    }
    #[cfg(feature = "sunfish-oack-grant-ack-turn")]
    pub fn pending_active(&self, tail: u32) -> bool {
        self.outcome.pending_remaining != 0 && self.outcome.pending_tail == tail
    }

    #[cfg(feature = "sunfish-oack-grant-ack-turn")]
    pub fn start_pending(&mut self, tail: u32) {
        self.outcome.pending_tail = tail;
        self.outcome.pending_remaining = self.outcome.rx_at_oack;
        if self.outcome.rx_at_oack == 0 { self.outcome.pending_clear = self.outcome.oack_at; }
    }

    /// Called only with an already validated record before its tail-store attempt.
    /// The initial eight bytes must be in the original interval to identify its ID.
    #[cfg(feature = "sunfish-oack-grant-ack-turn")]
    pub fn check_pending(&mut self, tail: u32, record: &[u8], at: Option<u64>) {
        let trace = &mut self.outcome;
        if trace.pending_remaining < 8 || trace.pending_tail != tail || record.len() < 8
            || !matches!(record[0], 9 | 12) || record[2..4] != [2, 0] { return; }
        let id = super::word(record, 4);
        for (slot, offer) in trace.available.iter().enumerate() {
            if offer.is_some_and(|offer| offer.id == id) && trace.used[slot].is_none() {
                trace.used[slot] = Some(super::WrqWindowHeader {
                    offset: trace.rx_at_oack - trace.pending_remaining,
                    header: super::WrqPrefixHeader::read(record), channel: 2, qmi_kind: 0xff,
                    name: u32::MAX, checked: at, clear: None,
                });
                trace.use_prefix_bytes[slot] = trace.pending_remaining.min(record.len() as u32);
                #[cfg(feature = "sunfish-oack-retry-close")]
                { trace.pending_tftp[slot] = PendingTftp::read(record); }
            }
        }
    }

    #[cfg(feature = "sunfish-oack-retry-close")]
    pub fn pending_repeat_identity(&mut self, tail: u32, matched: bool) {
        if !self.pending_active(tail) { return; }
        let offset = self.outcome.rx_at_oack - self.outcome.pending_remaining;
        for (slot, used) in self.outcome.used.iter().enumerate() {
            if used.is_some_and(|used| used.offset == offset)
                && let Some(meta) = self.outcome.pending_tftp[slot].as_mut()
            { meta.first_repeat = matched; }
        }
    }

    /// Only a successful RX-tail store advances the frozen interval. Later
    /// appends cannot extend it, including a record straddling its last byte.
    #[cfg(feature = "sunfish-oack-grant-ack-turn")]
    pub fn advance_pending(&mut self, tail: u32, next: u32, size: u32, at: Option<u64>) {
        let trace = &mut self.outcome;
        if trace.pending_remaining == 0 || trace.pending_tail != tail { return; }
        let offset = trace.rx_at_oack - trace.pending_remaining;
        for used in trace.used.iter_mut().flatten().filter(|used| used.offset == offset) {
            used.clear = at;
        }
        trace.pending_remaining = trace.pending_remaining.saturating_sub(size);
        trace.pending_tail = next;
        if trace.pending_remaining == 0 { trace.pending_clear = at; }
    }

    pub fn offer(&mut self, record: &[u8], remaining: usize, start: Option<u64>, end: Option<u64>) {
        use super::word;
        let (id, size) = match (record[0], record.len()) {
            (5, 16) if word(record, 4) == 1 => (word(record, 12), word(record, 8)),
            (14, 8) => (word(record, 4), MAX_DATAGRAM_BYTES as u32),
            _ => return,
        };
        self.release(id);
        if let Some(entry) = self.live.iter_mut().find(|e| e.is_none()) {
            *entry = Some(BufferOffer { id, size, command: record[0], published: end,
                last_short: start, remaining, first_read: None,
                #[cfg(feature = "sunfish-oack-grant-outcome")]
                peer_read: false,
            });
        }
    }

    pub fn release(&mut self, id: u32) {
        if let Some(entry) = self.live.iter_mut().find(|e| e.is_some_and(|offer| offer.id == id)) {
            *entry = None;
        }
    }

    pub fn sample(&mut self, consumed: usize, start: Option<u64>, clock: Option<fn() -> u64>) {
        let mut end = None;
        for offer in self.live.iter_mut().chain(&mut self.request.fits).flatten() {
            if self.clock_armed && offer.remaining != 0 && consumed >= offer.remaining && end.is_none() {
                end = clock.map(|now| now());
            }
            offer.observe(consumed, start, end);
        }
        #[cfg(feature = "sunfish-first-oack-spare-buffer")]
        if let Some(offer) = self.outcome.primed_offer.as_mut() {
            if self.clock_armed && offer.remaining != 0 && consumed >= offer.remaining && end.is_none() {
                end = clock.map(|now| now());
            }
            offer.observe(consumed, start, end);
        }
        #[cfg(feature = "sunfish-oack-grant-outcome")]
        for offer in self.outcome.available.iter_mut().flatten()
            .chain(self.outcome.advertisement.iter_mut()).chain(self.outcome.ack.iter_mut())
        {
            if self.clock_armed && offer.remaining != 0 && consumed >= offer.remaining && end.is_none() {
                end = clock.map(|now| now());
            }
            offer.observe(consumed, start, end);
        }
    }

    #[cfg(feature = "sunfish-oack-grant-outcome")]
    pub fn capture_oack(&mut self, available: [Option<u32>; RX_SLOTS], states: u32, unread: u32, at: Option<u64>) {
        if self.outcome.oack_captured { return; }
        self.outcome.oack_captured = true;
        self.outcome.oack_at = at;
        self.outcome.states_before_writer = states;
        self.outcome.rx_at_oack = unread;
        for (slot, id) in available.into_iter().enumerate() {
            if let Some(id) = id {
                let offer = self.live.iter().flatten().find(|offer| offer.id == id).copied()
                    .unwrap_or(BufferOffer { id, size: MAX_DATAGRAM_BYTES as u32, ..Default::default() });
                self.outcome.read_at_oack |= u32::from(offer.peer_read) << slot;
                self.outcome.available[slot] = Some(offer);
            }
        }
    }

    #[cfg(feature = "sunfish-oack-grant-outcome")]
    pub fn capture_108(&mut self, offset: u32, granted: bool, intent: Option<u32>, reserved: Option<u64>, at: Option<u64>) {
        if self.outcome.request_offset.is_some() { return; }
        self.outcome.request_offset = Some(offset);
        self.outcome.request_at = at;
        self.outcome.initial_granted = Some(granted);
        self.outcome.intent = intent;
        self.outcome.reserved_at = reserved;
        self.outcome.waiting = true;
    }

    #[cfg(feature = "sunfish-oack-grant-outcome")]
    pub fn sync_108(&mut self, intent: Option<u32>, reserved: Option<u64>) {
        if self.outcome.waiting && self.outcome.intent.is_none() && intent.is_some() {
            self.outcome.intent = intent;
            self.outcome.reserved_at = reserved;
        }
    }

    #[cfg(feature = "sunfish-oack-grant-outcome")]
    pub fn watches_grant_head(&self, record: &[u8]) -> bool {
        use super::word;
        self.outcome.waiting && (
            (record[0] == 8 && record.len() == 8)
            || (record[0] == 5 && record.len() == 16 && word(record, 4) == 1
                && self.outcome.intent == Some(word(record, 12)) && self.outcome.advertisement.is_none())
        )
    }

    #[cfg(feature = "sunfish-oack-grant-outcome")]
    pub fn record_grant_head(&mut self, record: &[u8], remaining: usize, at: Option<u64>) {
        use super::word;
        if !self.outcome.waiting { return; }
        if record[0] == 5 && record.len() == 16 && word(record, 4) == 1
            && self.outcome.intent == Some(word(record, 12)) && self.outcome.advertisement.is_none()
        {
            self.outcome.advertisement = Some(BufferOffer {
                id: word(record, 12), size: word(record, 8), command: 5,
                published: at, remaining, ..Default::default()
            });
        } else if record[0] == 8 && record.len() == 8 {
            self.outcome.ack = Some(BufferOffer {
                command: 8, published: at, remaining, ..Default::default()
            });
            self.outcome.ack_granted = Some(word(record, 4) != 0);
            self.outcome.waiting = false;
        }
    }

    pub fn capture(&mut self, requested: u32, backing_id: Option<u32>, available: [Option<u32>; RX_SLOTS],
        absent: Option<u64>, visible: Option<u64>, decoded: Option<u64>) {
        if self.request.matched { return; }
        self.request = GrantBufferReads { requested: Some(requested), backing_id, absent, visible,
            decoded, ..Default::default() };
        for (slot, id) in available.into_iter().flatten().enumerate() {
            let offer = self.live.iter().flatten().find(|offer| offer.id == id).copied()
                .unwrap_or(BufferOffer { id, size: MAX_DATAGRAM_BYTES as u32, ..Default::default() });
            if requested != 0 && offer.size >= requested {
                self.request.fits[slot] = Some(offer);
            }
        }
    }

    pub fn match_backing(&mut self, requested: u32, intent: Option<u32>) {
        if intent.is_some() && self.request.requested == Some(requested)
            && (self.request.backing_id.is_none() || self.request.backing_id == intent) {
            self.request.backing_id = intent;
            self.request.matched = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "sunfish-first-oack-spare-buffer")]
    #[test]
    fn primed_offer_survives_release_reuse_and_partial_peer_tail() {
        let mut ledger = GrantReadLedger::default();
        ledger.clock_armed = true;
        let mut record = crate::control(5, 1, 1);
        record.extend(4096u32.to_le_bytes()); record.extend(0x44u32.to_le_bytes());
        ledger.offer(&record, 24, Some(10), Some(11));
        ledger.capture_primed(0x43);
        assert!(ledger.outcome.primed_offer.is_none());
        ledger.sample(8, Some(12), Some(|| 13));
        ledger.capture_primed(0x44);
        ledger.release(0x44);
        record[12..16].copy_from_slice(&0x45u32.to_le_bytes());
        ledger.offer(&record, 40, Some(14), Some(15));
        ledger.capture_primed(0x45);
        ledger.sample(8, Some(16), Some(|| 17));
        assert_eq!(ledger.outcome.primed_offer.unwrap().id, 0x44);
        assert_eq!(ledger.outcome.primed_offer.unwrap().first_read, None);
        ledger.sample(8, Some(18), Some(|| 19));
        let offer = ledger.outcome.primed_offer.unwrap();
        assert_eq!((offer.id, offer.published, offer.last_short, offer.first_read, offer.peer_read),
            (0x44, Some(11), Some(16), Some(19), true));
        ledger.sample(100, Some(20), Some(|| 21));
        assert_eq!(ledger.outcome.primed_offer.unwrap().first_read, Some(19));
    }

    #[cfg(feature = "sunfish-oack-retry-close")]
    #[test]
    fn pending_tftp_metadata_distinguishes_exact_retry_from_same_size_other_packet() {
        let packet = |body: &[u8]| {
            let mut qrtr = std::vec::Vec::new();
            for value in [1u32, 1, 0, 0x1f, 0, body.len() as u32, 1, 0x5000] {
                qrtr.extend(value.to_le_bytes());
            }
            qrtr.extend(body);
            qrtr.resize((qrtr.len() + 3) & !3, 0);
            crate::data(0x20, &qrtr)
        };
        let exact = packet(crate::qcom_tftp_server_check::REQUEST);
        let meta = PendingTftp::read(&exact).unwrap();
        assert_eq!((meta.opcode, meta.path_kind, meta.bytes, meta.exact_wrq), (2, 1, 73, true));
        let mut changed = crate::qcom_tftp_server_check::REQUEST.to_vec();
        changed[3] = b'X';
        let other = PendingTftp::read(&packet(&changed)).unwrap();
        assert_eq!((other.bytes, other.path_kind, other.exact_wrq), (73, 6, false));
        let mut fragment = exact.clone(); fragment[12] = 8;
        assert!(PendingTftp::read(&fragment).is_none());
        let mut wrong_port = exact.clone();wrong_port[44..48].copy_from_slice(&0x5001u32.to_le_bytes());
        assert!(PendingTftp::read(&wrong_port).is_none());
    }

    #[cfg(feature = "sunfish-oack-grant-ack-turn")]
    #[test]
    fn pending_use_is_bound_to_frozen_window_and_confirmed_tail() {
        let mut ledger = GrantReadLedger::default();
        let mut ids = [None; RX_SLOTS];
        ids[2] = Some(0x20);
        ledger.capture_oack(ids, 0, 24, Some(1));
        ledger.start_pending(4080);
        let record = crate::data(0x20, &[0; 8]);
        ledger.check_pending(4080, &record, Some(2));
        let used = ledger.outcome.used[2].unwrap();
        assert_eq!((used.offset, used.header.command, used.header.parameter), (0, 9, 0x20));
        assert!(used.clear.is_none() && ledger.outcome.pending_clear.is_none());
        // A failed tail store cannot confirm use or clear the fixed boundary.
        ledger.advance_pending(4080, 16, 32, Some(3));
        assert_eq!(ledger.outcome.used[2].unwrap().clear, Some(3));
        assert_eq!(ledger.outcome.pending_clear, Some(3));
        ledger.check_pending(16, &record, Some(4));
        assert_eq!(ledger.outcome.used[2].unwrap().checked, Some(2));
        assert_eq!(ledger.outcome.used[2].unwrap().clear, Some(3));
    }

    #[cfg(feature = "sunfish-oack-grant-ack-turn")]
    #[test]
    fn pending_use_ignores_wrong_tail_channel_id_and_partial_header() {
        let mut ledger = GrantReadLedger::default();
        let mut ids = [None; RX_SLOTS];
        ids[0] = Some(0x20);
        ledger.capture_oack(ids, 0, 8, Some(1));
        ledger.start_pending(0);
        ledger.check_pending(8, &crate::data(0x20, &[]), Some(2));
        ledger.check_pending(0, &crate::data(0x21, &[]), Some(2));
        let mut wrong_channel = crate::data(0x20, &[]);
        wrong_channel[2] = 3;
        ledger.check_pending(0, &wrong_channel, Some(2));
        assert!(ledger.outcome.used[0].is_none());
        ledger.advance_pending(0, 8, 8, Some(3));
        ledger.check_pending(8, &crate::data(0x20, &[]), Some(4));
        assert!(ledger.outcome.used[0].is_none());
        assert_eq!(ledger.outcome.pending_clear, Some(3));
    }

    #[test]
    fn read_bounds_survive_partial_advances_release_and_reuse() {
        let mut ledger = GrantReadLedger::default();
        ledger.clock_armed = true;
        let mut ad = [0; 16];
        ad[0] = 5;
        ad[4] = 1;
        ad[8..12].copy_from_slice(&4096u32.to_le_bytes());
        ad[12] = 4;
        ledger.offer(&ad, 24, Some(1), Some(2));
        ledger.sample(16, Some(3), Some(|| 4));
        let mut ids = [None; RX_SLOTS];
        ids[0] = Some(4);
        ledger.capture(108, Some(8), ids, Some(5), Some(7), Some(9));
        ledger.match_backing(108, Some(9));
        assert!(!ledger.request.matched);
        ledger.match_backing(108, Some(8));
        ledger.release(4);
        ledger.offer(&[14,0,1,0,4,0,0,0], 16, Some(10), Some(11));
        ledger.sample(4, Some(12), Some(|| 13));
        ledger.sample(4, Some(14), Some(|| 15));
        ledger.capture(52, Some(10), ids, Some(16), Some(17), Some(18));
        let offer = ledger.request.fits[0].unwrap();
        assert_eq!((offer.command, offer.first_read, offer.last_short), (5, Some(15), Some(12)));
        assert_eq!(offer.order(ledger.request.absent, ledger.request.visible), 2);
        assert_eq!(offer.order(Some(16), Some(18)), 1);
        assert_eq!(offer.order(Some(15), Some(16)), 3);
        assert_eq!(offer.order(None, None), 0);
        assert_eq!(ledger.live[0].unwrap().first_read, None);
    }
}

#[cfg(all(test, feature = "sunfish-oack-grant-outcome"))]
mod oack_grant_tests {
    use super::*;

    fn advertisement(id: u32) -> [u8; 16] {
        let mut record = [0; 16];
        record[0] = 5;
        record[4] = 1;
        record[8..12].copy_from_slice(&4096u32.to_le_bytes());
        record[12..16].copy_from_slice(&id.to_le_bytes());
        record
    }

    #[test]
    fn matches_exact_request_and_freezes_original_oack_inventory() {
        let mut ledger = GrantReadLedger::default();
        ledger.clock_armed = true;
        ledger.offer(&advertisement(4), 16, Some(1), Some(2));
        ledger.sample(16, Some(3), Some(|| 4));
        let mut available = [None; RX_SLOTS];
        available[2] = Some(4);
        ledger.capture_oack(available, 0x200, 88, Some(5));
        ledger.release(4);
        assert_eq!(ledger.outcome.available[2].unwrap().id, 4);
        assert_eq!(ledger.outcome.available[2].unwrap().first_read, Some(4));
        ledger.capture_108(96, true, Some(9), Some(6), Some(7));
        ledger.capture_108(0, true, Some(10), Some(8), Some(9));
        ledger.record_grant_head(&advertisement(10), 16, Some(10));
        assert!(ledger.outcome.advertisement.is_none());
        ledger.record_grant_head(&advertisement(9), 24, Some(11));
        let mut ack = [8,0,1,0,1,0,0,0];
        ledger.record_grant_head(&ack, 32, Some(12));
        ledger.sample(24, Some(13), Some(|| 14));
        assert_eq!(ledger.outcome.advertisement.unwrap().first_read, Some(14));
        assert_eq!(ledger.outcome.ack.unwrap().first_read, None);
        ledger.sample(8, Some(15), Some(|| 16));
        assert_eq!(ledger.outcome.ack.unwrap().first_read, Some(16));
        ack[4] = 0;
        ledger.record_grant_head(&ack, 8, Some(17));
        assert_eq!(ledger.outcome.ack_granted, Some(true));
        assert_eq!(ledger.outcome.intent, Some(9));
    }

    #[test]
    fn read_evidence_before_profiler_and_frozen_snapshot_survive_reuse() {
        let mut ledger = GrantReadLedger::default();
        ledger.offer(&advertisement(4), 16, None, None);
        ledger.sample(16, None, None);
        let mut available = [None; RX_SLOTS];
        available[0] = Some(4);
        ledger.capture_oack(available, 2, 104, Some(10));
        assert_eq!(ledger.outcome.read_at_oack, 1);
        assert_eq!(ledger.outcome.available[0].unwrap().first_read, None);
        ledger.release(4);
        ledger.offer(&advertisement(5), 16, Some(11), Some(12));
        ledger.capture_oack([None; RX_SLOTS], 0, 0, Some(13));
        assert_eq!(ledger.outcome.available[0].unwrap().id, 4);
        assert_eq!(ledger.outcome.states_before_writer, 2);
        assert_eq!(ledger.outcome.rx_at_oack, 104);
    }

    #[test]
    fn negative_ack_closes_only_the_tracked_request() {
        let mut ledger = GrantReadLedger::default();
        ledger.capture_108(96, false, None, None, Some(1));
        ledger.record_grant_head(&[8,0,1,0,0,0,0,0], 8, Some(2));
        ledger.capture_108(0, true, Some(9), Some(3), Some(4));
        ledger.sync_108(Some(9), Some(3));
        ledger.record_grant_head(&advertisement(9), 16, Some(5));
        ledger.record_grant_head(&[8,0,1,0,1,0,0,0], 24, Some(6));
        assert_eq!(ledger.outcome.intent, None);
        assert_eq!(ledger.outcome.ack_granted, Some(false));
        assert_eq!(ledger.outcome.ack.unwrap().published, Some(2));
        assert!(ledger.outcome.advertisement.is_none());
    }

    #[test]
    fn denied_request_can_retain_a_later_retry_reservation() {
        let mut ledger = GrantReadLedger::default();
        ledger.capture_108(96, false, None, None, Some(1));
        ledger.sync_108(Some(9), Some(2));
        ledger.record_grant_head(&advertisement(9), 16, Some(3));
        ledger.record_grant_head(&[8,0,1,0,1,0,0,0], 24, Some(4));
        assert_eq!(ledger.outcome.initial_granted, Some(false));
        assert_eq!(ledger.outcome.ack_granted, Some(true));
        assert_eq!(ledger.outcome.intent, Some(9));
    }
}
