//! Post-publication observation only. Scheduling and FIFO validation are unchanged.
use super::*;
impl RuntimeDomain {
    pub(super) fn show_tftp_completion(&self, display: &Display, owner: &ModemOwner) {
        let window = owner.registry_window_trace();
        let prior = window.predecessor;
        let fifo = owner.registry_reply_fifo();
        let visibility = fifo.and_then(|f| f.completion);
        let grant = prior.and_then(|p| p.grant);
        let pair = window.target;
        let pressure = pair.and_then(|p| p.publication);
        let registry = owner.service_registry_trace();
        let tx4 = registry.txn4;
        let elf = registry.elf;
        let age = |at| self.time_from_us(Some(self.runtime_started), at);
        let pack = |a: u32, b: u32| (a.min(65535) << 16) | b.min(65535);
        let gates = |i| pressure.map_or(u32::MAX, |p| pack(p.grant_gates[i], p.grant_gates[i+1]));
        display.replace_line(1, "REGISTRY POST REPLY");
        for (row, (label, value)) in [
            ("RUNTIME ERROR", self.error),
            ("OLD PUB US", age(prior.and_then(|p| p.published_at))),
            ("OLD TAIL US", age(fifo.and_then(|f| f.peer_tail_at))),
            ("OLD RX ABSENT", age(visibility.and_then(|v| v.last_absent))),
            ("OLD RX VISIBLE", age(visibility.and_then(|v| v.first_visible))),
            ("OLD DONE US", age(prior.and_then(|p| p.done_at))),
            ("OLD RX AHEAD", visibility.and_then(|v| v.bytes_ahead).unwrap_or(u32::MAX)),
            ("RX END EVICT", fifo.map_or(u32::MAX, |f| pack(u32::from(f.finished), f.history_evictions))),
            ("GRANT ID", grant.map_or(u32::MAX, |g| g.intent)),
            ("GRANT RESERVED", age(grant.and_then(|g| g.converted_at))),
            ("GRANT AD US", age(grant.and_then(|g| g.advertised_at))),
            ("GRANT ACK US", age(grant.and_then(|g| g.ack_at))),
            ("TX4 QUEUE US", age(tx4.and_then(|t| t.queued))),
            ("TX4 PUB US", age(tx4.and_then(|t| t.published))),
            ("TX4 SNAP STATE", pair.map_or(0, |p| if p.publication_frozen { 2 } else { 1 })),
            ("TX4 CALLS", pressure.map_or(u32::MAX, |p| p.calls)),
            ("TX4 MASK", pressure.map_or(u32::MAX, |p| p.mask)),
            ("NOACK DENY", gates(0)),
            ("NOAD NOYIELD", gates(2)),
            ("COOL TFTP", gates(4)),
            ("WRQ OWNRET", gates(6)),
            ("USED NOCAND", gates(8)),
            ("CREDIT BORROW", gates(10)),
            ("ELF DONE US", age(elf.and_then(|e| e.done))),
            ("IPA COMPLETE", u32::from(self.ipa_complete_seen)),
            ("WLAN RRQ RX", owner.tftp_wire_counts()[2]),
            ("WLFW SERVERS", self.wlan_query.snapshot(owner)[7]),
        ].into_iter().enumerate() { display.hex(row + 2, label, value); }
    }
}
