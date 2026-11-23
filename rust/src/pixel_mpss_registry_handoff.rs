//! Same-clock predecessor publication and bounded registry retry outcomes.
use super::*;
impl RuntimeDomain {
    pub(super) fn show_tftp_completion(&self, display: &Display, owner: &ModemOwner) {
        let window = owner.registry_window_trace();
        let prior = window.predecessor;
        let handoff = owner.registry_handoff_trace();
        let target = handoff.target;
        let registry = owner.service_registry_trace();
        let tx4 = registry.txn4;
        let elf = registry.elf;
        let ack = owner.original_tftp_write_trace().filter(|w| w.data.is_some_and(|d|
            d.stages.final_prepared && d.stages.final_queued && w.final_block == Some(d.block))).map(|w| w.ack);
        let age = |at| self.time_from_us(Some(self.runtime_started), at);
        let query = self.wlan_query.snapshot(owner);
        display.replace_line(1, "REGISTRY HANDOFF");
        for (row, (label, value)) in [
            ("RUNTIME ERROR", self.error),
            ("HANDOFFS", handoff.attempts),
            ("HANDOFF MATCH", u32::from(target.is_some())),
            ("OLD QUEUE US", age(prior.and_then(|p| p.queued_at))),
            ("OLD PUB US", age(prior.and_then(|p| p.published_at))),
            ("WRITER START US", age(target.and_then(|t| t.writer_at))),
            ("WRITER END US", age(target.and_then(|t| t.writer_end))),
            ("RETRY READY US", age(target.and_then(|t| t.ready_at))),
            ("RETRY START US", age(target.and_then(|t| t.retry_at))),
            ("RETRY END US", age(target.and_then(|t| t.retry_end))),
            ("RETRY RESULT", target.map_or(u32::MAX, |t| u32::from(t.result))),
            ("TX4 RX US", age(tx4.and_then(|t| t.received))),
            ("TX4 QUEUE US", age(tx4.and_then(|t| t.queued))),
            ("TX4 PUB US", age(tx4.and_then(|t| t.published))),
            ("TX4 DONE US", age(tx4.and_then(|t| t.done))),
            ("ELF RX US", age(elf.and_then(|e| e.received))),
            ("ELF QUEUE US", age(elf.and_then(|e| e.queued))),
            ("ELF PUB US", age(elf.and_then(|e| e.published))),
            ("ELF DONE US", age(elf.and_then(|e| e.done))),
            ("PUB BEFORE DONE", window.target.and_then(|p| p.published_before_done).map_or(u32::MAX, u32::from)),
            ("ACK PHASE", ack.map_or(u32::MAX, |a| u32::from(a.phase))),
            ("IPA COMPLETE", u32::from(self.ipa_complete_seen)),
            ("WLAN RRQ RX", owner.tftp_wire_counts()[2]),
            ("WLFW SERVERS", query[7]),
            ("WRITES COMPLETE", self.server_check.completed_count() as u32),
            ("OLD DONE US", age(prior.and_then(|p| p.done_at))),
            ("WINDOW ADMITS", window.admissions),
        ].into_iter().enumerate() { display.hex(row + 2, label, value); }
    }
}
