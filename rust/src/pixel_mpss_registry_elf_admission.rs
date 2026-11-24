//! Reuse retained exact reply/ELF observations; no new protocol instrumentation.
use super::*;
impl RuntimeDomain {
    pub(super) fn show_tftp_completion(&self, display: &Display, owner: &ModemOwner) {
        let registry=owner.service_registry_trace();
        let predecessor=owner.registry_window_trace().predecessor;
        let four=registry.txn4;
        let elf=registry.elf;
        let pressure=elf.and_then(|e|e.last_pressure);
        let receipt=owner.registry_reply_fifo().and_then(|f|f.completion);
        let age=|at|self.time_from_us(Some(self.runtime_started),at);
        display.replace_line(1,"REGISTRY ELF ADMISSION");
        for (row,(label,value)) in [
            ("RUNTIME ERROR",self.error),
            ("TX3 PUB US",age(predecessor.and_then(|p|p.published_at))),
            ("TX3 DONE US",age(predecessor.and_then(|p|p.done_at))),
            ("TX4 QUEUE US",age(four.and_then(|t|t.queued))),
            ("TX4 PUB US",age(four.and_then(|t|t.published))),
            ("TX4 DONE US",age(four.and_then(|t|t.done))),
            ("ELF RX US",age(elf.and_then(|e|e.received))),
            ("ELF RX END US",age(elf.and_then(|e|e.dispatch.receive_pass_end))),
            ("ELF ENTER US",age(elf.and_then(|e|e.dispatch.registry_entered))),
            ("ELF SELECT US",age(elf.and_then(|e|e.dispatch.selected))),
            ("ELF QUEUE US",age(elf.and_then(|e|e.queued))),
            ("ELF PUB US",age(elf.and_then(|e|e.published))),
            ("ELF DONE US",age(elf.and_then(|e|e.done))),
            ("WAIT PEER",elf.map_or(u32::MAX,|e|e.waits_by_reason[1])),
            ("WAIT TX FULL",elf.map_or(u32::MAX,|e|e.waits_by_reason[2])),
            ("WAIT CREDIT",elf.map_or(u32::MAX,|e|e.waits_by_reason[4])),
            ("LAST REASON",pressure.map_or(u32::MAX,|p|u32::from(p.reason))),
            ("LAST PEER TXN",pressure.and_then(|p|p.peer_publication).map_or(u32::MAX,|p|u32::from(p.transaction))),
            ("LAST PEER FLAGS",pressure.map_or(u32::MAX,|p|u32::from(p.peer_flags))),
            ("TX COUNTS",pressure.map_or(u32::MAX,|p|u32::from(p.used_slots)|(u32::from(p.unpublished_slots)<<8)|(u32::from(p.glink_wait_slots)<<16)|(u32::from(p.qrtr_wait_slots)<<24))),
            ("OLD RX ABS US",age(receipt.and_then(|r|r.last_absent))),
            ("OLD RX VIS US",age(receipt.and_then(|r|r.first_visible))),
            ("OLD RX AHEAD",receipt.and_then(|r|r.bytes_ahead).unwrap_or(u32::MAX)),
            ("PAINT DEFERRED",self.registry_paint.deferred),
            ("IPA COMPLETE",u32::from(self.ipa_complete_seen)),
            ("WLAN RRQ RX",owner.tftp_wire_counts()[2]),
            ("WLFW SERVERS",self.wlan_query.snapshot(owner)[7]),
        ].into_iter().enumerate(){display.hex(row+2,label,value);}
    }
}
