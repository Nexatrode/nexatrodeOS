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
}

impl BufferOffer {
    fn observe(&mut self, consumed: usize, start: Option<u64>, end: Option<u64>) {
        if self.remaining == 0 { return; }
        self.remaining = self.remaining.saturating_sub(consumed);
        if self.remaining == 0 {
            self.first_read = end;
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

#[derive(Default)]
pub(super) struct GrantReadLedger {
    pub clock_armed: bool,
    live: [Option<BufferOffer>; RX_SLOTS],
    pub request: GrantBufferReads,
}

impl GrantReadLedger {
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
                last_short: start, remaining, first_read: None });
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
