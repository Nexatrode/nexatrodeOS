//! Exact original DATA/ACK phases and existing first-ELF admission observations.
use super::*;
impl RuntimeDomain {
    pub(super) fn show_tftp_completion(&self, display: &Display, owner: &ModemOwner) {
        let write = owner.original_tftp_write_trace();
        let data = write.and_then(|w| w.data);
        let stages = data.map(|d| d.stages);
        let ack = write.filter(|w| w.data.is_some_and(|d| d.stages.final_prepared
            && d.stages.final_queued && w.final_block == Some(d.block))).map(|w| w.ack);
        let elf = owner.service_registry_trace().elf;
        let dispatch = elf.map(|e| e.dispatch);
        let turns = dispatch.map(|d| d.turns);
        let blocker = elf.and_then(|e| e.blocker);
        // This separate visibility capture targets modem0:0x1f only.
        let close = write.filter(|w| w.peer.node == 0 && w.peer.port == 0x1f)
            .and_then(|_| owner.init_close_fifo().bounds.and_then(|c| c.first_visible));
        let age = |at| self.time_from_us(Some(self.runtime_started), at);
        let query = self.wlan_query.snapshot(owner);
        display.replace_line(1, "ACK ELF STAGES");
        for (row, (label, value)) in [
            ("RUNTIME ERROR", self.error),
            ("TRACE MATCH", u32::from(write.is_some())),
            ("DATA RX US", age(data.and_then(|d| d.received_at))),
            ("DATA DISP US", age(stages.and_then(|s| s.dispatch_at))),
            ("ACK READY US", age(stages.and_then(|s| s.prepared_at))),
            ("DATA RETURN US", age(stages.and_then(|s| s.returned_at))),
            ("ACK QUEUE US", age(stages.and_then(|s| s.queued_at))),
            ("ACK PUB US", age(ack.and_then(|a| a.published_at))),
            ("ACK DONE US", age(ack.and_then(|a| a.done_at))),
            ("CLOSE VIS US", age(close)),
            ("ELF RX US", age(elf.and_then(|e| e.received))),
            ("ELF RX END US", age(dispatch.and_then(|d| d.receive_pass_end))),
            ("REG ENTRY US", age(dispatch.and_then(|d| d.registry_entered))),
            ("ELF SELECT US", age(dispatch.and_then(|d| d.selected))),
            ("ELF QUEUE US", age(elf.and_then(|e| e.queued))),
            ("ELF PUB US", age(elf.and_then(|e| e.published))),
            ("ELF DONE US", age(elf.and_then(|e| e.done))),
            ("REG RUN 0", turns.map_or(u32::MAX, |t| t.runs[0])),
            ("REG RUN 1", turns.map_or(u32::MAX, |t| t.runs[1])),
            ("REG OMIT", turns.map_or(u32::MAX, |t| t.omitted)),
            ("ELF PEER WAITS", elf.map_or(u32::MAX, |e| e.waits_by_reason[1])),
            ("BLOCK TXN", blocker.map_or(u32::MAX, |b| u32::from(b.transaction))),
            ("BLOCK PUB US", age(blocker.and_then(|b| b.published))),
            ("BLOCK DONE US", age(blocker.and_then(|b| b.done))),
            ("ACK PHASE", ack.map_or(u32::MAX, |a| u32::from(a.phase))),
            ("WLAN RRQ RX", owner.tftp_wire_counts()[2]),
            ("WLFW SERVERS", query[7]),
        ].into_iter().enumerate() { display.hex(row + 2, label, value); }
    }
}
