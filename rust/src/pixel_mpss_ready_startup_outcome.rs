//! Reuse retained traces to connect early admission with settled startup outcomes.
//! This renderer adds no samples, transport state, or scheduling authority.
use super::*;
impl RuntimeDomain {
    pub(super) fn show_ready_startup_outcome(&self, d: &Display, owner: &ModemOwner) {
        for n in 2..=28 { d.replace_line(n, ""); }
        d.replace_line(1, "READY STARTUP OUTCOME");
        let age = |t| self.time_from_us(Some(self.runtime_started), t);
        let row = |n, label, values: &[u32]| {
            d.header_label(n, label);
            let mut fields = [(0, 8); 6];
            for (i, value) in values.iter().enumerate() { fields[i] = (*value, 8); }
            d.header_row(n, true, &fields[..values.len()]);
        };
        let s = owner.service_registry_trace();
        let e = s.elf;
        let w = owner.registry_window_trace();
        let a = w.predecessor;
        let b = s.txn4;
        let h = owner.ready_handoff_trace();
        row(2, "ERROR DETAIL CLOSE STAGE", &[self.error, self.detail, owner.pending_mcfg_close().map_or(u32::MAX, |p| p.port), owner.mcfg_close_trace().stage]);
        row(3, "TX3 QUEUE PUB CORE DONE", &[age(a.and_then(|a| a.queued_at)), age(a.and_then(|a| a.published_at)), age(a.and_then(|a| a.done_at))]);
        row(4, "TX4 RX QUEUE PUB OWNER DONE", &[age(b.and_then(|b| b.received)), age(b.and_then(|b| b.queued)), age(b.and_then(|b| b.published)), age(b.and_then(|b| b.done))]);
        row(5, "ELF RX QUEUE PUB OWNER DONE", &[age(e.and_then(|e| e.received)), age(e.and_then(|e| e.queued)), age(e.and_then(|e| e.published)), age(e.and_then(|e| e.done))]);
        let ap = a.and_then(|a| a.publication);
        let bp = w.target.and_then(|b| b.publication);
        row(6, "TX3 SNAP CALLS MASK TX4 SNAP CALLS MASK", &[a.map_or(0, |a| if a.publication_frozen {2} else {1}), ap.map_or(u32::MAX, |a| a.calls), ap.map_or(u32::MAX, |a| a.mask), w.target.map_or(0, |b| if b.publication_frozen {2} else {1}), bp.map_or(u32::MAX, |b| b.calls), bp.map_or(u32::MAX, |b| b.mask)]);
        row(7, "HANDOFF FLAGS ARMS RECEIPTS SERVICES RESULT", &[h.flags, h.arms, h.receipts, h.services, h.result]);
        row(8, "ELF WAITS NONE PEER TXCAP FLOW CREDIT CAP", &e.map_or([0; 6], |e| e.waits_by_reason));
        let c = s.elf_exchange.and_then(|e| e.host_credit);
        row(9, "ELF BODY DONE HOST REQ PUB DONE", &[s.elf_exchange.map_or(u32::MAX, |e| u32::from(e.qmi_matches_reference)), s.elf_exchange.map_or(u32::MAX, |e| u32::from(e.completed)), c.map_or(u32::MAX, |c| u32::from(c.requested)), c.map_or(u32::MAX, |c| u32::from(c.remote_intent.is_some())), c.map_or(u32::MAX, |c| u32::from(c.completed))]);
        let ipa = owner.linux_ipa_report();
        let flags = ipa.and_then(|r| r.protocol.map(|p| u32::from(p.modem_ready) | (u32::from(p.qmi_uc_ready) << 1) | (u32::from(p.indication_requested) << 2) | (u32::from(p.indication_sent) << 3) | (u32::from(r.protocol_complete) << 4) | (u32::from(r.failed) << 5) | (u32::from(r.pending_record) << 6) | (u32::from(r.record_queued) << 7) | (u32::from(r.held_response) << 8) | (u32::from(p.initial_boot) << 9)));
        row(10, "LIVE IPA FLAGS REG DRIVER COMMITS RESP", &[flags.unwrap_or(u32::MAX), ipa.map_or(u32::MAX, |r| r.registration_requests), ipa.map_or(u32::MAX, |r| r.driver_requests), ipa.map_or(u32::MAX, |r| r.publications_committed), ipa.map_or(u32::MAX, |r| r.responses_accepted)]);
        row(11, "IPA PENDING FAILED TIMEOUT IGNORED ID", &[ipa.and_then(|r| r.pending_message_id).map_or(u32::MAX, u32::from), ipa.map_or(u32::MAX, |r| u32::from(r.failed)), ipa.map_or(u32::MAX, |r| r.timeout_causes), ipa.map_or(u32::MAX, |r| r.ignored_unhandled_requests), ipa.and_then(|r| r.last_ignored_unhandled_id).map_or(u32::MAX, u32::from)]);
        let original = owner.original_tftp_write_trace();
        row(12, "ORIG PEER WRQ OACK DATA", &[original.map_or(u32::MAX, |w| w.peer.port), age(original.and_then(|w| w.received_at)), age(original.and_then(|w| w.oack.published_at)), age(original.and_then(|w| w.data).and_then(|d| d.received_at))]);
        row(13, "ORIG ACK PUB DONE CLOSE PHASE BLOCK", &[age(original.and_then(|w| w.ack.published_at)), age(original.and_then(|w| w.ack.done_at)), age(original.and_then(|w| w.timing.first_close_at)), original.map_or(u32::MAX, |w| u32::from(w.ack.phase)), original.and_then(|w| w.final_block).map_or(u32::MAX, u32::from)]);
        let early = owner.startup_prerequisites();
        for (i, r) in early.reads.iter().enumerate() {
            let r = r.as_ref();
            row(14+i*2, "READ KIND PEER FLAGS OP CODE CLIENT", &[r.map_or(u32::MAX, |r| r.kind), r.map_or(u32::MAX, |r| r.peer.port), r.map_or(0, |r| r.flags), r.map_or(u32::MAX, |r| r.opcode), r.map_or(u32::MAX, |r| r.number), r.map_or(u32::MAX, |r| r.client)]);
            row(15+i*2, "READ RX QUEUE PUB DONE CLOSE RESULT", &[age(r.and_then(|r| r.received)), age(r.and_then(|r| r.queued)), age(r.and_then(|r| r.published)), age(r.and_then(|r| r.done)), age(r.and_then(|r| r.close)), r.map_or(u32::MAX, |r| r.result)]);
        }
        let post = s.elf_exchange.and_then(|e| e.first_tftp_request);
        row(22, "POST PATH OP PEER BYTES", &[post.map_or(u32::MAX, |p| match p.path_kind { "SERVER CHECK" => 1, "MCFG TMP" => 2, "OTA FIREWALL RULESET" => 3, "MODEM EFS" => 4, "WLANMDSP MBN" => 5, "OTHER PATH" => 6, _ => 7 }), post.map_or(u32::MAX, |p| u32::from(p.opcode)), post.map_or(u32::MAX, |p| p.source_port), post.map_or(u32::MAX, |p| p.body_bytes)]);
        let cap = owner.post_elf_mcfg();
        let m = cap.and_then(|c| c.mcfg.as_ref());
        let seq = m.and_then(|m| m.record_sequence());
        row(23, "MCFG PEER FLAGS RECORD LO HI OP CODE", &[m.map_or(u32::MAX, |m| m.peer.port), m.map_or(0, |m| m.flags), seq.map_or(u32::MAX, |s| s as u32), seq.map_or(u32::MAX, |s| (s >> 32) as u32), m.map_or(u32::MAX, |m| m.reply_opcode), m.map_or(u32::MAX, |m| m.reply_number)]);
        row(24, "MCFG RX QUEUE PUB DONE CLOSE CANCEL", &[age(m.and_then(|m| m.received)), age(m.and_then(|m| m.queued)), age(m.and_then(|m| m.published)), age(m.and_then(|m| m.done)), age(m.and_then(|m| m.close_at)), age(m.and_then(|m| m.canceled_at))]);
        row(25, "MCFG FILE RESULT PORT CLIENT FIRST LAST", &[m.and_then(|m| m.file_before).unwrap_or(u32::MAX), m.and_then(|m| m.server_result).unwrap_or(u32::MAX), m.map_or(u32::MAX, |m| m.transfer_port), m.and_then(|m| m.first_client).unwrap_or(u32::MAX), m.and_then(|m| m.last_client).unwrap_or(u32::MAX)]);
        let reads = owner.read_sequence();
        row(26, "READ TOTAL MASK RULE EFS OMIT R E", &[reads.total, reads.kinds, early.total[0], early.total[1], early.omitted[0], early.omitted[1]]);
        let (writes, bytes, done) = self.server_check.mcfg_progress();
        let cleanup = owner.mcfg_close_trace();
        row(27, "MCFG WRITES BYTES READS CLOSE OBS COMMIT", &[writes, bytes.map_or(u32::MAX, |n| n as u32), done, cleanup.observed, cleanup.committed]);
        let q = self.wlan_query.snapshot(owner);
        row(28, "HIST IPA PD WLAN WLFW OTHER KIND PEER", &[u32::from(self.ipa_complete_seen), q[1], owner.tftp_wire_counts()[2], q[7], cap.and_then(|c| c.other).map_or(u32::MAX, |o| o[0]), cap.and_then(|c| c.other).map_or(u32::MAX, |o| o[1])]);
    }
}
