//! Presentation only: original write and retained observations after ELF.
use super::*;
impl RuntimeDomain {
    pub(super) fn show_tftp_completion(&self, display: &Display, owner: &ModemOwner) {
        let trace = owner.service_registry_trace();
        let elf = trace.elf;
        let exchange = trace.elf_exchange;
        let first = owner.original_tftp_write_trace();
        let completed = owner.tftp_final_ack_trace();
        let post = exchange.and_then(|e| e.first_tftp_request);
        let reads = owner.read_sequence();
        let age = |at| self.time_from_us(Some(self.runtime_started), at);
        for row in 2..=28 { display.replace_line(row, ""); }
        display.replace_line(1, "POST ELF STARTUP");
        display.hex(2, "RUNTIME ERROR", self.error);
        display.replace_line(3, "ELF R Q P D");
        display.wire_row(3, 3, &[
            (age(elf.and_then(|e| e.received)), 8), (age(elf.and_then(|e| e.queued)), 8),
            (age(elf.and_then(|e| e.published)), 8), (age(elf.and_then(|e| e.done)), 8)]);
        display.hex(4, "ELF BODY MATCH", exchange.map_or(u32::MAX, |e| u32::from(e.qmi_matches_reference)));
        display.hex(5, "ORIG WRQ PEER", first.map_or(u32::MAX, |w| w.peer.port));
        display.hex(6, "ORIG WRQ US", age(first.and_then(|w| w.received_at)));
        display.hex(7, "ORIG OACK US", age(first.and_then(|w| w.oack.published_at)));
        display.hex(8, "ORIG DATA US", age(first.and_then(|w| w.data).and_then(|d| d.received_at)));
        display.hex(9, "ACK PHASE BLOCK", first.map_or(u32::MAX, |w|
            (u32::from(w.ack.phase) << 16) | w.final_block.map_or(0xffff, u32::from)));
        display.hex(10, "ORIG ACK PUB US", age(first.and_then(|w| w.ack.published_at)));
        display.hex(11, "ORIG ACK DONE", age(first.and_then(|w| w.ack.done_at)));
        display.hex(12, "ORIG CLOSE US", age(first.and_then(|w| w.timing.first_close_at)));
        display.replace_line(13, "WRITES DONE PEER FLAGS");
        display.wire_row(13, 3, &[(self.server_check.completed_count() as u32, 8),
            (completed.map_or(u32::MAX, |a| a.destination.port), 8),
            (completed.map_or(0, |a| a.flags), 8)]);
        let path = post.map_or(u32::MAX, |p| match p.path_kind {
            "SERVER CHECK" => 1, "MCFG TMP" => 2, "OTA FIREWALL RULESET" => 3,
            "MODEM EFS" => 4, "WLANMDSP MBN" => 5, "OTHER PATH" => 6, _ => 7,
        });
        display.hex(14, "POST ELF PATH", path);
        display.replace_line(15, "POST OP PEER DST LEN");
        display.wire_row(15, 3, &[(post.map_or(u32::MAX, |p| u32::from(p.opcode)), 8),
            (post.map_or(u32::MAX, |p| p.source_port), 8),
            (post.map_or(u32::MAX, |p| p.destination_port), 8),
            (post.map_or(u32::MAX, |p| p.body_bytes), 8)]);
        display.hex(16, "ELF COMPLETED", exchange.map_or(u32::MAX, |e| u32::from(e.completed)));
        let (writes, bytes, read_done) = self.server_check.mcfg_progress();
        display.replace_line(17, "MCFG W FILE READ");
        display.wire_row(17, 3, &[(writes, 8), (bytes.map_or(u32::MAX, |b| b as u32), 8), (read_done, 8)]);
        display.hex(18, "READ COUNT MASK", diagnostic::packed_counts(reads.total, reads.kinds));
        display.replace_line(19, "POST QRTR META SRC DST");
        if let Some(e) = exchange {
            for (i, h) in e.following.into_iter().enumerate() {
                let values = h.map_or([u32::MAX; 3], |h| [h.meta, h.source_port, h.destination_port]);
                display.wire_row(20, i, &[(values[0], 8), (values[1], 8), (values[2], 8)]);
            }
        }
        display.replace_line(21, "READ US PEER KIND");
        for (i, r) in reads.first.into_iter().enumerate() {
            let (kind, peer, at) = r.unwrap_or((u32::MAX, u32::MAX, None));
            display.wire_row(22 + i / 4, i % 4, &[(age(at), 8), (peer, 8), (kind, 8)]);
        }
        let q = self.wlan_query.snapshot(owner);
        display.hex(24, "QUERY PHASE", q[0]);
        display.hex(25, "PD QUERY STATE", q[1]);
        display.hex(26, "IPA COMPLETE", u32::from(self.ipa_complete_seen));
        display.hex(27, "WLAN RRQ RX", owner.tftp_wire_counts()[2]);
        display.hex(28, "WLFW SERVERS", q[7]);
    }
}
