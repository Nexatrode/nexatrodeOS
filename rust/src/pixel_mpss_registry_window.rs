//! Exact successful second admission and publication/completion overlap.
use super::*;
impl RuntimeDomain {
    pub(super) fn show_tftp_completion(&self, display: &Display, owner: &ModemOwner) {
        let window = owner.registry_window_trace();
        let pair = window.target;
        let registry = owner.service_registry_trace();
        let tx4 = registry.txn4;
        let elf = registry.elf;
        let write = owner.original_tftp_write_trace();
        let ack = write.filter(|w| w.data.is_some_and(|d| d.stages.final_prepared
            && d.stages.final_queued && w.final_block == Some(d.block))).map(|w| w.ack);
        let close = write.filter(|w| w.peer.node == 0 && w.peer.port == 0x1f)
            .and_then(|_| owner.init_close_fifo().bounds.and_then(|c| c.first_visible));
        let age = |at| self.time_from_us(Some(self.runtime_started), at);
        let query = self.wlan_query.snapshot(owner);
        display.replace_line(1, "REGISTRY WINDOW TWO");
        for (row, (label, value)) in [
            ("RUNTIME ERROR", self.error),
            ("WINDOW ADMITS", window.admissions),
            ("OLD TXN", pair.map_or(u32::MAX, |p| u32::from(p.older_transaction))),
            ("NEW TXN", pair.map_or(u32::MAX, |p| u32::from(p.newer_transaction))),
            ("PEER NODE", pair.map_or(u32::MAX, |p| p.peer.node)),
            ("PEER PORT", pair.map_or(u32::MAX, |p| p.peer.port)),
            ("NEW QUEUE US", age(pair.and_then(|p| p.queued_at))),
            ("NEW PUB US", age(pair.and_then(|p| p.newer_published_at))),
            ("OLD DONE US", age(pair.and_then(|p| p.older_done_at))),
            ("PUB BEFORE DONE", pair.and_then(|p| p.published_before_done).map_or(u32::MAX, u32::from)),
            ("TX4 RX US", age(tx4.and_then(|t| t.received))),
            ("TX4 QUEUE US", age(tx4.and_then(|t| t.queued))),
            ("TX4 PUB US", age(tx4.and_then(|t| t.published))),
            ("TX4 DONE US", age(tx4.and_then(|t| t.done))),
            ("ELF RX US", age(elf.and_then(|e| e.received))),
            ("ELF QUEUE US", age(elf.and_then(|e| e.queued))),
            ("ELF PUB US", age(elf.and_then(|e| e.published))),
            ("ELF DONE US", age(elf.and_then(|e| e.done))),
            ("ACK PUB US", age(ack.and_then(|a| a.published_at))),
            ("ACK DONE US", age(ack.and_then(|a| a.done_at))),
            ("CLOSE VIS US", age(close)),
            ("ACK TURN", u32::from(self.router.original_final_ack_turn())),
            ("ACK PHASE", ack.map_or(u32::MAX, |a| u32::from(a.phase))),
            ("IPA COMPLETE", u32::from(self.ipa_complete_seen)),
            ("WLAN RRQ RX", owner.tftp_wire_counts()[2]),
            ("WLFW SERVERS", query[7]),
            ("WRITES COMPLETE", self.server_check.completed_count() as u32),
        ].into_iter().enumerate() { display.hex(row + 2, label, value); }
    }
}
