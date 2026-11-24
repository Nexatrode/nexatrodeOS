//! Existing exact predecessor FIFO/receipt and converted-grant observations.
use super::*;
impl RuntimeDomain {
    pub(super) fn show_tftp_completion(&self, display: &Display, owner: &ModemOwner) {
        let registry=owner.service_registry_trace();
        let prior=owner.registry_window_trace().predecessor;
        let grant=prior.and_then(|p|p.grant);
        let fifo=owner.registry_reply_fifo();
        let receipt=fifo.and_then(|f|f.completion);
        let four=registry.txn4;let elf=registry.elf;
        let age=|at|self.time_from_us(Some(self.runtime_started),at);
        display.replace_line(1,"REGISTRY TX3 RECEIPT");
        for (row,(label,value)) in [
            ("RUNTIME ERROR",self.error),
            ("TX3 QUEUE US",age(prior.and_then(|p|p.queued_at))),
            ("TX3 PUB US",age(prior.and_then(|p|p.published_at))),
            ("TX3 TAIL US",age(fifo.and_then(|f|f.peer_tail_at))),
            ("TX3 RX ABS US",age(receipt.and_then(|r|r.last_absent))),
            ("TX3 RX VIS US",age(receipt.and_then(|r|r.first_visible))),
            ("TX3 RX AHEAD",receipt.and_then(|r|r.bytes_ahead).unwrap_or(u32::MAX)),
            ("TX3 DONE US",age(prior.and_then(|p|p.done_at))),
            ("RX HISTORY LOST",fifo.map_or(u32::MAX,|f|f.history_evictions)),
            ("GRANT CONV US",age(grant.and_then(|g|g.converted_at))),
            ("GRANT AD US",age(grant.and_then(|g|g.advertised_at))),
            ("GRANT ACK US",age(grant.and_then(|g|g.ack_at))),
            ("TX4 PUB US",age(four.and_then(|t|t.published))),
            ("TX4 DONE US",age(four.and_then(|t|t.done))),
            ("ELF RX US",age(elf.and_then(|e|e.received))),
            ("ELF SELECT US",age(elf.and_then(|e|e.dispatch.selected))),
            ("ELF QUEUE US",age(elf.and_then(|e|e.queued))),
            ("ELF PUB US",age(elf.and_then(|e|e.published))),
            ("ELF DONE US",age(elf.and_then(|e|e.done))),
            ("WAIT PEER",elf.map_or(u32::MAX,|e|e.waits_by_reason[1])),
            ("TX3 BYTES",prior.and_then(|p|p.publication).map_or(u32::MAX,|p|u32::from(p.bytes))),
            ("RX MATCHED",fifo.map_or(u32::MAX,|f|u32::from(f.finished))),
            ("PAINT DEFERRED",self.registry_paint.deferred),
            ("GRANT INTENT",grant.map_or(u32::MAX,|g|g.intent)),
            ("IPA COMPLETE",u32::from(self.ipa_complete_seen)),
            ("WLAN RRQ RX",owner.tftp_wire_counts()[2]),
            ("WLFW SERVERS",self.wlan_query.snapshot(owner)[7]),
        ].into_iter().enumerate(){display.hex(row+2,label,value);}
    }
}
