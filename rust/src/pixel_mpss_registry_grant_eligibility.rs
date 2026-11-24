//! Exact borrowed-turn eligibility counters; no scheduling decisions.
use super::*;
impl RuntimeDomain {
    pub(super) fn show_tftp_completion(&self, display: &Display, owner: &ModemOwner) {
        let window = owner.registry_window_trace();
        let prior = window.predecessor;
        let pressure = prior.and_then(|p| p.publication);
        let registry = owner.service_registry_trace();
        let tx4 = registry.txn4;
        let elf = registry.elf;
        let ack = owner.original_tftp_write_trace().filter(|w| w.data.is_some_and(|d|
            d.stages.final_prepared && d.stages.final_queued && w.final_block == Some(d.block))).map(|w| w.ack);
        let age = |at| self.time_from_us(Some(self.runtime_started), at);
        let duration_ms = |ticks| { let us = self.time_from_us(Some(0), ticks);
            if us == u32::MAX { u32::MAX } else { us / 1000 } };
        let pair = |a: u32, b: u32| (a.min(65535) << 16) | b.min(65535);
        let query = self.wlan_query.snapshot(owner);
        let gates = pressure.map(|p| p.grant_gates);
        let packed = |i| gates.map_or(u32::MAX, |g| pair(g[i], g[i+1]));
        #[cfg(not(feature="sunfish-registry-converted-grant-turn"))]
        display.replace_line(1, "REGISTRY GRANT GATES");
        #[cfg(feature="sunfish-registry-converted-grant-turn")]
        display.replace_line(1, "REGISTRY GRANT RETRY");
        for (row, (label, value)) in [
            ("RUNTIME ERROR", self.error),
            ("OLD SNAP STATE", prior.map_or(0, |p| if p.publication_frozen { 2 } else { 1 })),
            ("OLD QUEUE US", age(prior.and_then(|p| p.queued_at))),
            ("OLD PUB US", age(prior.and_then(|p| p.published_at))),
            ("OLD DONE US", age(prior.and_then(|p| p.done_at))),
            ("OLD BYTES", pressure.map_or(u32::MAX, |p| u32::from(p.bytes))),
            ("OLD CALLS", pressure.map_or(u32::MAX, |p| p.calls)),
            ("OLD MASK", pressure.map_or(u32::MAX, |p| p.mask)),
            ("NOACK DENY", packed(0)),
            ("NOAD NOYIELD", packed(2)),
            ("COOL TFTP", packed(4)),
            ("WRQ OWNRET", packed(6)),
            ("USED NOCAND", packed(8)),
            ("CREDIT BORROW", packed(10)),
            ("ACK SENT", pressure.map_or(u32::MAX, |p| p.ack.sent)),
            ("ACK MAX AGE MS", duration_ms(pressure.and_then(|p| p.ack.max_age))),
            ("TX4 RX US", age(tx4.and_then(|t| t.received))),
            ("TX4 QUEUE US", age(tx4.and_then(|t| t.queued))),
            ("TX4 PUB US", age(tx4.and_then(|t| t.published))),
            ("ELF RX US", age(elf.and_then(|e| e.received))),
            ("ELF QUEUE US", age(elf.and_then(|e| e.queued))),
            ("ELF PUB US", age(elf.and_then(|e| e.published))),
            ("ELF DONE US", age(elf.and_then(|e| e.done))),
            ("ACK PHASE", ack.map_or(u32::MAX, |a| u32::from(a.phase))),
            ("IPA COMPLETE", u32::from(self.ipa_complete_seen)),
            ("WLAN RRQ RX", owner.tftp_wire_counts()[2]),
            ("WLFW SERVERS", query[7]),
        ].into_iter().enumerate() { display.hex(row + 2, label, value); }
    }
}
