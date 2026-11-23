//! First converted transaction 4 token lifetime. Observation only.
use super::*;
impl RuntimeDomain {
    pub(super) fn show_tftp_completion(&self, display: &Display, owner: &ModemOwner) {
        let window = owner.registry_window_trace();
        let pair = window.target;
        let trace = pair.map(|p| p.conversion);
        let first = trace.and_then(|t| t.first);
        let entry = first.and_then(|t| t.entry);
        let choice = first.and_then(|t| t.choice);
        let pressure = pair.and_then(|p| p.publication);
        let tx4 = owner.service_registry_trace().txn4;
        let age = |at| self.time_from_us(Some(self.runtime_started), at);
        display.replace_line(1, "REGISTRY TX4 TURN LIFE");
        for (row, (label, value)) in [
            ("RUNTIME ERROR", self.error),
            ("TX4 QUEUE US", age(tx4.and_then(|t| t.queued))),
            ("TX4 PUB US", age(tx4.and_then(|t| t.published))),
            ("ARMS", trace.map_or(0, |t| t.arms)),
            ("ARM US", age(first.and_then(|t| t.armed_at))),
            ("ARM BACKING", first.map_or(u32::MAX, |t| t.intent)),
            ("ARM REQUEST", first.map_or(u32::MAX, |t| t.request_intent)),
            ("ARM LEN SENT", first.map_or(u32::MAX, |t| (u32::from(t.bytes) << 16) | u32::from(t.sent))),
            ("ENTRY US", age(entry.and_then(|e| e.at))),
            ("ENTRY FLAGS", entry.map_or(u32::MAX, |e| e.flags)),
            ("ENTRY GATE", entry.and_then(|e| e.gate).map_or(u32::MAX, u32::from)),
            ("ENTRY BACKING", entry.and_then(|e| e.backing).unwrap_or(u32::MAX)),
            ("ENTRY TOKEN REQ", entry.and_then(|e| e.token_request).unwrap_or(u32::MAX)),
            ("CHOICE US", age(choice.and_then(|c| c.at))),
            ("CHOICE TXN CMD", choice.map_or(u32::MAX, |c| (u32::from(c.transaction.unwrap_or(u16::MAX)) << 16) | u32::from(c.command))),
            ("CHOICE PARAM", choice.map_or(u32::MAX, |c| c.parameter)),
            ("WRITE RESULT", choice.map_or(u32::MAX, |c| u32::from(c.result))),
            ("EXIT US", age(first.and_then(|t| t.exit).and_then(|(_, at)| at))),
            ("EXIT CAUSE", first.and_then(|t| t.exit).map_or(0, |(cause, _)| u32::from(cause))),
            ("GRANT AD US", age(first.and_then(|t| t.advertised_at))),
            ("GRANT ACK US", age(first.and_then(|t| t.ack_at))),
            ("TX4 CALLS", pressure.map_or(u32::MAX, |p| p.calls)),
            ("TX4 MASK", pressure.map_or(u32::MAX, |p| p.mask)),
            ("TX4 SNAP STATE", pair.map_or(0, |p| if p.publication_frozen { 2 } else { 1 })),
            ("IPA COMPLETE", u32::from(self.ipa_complete_seen)),
            ("WLAN RRQ RX", owner.tftp_wire_counts()[2]),
            ("WLFW SERVERS", self.wlan_query.snapshot(owner)[7]),
        ].into_iter().enumerate() { display.hex(row + 2, label, value); }
    }
}
