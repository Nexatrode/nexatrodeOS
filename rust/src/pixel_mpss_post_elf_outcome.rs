//! Retained startup outcomes and bounded MCFG cleanup diagnostics.
use super::*;
impl RuntimeDomain {
    pub(super) fn show_tftp_completion(&self, d: &Display, owner: &ModemOwner) {
        #[cfg(feature="sunfish-startup-prerequisites")]
        {self.show_startup_prerequisites(d,owner);return;}
        #[cfg(feature="sunfish-mcfg-writer-trace")]
        {self.show_mcfg_writer(d,owner);return;}
        for n in 2..=28 { d.replace_line(n, ""); }
        d.replace_line(1, if cfg!(feature="sunfish-mcfg-record-identity"){ "MCFG RECORD IDENTITY" }else if cfg!(feature="sunfish-mcfg-missing-close"){ "MCFG MISSING CLOSE" }else if cfg!(feature="sunfish-mcfg-close-handoff"){ "MCFG CLOSE HANDOFF" }else if cfg!(feature="sunfish-mcfg-close-cleanup"){ "MCFG CLOSE CLEANUP" }else{ "POST ELF OUTCOME" });
        #[cfg(not(feature="sunfish-mcfg-close-handoff"))]
        d.hex(2, "RUNTIME ERROR", self.error);
        let trace = owner.service_registry_trace();
        let elf = trace.elf;
        let exchange = trace.elf_exchange;
        let credit = exchange.and_then(|e| e.host_credit);
        let cap = owner.post_elf_mcfg();
        let m = cap.and_then(|c| c.mcfg.as_ref());
        let age = |t| self.time_from_us(Some(self.runtime_started), t);
        let row = |n, label, values: &[u32]| {
            d.header_label(n, label);
            let mut fields = [(0, 8); 4];
            for (i, value) in values.iter().enumerate() { fields[i] = (*value, 8); }
            d.header_row(n, true, &fields[..values.len()]);
        };
        #[cfg(feature="sunfish-mcfg-close-handoff")]
        row(2,"RUNTIME DETAIL CLOSE STAGE",&[self.error,self.detail,owner.pending_mcfg_close().map_or(u32::MAX,|p|p.port),owner.mcfg_close_trace().stage]);
        row(3, "ELF RX QUEUE PUB DONE", &[age(elf.and_then(|e| e.received)), age(elf.and_then(|e| e.queued)), age(elf.and_then(|e| e.published)), age(elf.and_then(|e| e.done))]);
        row(4, "ELF BODY DONE HOST BEFORE CONF", &[
            exchange.map_or(u32::MAX, |e| u32::from(e.qmi_matches_reference)),
            exchange.map_or(u32::MAX, |e| u32::from(e.completed)),
            credit.and_then(|c| c.before_reply).map_or(u32::MAX, |v| u32::from(v[0]) | (u32::from(v[1]) << 1)),
            exchange.map_or(u32::MAX, |e| e.confirm_rx),
        ]);
        row(5, "HOST REQUESTED PUB DONE INTENT", &[
            credit.map_or(u32::MAX, |c| u32::from(c.requested)),
            credit.map_or(u32::MAX, |c| u32::from(c.remote_intent.is_some())),
            credit.map_or(u32::MAX, |c| u32::from(c.completed)),
            credit.and_then(|c| c.remote_intent).unwrap_or(u32::MAX),
        ]);
        let ipa = owner.linux_ipa_report();
        let flags = ipa.and_then(|r| r.protocol.map(|s|
            u32::from(s.modem_ready) | (u32::from(s.qmi_uc_ready) << 1)
            | (u32::from(s.indication_requested) << 2) | (u32::from(s.indication_sent) << 3)
            | (u32::from(r.protocol_complete) << 4) | (u32::from(r.failed) << 5)
            | (u32::from(r.pending_record) << 6) | (u32::from(r.record_queued) << 7)
            | (u32::from(r.held_response) << 8) | (u32::from(s.initial_boot) << 9)));
        row(6, "IPA FLAGS REG DRIVER COMMITS", &[flags.unwrap_or(u32::MAX), ipa.map_or(u32::MAX, |r| r.registration_requests), ipa.map_or(u32::MAX, |r| r.driver_requests), ipa.map_or(u32::MAX, |r| r.publications_committed)]);
        row(7, "IPA PENDING RESP FAILED TIMEOUT", &[ipa.and_then(|r| r.pending_message_id).map_or(u32::MAX, u32::from), ipa.map_or(u32::MAX, |r| r.responses_accepted), ipa.map_or(u32::MAX, |r| u32::from(r.failed)), ipa.map_or(u32::MAX, |r| r.timeout_causes)]);
        let early = owner.elf_capacity_turn_trace();
        row(8, "EARLY FLAGS SLOT RESULT FULL", &[early.flags, early.free_slot.map_or(u32::MAX, u32::from), u32::from(early.result), early.full_skips]);
        let post = exchange.and_then(|e| e.first_tftp_request);
        row(9, "POST PATH OP PEER BYTES", &[
            post.map_or(u32::MAX, |p| match p.path_kind { "SERVER CHECK" => 1, "MCFG TMP" => 2, "OTA FIREWALL RULESET" => 3, "MODEM EFS" => 4, "WLANMDSP MBN" => 5, "OTHER PATH" => 6, _ => 7 }),
            post.map_or(u32::MAX, |p| u32::from(p.opcode)), post.map_or(u32::MAX, |p| p.source_port), post.map_or(u32::MAX, |p| p.body_bytes),
        ]);
        let original = owner.original_tftp_write_trace();
        row(10, "ORIG ACK ELF DONE PHASE BLOCK", &[age(original.and_then(|w| w.ack.done_at)), age(elf.and_then(|e| e.done)), original.map_or(u32::MAX, |w| u32::from(w.ack.phase)), original.and_then(|w| w.final_block).map_or(u32::MAX, u32::from)]);
        row(11, "MCFG PEER FLAGS FILE RESULT", &[m.map_or(u32::MAX, |m| m.peer.port), m.map_or(0, |m| m.flags), m.and_then(|m| m.file_before).unwrap_or(u32::MAX), m.and_then(|m| m.server_result).unwrap_or(u32::MAX)]);
        row(12, "MCFG OP NUM LEN PORT", &[m.map_or(u32::MAX, |m| m.reply_opcode), m.map_or(u32::MAX, |m| m.reply_number), m.map_or(0, |m| m.reply_bytes), m.map_or(u32::MAX, |m| m.transfer_port)]);
        row(13, "MCFG RX QUEUE PUB DONE", &[age(m.and_then(|m| m.received)), age(m.and_then(|m| m.queued)), age(m.and_then(|m| m.published)), age(m.and_then(|m| m.done))]);
        #[cfg(not(feature="sunfish-mcfg-record-identity"))]
        row(14, "CLIENT FIRST LAST CLOSE REPEAT", &[m.and_then(|m| m.first_client).unwrap_or(u32::MAX), m.and_then(|m| m.last_client).unwrap_or(u32::MAX), age(m.and_then(|m| m.close_at)), m.map_or(0, |m| m.client_counts[2])]);
        #[cfg(feature="sunfish-mcfg-record-identity")]
        {let key=m.and_then(|m|m.record_sequence());row(14,"MCFG CLOSE CANCEL RECORD LO HI",&[age(m.and_then(|m|m.close_at)),age(m.and_then(|m|m.canceled_at)),key.map_or(u32::MAX,|n|n as u32),key.map_or(u32::MAX,|n|(n>>32)as u32)]);}
        #[cfg(not(feature="sunfish-mcfg-close-cleanup"))]
        row(15, "OFFER MASK BLK TIME WIN", &[m.map_or(0, |m| m.options.present), m.map_or(u32::MAX, |m| m.options.values[0]), m.map_or(u32::MAX, |m| m.options.values[1]), m.map_or(u32::MAX, |m| m.options.values[2])]);
        #[cfg(not(feature="sunfish-mcfg-close-cleanup"))]
        row(16, "REPLY MASK BLK WIN TSIZE", &[m.map_or(0, |m| m.reply_options.present), m.map_or(u32::MAX, |m| m.reply_options.values[0]), m.map_or(u32::MAX, |m| m.reply_options.values[2]), m.map_or(u32::MAX, |m| m.reply_options.values[3])]);
        #[cfg(feature="sunfish-mcfg-close-cleanup")]
        {let t=owner.mcfg_close_trace();row(15,"CLOSE OBS COMMIT HELD RETAINED",&[t.observed,t.committed,t.held,t.retained]);
            #[cfg(not(feature="sunfish-mcfg-missing-close"))]
            row(16,"CLOSE UNSENT KEPT PEER US",&[t.unsent,t.published.saturating_add(t.credits),t.peer.map_or(u32::MAX,|p|p.port),age(t.at)]);
            #[cfg(feature="sunfish-mcfg-missing-close")]
            row(16,"CLOSE UNSENT KEPT PEER KIND",&[t.unsent,t.published.saturating_add(t.credits),t.peer.map_or(u32::MAX,|p|p.port),t.kind]);}
        row(17, "JOURNAL TOTAL OMIT BAD QWAIT", &[cap.map_or(0, |c| c.total), cap.map_or(0, |c| c.omitted), cap.map_or(0, |c| c.bad), m.map_or(0, |m| diagnostic::packed_counts(m.queue_waits[0], m.queue_waits[1]))]);
        if let Some(c) = cap {
            for (i, h) in c.journal.iter().enumerate() {
                if let Some(h) = h { d.header_row(18 + i / 2, i % 2 != 0, &[(age(h.at), 8), (u32::from(h.tx), 1), (u32::from(h.kind), 2), (h.source, 8), (h.destination, 8), (u32::from(h.tag), 1), (h.a, 8), (h.b, 8)]); }
            }
        }
        row(26, "OTHER KIND PEER BYTES OP", &cap.and_then(|c| c.other).unwrap_or([u32::MAX; 4]));
        let reads = owner.read_sequence();
        let (writes, bytes, read_done) = self.server_check.mcfg_progress();
        row(27, "READ COUNT MASK MCFG W FILE DONE", &[diagnostic::packed_counts(reads.total, reads.kinds), writes, bytes.map_or(u32::MAX, |n| n as u32), read_done]);
        let q = self.wlan_query.snapshot(owner);
        row(28, "IPA PD STATE WLAN WLFW", &[u32::from(self.ipa_complete_seen), q[1], owner.tftp_wire_counts()[2], q[7]]);
    }
}

#[cfg(feature="sunfish-mcfg-writer-trace")]
impl RuntimeDomain {
    fn show_mcfg_writer(&self,d:&Display,owner:&ModemOwner){
        for n in 2..=28{d.replace_line(n,"");}d.replace_line(1,if cfg!(feature="sunfish-mcfg-idle-detail"){"MCFG IDLE TURN DETAIL"}else if cfg!(feature="sunfish-mcfg-idle-turn"){"MCFG IDLE RETAINED TURN"}else if cfg!(feature="sunfish-mcfg-rx-veto-trace"){"MCFG RX VETO TRACE"}else if cfg!(feature="sunfish-mcfg-queued-turn"){"MCFG QUEUED TURN"}else if cfg!(feature="sunfish-mcfg-predecessor-trace"){"MCFG PREDECESSOR TURN"}else{"MCFG QUEUE WRITER"});
        let age=|t|self.time_from_us(Some(self.runtime_started),t);
        let row=|n,label,values:&[u32]|{d.header_label(n,label);let mut fields=[(0,8);4];for(i,v)in values.iter().enumerate(){fields[i]=(*v,8);}d.header_row(n,true,&fields[..values.len()]);};
        let cap=owner.post_elf_mcfg();let m=cap.and_then(|c|c.mcfg.as_ref());let a=m.map(|m|&m.admission);let seq=m.and_then(|m|m.record_sequence());
        let ct=owner.mcfg_close_trace();row(2,"RUNTIME DETAIL CLOSE STAGE",&[self.error,self.detail,owner.pending_mcfg_close().map_or(u32::MAX,|p|p.port),ct.stage]);
        row(3,"PEER FLAGS RECORD LO HI",&[m.map_or(u32::MAX,|m|m.peer.port),m.map_or(0,|m|m.flags),seq.map_or(u32::MAX,|n|n as u32),seq.map_or(u32::MAX,|n|(n>>32)as u32)]);
        row(4,"RX DISPATCH QUEUE CLOSE",&[age(m.and_then(|m|m.received)),age(m.and_then(|m|m.dispatched)),age(m.and_then(|m|m.queued)),age(m.and_then(|m|m.close_at))]);
        row(5,"PUB DONE CANCEL ATTEMPTS",&[age(m.and_then(|m|m.published)),age(m.and_then(|m|m.done)),age(m.and_then(|m|m.canceled_at)),a.map_or(0,|a|a.attempts)]);
        row(6,"FIRST LAST BUSY CAPACITY",&[age(a.and_then(|a|a.first_attempt)),age(a.and_then(|a|a.last_attempt)),m.map_or(0,|m|m.queue_waits[0]),m.map_or(0,|m|m.queue_waits[1])]);
        row(7,"LAST BLOCK CHECK KIND OP FLAGS",&a.map_or([0;4],|a|a.block));
        let bs=a.and_then(|a|a.block_sequence);row(8,"BLOCK LEN SENT RECORD LO HI",&[a.map_or(0,|a|a.block_len),a.map_or(0,|a|a.block_sent),bs.map_or(u32::MAX,|n|n as u32),bs.map_or(u32::MAX,|n|(n>>32)as u32)]);
        #[cfg(not(feature="sunfish-mcfg-predecessor-trace"))]
        row(9,"BLOCK US PASSES OMIT FILE",&[age(a.and_then(|a|a.block_at)),a.map_or(0,|a|a.passes),a.map_or(0,|a|a.omitted),m.and_then(|m|m.file_before).unwrap_or(u32::MAX)]);
        #[cfg(feature="sunfish-mcfg-predecessor-trace")]
        row(9,"BLOCK PUB DONE PASSES",&[age(a.and_then(|a|a.block_at)),age(a.and_then(|a|a.block_published)),age(a.and_then(|a|a.block_completed)),a.map_or(0,|a|a.passes)]);
        let trace=owner.service_registry_trace();let elf=trace.elf;row(10,"ELF RX QUEUE PUB DONE",&[age(elf.and_then(|e|e.received)),age(elf.and_then(|e|e.queued)),age(elf.and_then(|e|e.published)),age(elf.and_then(|e|e.done))]);
        let ipa=owner.linux_ipa_report();row(11,"IPA REG DRIVER COMMITS COMPLETE",&[ipa.map_or(u32::MAX,|r|r.registration_requests),ipa.map_or(u32::MAX,|r|r.driver_requests),ipa.map_or(u32::MAX,|r|r.publications_committed),u32::from(self.ipa_complete_seen)]);
        row(12,"PENDING RESP FAILED TIMEOUT",&[ipa.and_then(|r|r.pending_message_id).map_or(u32::MAX,u32::from),ipa.map_or(u32::MAX,|r|r.responses_accepted),ipa.map_or(u32::MAX,|r|u32::from(r.failed)),ipa.map_or(u32::MAX,|r|r.timeout_causes)]);
        #[cfg(not(feature="sunfish-mcfg-predecessor-trace"))]
        row(13,"ORIG ACK HOST REQ PUB DONE",&[age(owner.original_tftp_write_trace().and_then(|w|w.ack.done_at)),trace.elf_exchange.and_then(|e|e.host_credit).map_or(u32::MAX,|c|u32::from(c.requested)),trace.elf_exchange.and_then(|e|e.host_credit).map_or(u32::MAX,|c|u32::from(c.remote_intent.is_some())),trace.elf_exchange.and_then(|e|e.host_credit).map_or(u32::MAX,|c|u32::from(c.completed))]);
        #[cfg(feature="sunfish-mcfg-predecessor-trace")]
        {let c=trace.elf_exchange.and_then(|e|e.host_credit);row(13,"ORIG ACK HOST FILE RESULT",&[age(owner.original_tftp_write_trace().and_then(|w|w.ack.done_at)),c.map_or(u32::MAX,|c|u32::from(c.requested)|(u32::from(c.remote_intent.is_some())<<1)|(u32::from(c.completed)<<2)),m.and_then(|m|m.file_before).unwrap_or(u32::MAX),m.and_then(|m|m.server_result).unwrap_or(u32::MAX)]);}
        #[cfg(feature="sunfish-mcfg-idle-detail")]
        {self.show_idle_detail_rows(d,owner);return;}
        #[cfg(feature="sunfish-mcfg-rx-veto-trace")]
        {self.show_mcfg_rx_rows(d,owner);return;}
        d.header_label(14,if cfg!(feature="sunfish-mcfg-predecessor-trace"){"START END GATES CMD RECORD LO HI"}else{"START END GATES CMD; ARG Q S D R"});
        if let Some(a)=a{for(i,r)in a.records.iter().enumerate(){if let Some(r)=r{#[cfg(not(feature="sunfish-mcfg-predecessor-trace"))]
        d.header_row(15+i,false,&[(age(r.start),8),(age(r.end),8),(r.gates,8),(r.command,2)]);
        #[cfg(feature="sunfish-mcfg-predecessor-trace")]
        d.header_row(15+i,false,&[(age(r.start),8),(age(r.end),8),(r.gates,8),(r.command,2),(r.record_sequence.map_or(u32::MAX,|n|n as u32),8),(r.record_sequence.map_or(u32::MAX,|n|(n>>32)as u32),8)]);#[cfg(not(feature="sunfish-mcfg-predecessor-trace"))]
        d.header_row(15+i,true,&[(r.parameter,8),(r.kind,2),(r.source,8),(r.destination,8),(r.result,1)]);
        #[cfg(feature="sunfish-mcfg-predecessor-trace")]
        d.header_row(15+i,true,&[(r.parameter,8),(r.kind,2),(r.source,8),(r.destination,8),(r.result,1),(r.grant_size,8)]);}}}
        row(23,"CLOSE OBS COMMIT UNSENT KEPT",&[ct.observed,ct.committed,ct.unsent,ct.published.saturating_add(ct.credits)]);
        let(w,bytes,done)=self.server_check.mcfg_progress();row(24,"MCFG WRITES FILE READS STAGE",&[w,bytes.map_or(u32::MAX,|n|n as u32),done,ct.stage]);
        #[cfg(not(feature="sunfish-mcfg-queued-turn"))]
        row(25,"OTHER KIND PEER BYTES OP",&cap.and_then(|c|c.other).unwrap_or([u32::MAX;4]));
        #[cfg(feature="sunfish-mcfg-queued-turn")]
        {let t=owner.mcfg_queued_turn_trace();row(25,"TURN CALLS WRITES RX VETO BLOCK",&[t.calls,t.writes,t.rx_veto,t.blocked]);}
        row(26,"REPLY OP CODE BYTES PORT",&[m.map_or(u32::MAX,|m|m.reply_opcode),m.map_or(u32::MAX,|m|m.reply_number),m.map_or(0,|m|m.reply_bytes),m.map_or(u32::MAX,|m|m.transfer_port)]);
        let reads=owner.read_sequence();
        #[cfg(not(feature="sunfish-mcfg-predecessor-trace"))]
        row(27,"READ TOTAL MASK JOURNAL OMIT",&[reads.total,reads.kinds,cap.map_or(0,|c|c.total),cap.map_or(0,|c|c.omitted)]);
        #[cfg(feature="sunfish-mcfg-predecessor-trace")]
        row(27,"READ TOTAL MASK PASSES OMIT",&[reads.total,reads.kinds,a.map_or(0,|a|a.passes),a.map_or(0,|a|a.omitted)]);
        let q=self.wlan_query.snapshot(owner);row(28,"IPA PD STATE WLAN WLFW",&[u32::from(self.ipa_complete_seen),q[1],owner.tftp_wire_counts()[2],q[7]]);
    }
}

#[cfg(feature="sunfish-mcfg-rx-veto-trace")]
impl RuntimeDomain{
 fn show_mcfg_rx_rows(&self,d:&Display,owner:&ModemOwner){
  let trace=owner.mcfg_rx_veto_trace();let t=owner.mcfg_queued_turn_trace();
  #[cfg(feature="sunfish-mcfg-idle-turn")]let t=owner.mcfg_idle_turn_trace();
  let age=|at|self.time_from_us(Some(self.runtime_started),at);
  d.header_label(14,if cfg!(feature="sunfish-mcfg-idle-turn"){"TOTAL OMIT IDLE WRITE RX BLOCK"}else{"TOTAL OMIT CALL WRITE RX BLOCK"});d.header_row(14,true,&[(trace.total,8),(trace.omitted,8),(t.calls,8),(t.writes,8),(t.rx_veto,8),(t.blocked,8)]);
  for(i,e)in trace.records.iter().enumerate(){if let Some(e)=e{
   d.header_row(15+i,false,&[(age(e.at),8),(e.tag,1),(e.call,8),(e.unread,8),(e.early,2),(e.watched_sequence.map_or(u32::MAX,|v|v as u32),8),(e.watched_sequence.map_or(u32::MAX,|v|(v>>32)as u32),8)]);
   let h=e.header;let q=h.and_then(|h|h.qrtr);let endpoints=h.and_then(|h|h.del_client).or(q.map(|q|[q[1],q[2]])).unwrap_or([u32::MAX;2]);
   d.header_row(15+i,true,&[(h.map_or(u32::MAX,|h|u32::from(h.command)),4),(h.map_or(u32::MAX,|h|u32::from(h.channel)),4),(h.map_or(u32::MAX,|h|h.parameter),8),(h.map_or(u32::MAX,|h|u32::from(h.bytes)),4),(q.map_or(u32::MAX,|q|q[0]),2),(endpoints[0],8),(endpoints[1],8)]);
  }}
  let c=owner.mcfg_close_trace();let(w,_,read)=self.server_check.mcfg_progress();d.header_label(27,"CLOSE OBS COMMIT MCFG W READS");d.header_row(27,true,&[(c.observed,8),(c.committed,8),(w,8),(read,8)]);
  let q=self.wlan_query.snapshot(owner);d.header_label(28,"IPA PD STATE WLAN WLFW");d.header_row(28,true,&[(u32::from(self.ipa_complete_seen),8),(q[1],8),(owner.tftp_wire_counts()[2],8),(q[7],8)]);
 }
}

#[cfg(feature="sunfish-mcfg-idle-detail")]
impl RuntimeDomain{
 fn show_idle_detail_rows(&self,d:&Display,owner:&ModemOwner){
  let c=owner.mcfg_idle_detail();let age=|at|self.time_from_us(Some(self.runtime_started),at);
  d.header_label(13,"IDLE PEER LOCAL BODY SLOT");d.header_row(13,true,&c.meta.map(|v|(v,8)));
  d.header_label(14,"CALL START END WRITE RX BLOCK");d.header_row(14,true,&[(c.call,8),(age(c.start),8),(age(c.end),8),(c.summary[0],8),(c.summary[1],8),(c.summary[2],8)]);
  for(i,e)in c.attempts.iter().enumerate(){if let Some(e)=e{let h=e.header;let q=h.and_then(|h|h.qrtr);let ports=e.advertisement.or(q.map(|q|[q[1],q[2]])).unwrap_or([u32::MAX;2]);
   d.header_row(15+i,false,&[(age(e.start),8),(age(e.end),8),(h.map_or(u32::MAX,|h|u32::from(h.command)),4),(h.map_or(u32::MAX,|h|u32::from(h.channel)),4),(h.map_or(u32::MAX,|h|h.parameter),8)]);
   d.header_row(15+i,true,&[(e.result,1),(h.map_or(u32::MAX,|h|u32::from(h.bytes)),4),(q.map_or(u32::MAX,|q|q[0]),2),(ports[0],8),(ports[1],8),(e.unread,8)]);
  }}
  for(i,e)in c.rx.iter().enumerate(){if let Some(e)=e{let h=e.header;let q=h.and_then(|h|h.qrtr);let ports=h.and_then(|h|h.del_client).or(e.advertisement).or(q.map(|q|[q[1],q[2]])).unwrap_or([u32::MAX;2]);
   d.header_row(19+i,false,&[(age(e.at),8),(e.tag,1),(e.unread,8),(e.early,2)]);
   d.header_row(19+i,true,&[(h.map_or(u32::MAX,|h|u32::from(h.command)),4),(h.map_or(u32::MAX,|h|u32::from(h.channel)),4),(h.map_or(u32::MAX,|h|h.parameter),8),(h.map_or(u32::MAX,|h|u32::from(h.bytes)),4),(q.map_or(u32::MAX,|q|q[0]),2),(ports[0],8),(ports[1],8)]);
  }}
  let cl=owner.mcfg_close_trace();d.header_label(27,"RX TOTAL OMIT CLOSE OBS COMMIT");d.header_row(27,true,&[(c.rx_total,8),(c.rx_omitted,8),(cl.observed,8),(cl.committed,8)]);
  let q=self.wlan_query.snapshot(owner);d.header_label(28,"IPA PD STATE WLAN WLFW");d.header_row(28,true,&[(u32::from(self.ipa_complete_seen),8),(q[1],8),(owner.tftp_wire_counts()[2],8),(q[7],8)]);
 }
}

#[cfg(feature="sunfish-startup-prerequisites")]
impl RuntimeDomain{
 fn show_startup_prerequisites(&self,d:&Display,owner:&ModemOwner){
  let age=|at|self.time_from_us(Some(self.runtime_started),at);
  let row=|n,label,values:&[u32]|{d.header_label(n,label);let mut fields=[(0,8);6];for(i,v)in values.iter().enumerate(){fields[i]=(*v,8);}d.header_row(n,true,&fields[..values.len()]);};
  for n in 2..=28{d.replace_line(n,"");}d.replace_line(1,if cfg!(feature="sunfish-efs-reply-overlap"){"EFS REPLY OVERLAP"}else if cfg!(feature="sunfish-efs-service-handoff"){"EFS SERVICE HANDOFF"}else if cfg!(feature="sunfish-efs-early-error-turn"){"MODEM EFS EARLY ERROR"}else if cfg!(feature="sunfish-efs-reply-fifo"){"MODEM EFS REPLY FIFO"}else if cfg!(feature="sunfish-tftp-arrival-order"){"EARLY TFTP ARRIVAL ORDER"}else{"EARLY STARTUP REPLIES"});
  let close=owner.mcfg_close_trace();row(2,"ERROR DETAIL CLOSE STAGE",&[self.error,self.detail,owner.pending_mcfg_close().map_or(u32::MAX,|p|p.port),close.stage]);
  let first=owner.original_tftp_write_trace();row(3,"ORIG PEER WRQ OACK DATA",&[first.map_or(u32::MAX,|w|w.peer.port),age(first.and_then(|w|w.received_at)),age(first.and_then(|w|w.oack.published_at)),age(first.and_then(|w|w.data).and_then(|w|w.received_at))]);
  row(4,"ACK PHASE PUB DONE CLOSE",&[first.map_or(u32::MAX,|w|u32::from(w.ack.phase)),age(first.and_then(|w|w.ack.published_at)),age(first.and_then(|w|w.ack.done_at)),age(first.and_then(|w|w.timing.first_close_at))]);
  let t=owner.service_registry_trace();let elf=t.elf;row(5,"ELF RX QUEUE PUB DONE",&[age(elf.and_then(|e|e.received)),age(elf.and_then(|e|e.queued)),age(elf.and_then(|e|e.published)),age(elf.and_then(|e|e.done))]);
  let e=t.elf_exchange;let c=e.and_then(|e|e.host_credit);row(6,"ELF MATCH CREDIT LOC Q DONE",&[e.map_or(u32::MAX,|e|u32::from(e.qmi_matches_reference)),c.map_or(u32::MAX,|c|u32::from(c.requested)|(u32::from(c.remote_intent.is_some())<<1)|(u32::from(c.completed)<<2)),t.queued_names,t.completed_names]);
  let q=self.wlan_query.snapshot(owner);row(7,"IPA PD STATE WLAN WLFW",&[u32::from(self.ipa_complete_seen),q[1],owner.tftp_wire_counts()[2],q[7]]);
  let p=owner.startup_prerequisites();for(i,r)in p.reads.iter().enumerate(){let n=8+i*5;let r=r.as_ref();let seq=r.and_then(|r|r.record_sequence());
   row(n,"K PEER FLAGS RECORD LO HI",&[r.map_or(u32::MAX,|r|r.kind),r.map_or(u32::MAX,|r|r.peer.port),r.map_or(0,|r|r.flags),seq.map_or(u32::MAX,|s|s as u32),seq.map_or(u32::MAX,|s|(s>>32)as u32)]);
   row(n+1,"BYTES RSIZE OPTS ERR OPT CLIENT",&[r.map_or(u32::MAX,|r|r.request_bytes),r.map_or(u32::MAX,|r|r.rsize),r.map_or(0,|r|r.options_present),r.map_or(u32::MAX,|r|r.result),r.map_or(0,|r|r.options_flags),r.map_or(u32::MAX,|r|r.client)]);
   row(n+2,"RX DISPATCH QUEUE",&[age(r.and_then(|r|r.received)),age(r.and_then(|r|r.dispatched)),age(r.and_then(|r|r.queued))]);
   row(n+3,"PUB DONE CLOSE",&[age(r.and_then(|r|r.published)),age(r.and_then(|r|r.done)),age(r.and_then(|r|r.close))]);
   d.header_label(n+4,"OP CODE LEN PORT TRY BUSY CAP");d.header_row(n+4,true,&[(r.map_or(u32::MAX,|r|r.opcode),4),(r.map_or(u32::MAX,|r|r.number),4),(r.map_or(0,|r|r.reply_bytes),4),(r.map_or(u32::MAX,|r|r.local_port),8),(r.map_or(0,|r|r.attempts),4),(r.map_or(0,|r|r.waits[0]),4),(r.map_or(0,|r|r.waits[1]),4)]);
  }
  #[cfg(feature="sunfish-efs-reply-fifo")]
  {
   let w=owner.efs_reply_fifo();let c=w.and_then(|w|w.fifo.completion);
   row(13,if cfg!(feature="sunfish-efs-reply-overlap"){"EFS2 KEY LO HI INTENT ARM"}else{"EFS KEY LO HI INTENT ARM"},&[w.map_or(u32::MAX,|w|w.record as u32),w.map_or(u32::MAX,|w|(w.record>>32)as u32),w.map_or(u32::MAX,|w|w.intent),age(w.and_then(|w|w.armed))]);
   row(14,"PEER TAIL RX ARM RX CLOSE",&[age(w.and_then(|w|w.fifo.peer_tail_at)),w.map_or(u32::MAX,|w|w.unread_at_arm),w.and_then(|w|w.unread_at_close).unwrap_or(u32::MAX)]);
   row(15,"RX LAST ABSENT FIRST VISIBLE",&[age(c.and_then(|c|c.last_absent)),age(c.and_then(|c|c.first_visible)),c.and_then(|c|c.bytes_ahead).unwrap_or(u32::MAX)]);
   #[cfg(not(feature="sunfish-efs-early-error-turn"))]
   {   row(16,"VISIBLE CLOSE FINISH EVICT",&[c.and_then(|c|c.visible_at_close).map_or(u32::MAX,u32::from),w.map_or(0,|w|u32::from(w.fifo.finished)),w.map_or(0,|w|w.fifo.history_evictions)]);
   let r=p.reads[2];row(17,"EFS OP CODE LEN TRACKED",&[r.map_or(u32::MAX,|r|r.opcode),r.map_or(u32::MAX,|r|r.number),r.map_or(u32::MAX,|r|r.reply_bytes),u32::from(w.is_some())]);}
   #[cfg(feature="sunfish-efs-early-error-turn")]
   {let t=owner.efs_error_turn_trace();
    #[cfg(not(feature="sunfish-efs-service-handoff"))]
    {row(16,"VISIBLE CLOSE FIN EVICT STOP",&[c.and_then(|c|c.visible_at_close).map_or(u32::MAX,u32::from),w.map_or(0,|w|u32::from(w.fifo.finished)),w.map_or(0,|w|w.fifo.history_evictions),t.stop]);row(17,"EARLY CALL WRITE RX BLOCK",&[t.calls,t.writes,t.rx_veto,t.blocked]);}
    #[cfg(feature="sunfish-efs-service-handoff")]
    {let service=owner.efs_service_trace();let commands=t.commands.iter().enumerate().fold(0u32,|v,(i,c)|v|((u32::from(*c)&255)<<(i*8)));let values=[c.and_then(|c|c.visible_at_close).map_or(u32::MAX,u32::from),w.map_or(0,|w|u32::from(w.fifo.finished)),w.map_or(0,|w|w.fifo.history_evictions),t.stop,commands];
     #[cfg(not(feature="sunfish-efs-reply-overlap"))]
     row(16,"VISIBLE CLOSE FIN EVICT STOP CMDS",&values);
     #[cfg(feature="sunfish-efs-reply-overlap")]
     {let pair=owner.efs_overlap_trace();let packed=(pair.flags&255)|((pair.admitted&255)<<8)|((pair.offers&65535)<<16);row(16,"VISIBLE CLOSE FIN EVICT STOP CMDS PAIR",&[values[0],values[1],values[2],values[3],values[4],packed]);}row(17,"EARLY CALL WRITE RX BLOCK SVC STATE",&[t.calls,t.writes,t.rx_veto,t.blocked,service.calls,service.outcome]);}
   }

  }
  let reads=owner.read_sequence();row(28,"READ MASK RULE EFS OMIT R E",&[diagnostic::packed_counts(reads.total,reads.kinds),p.total[0],p.total[1],p.omitted[0],p.omitted[1]]);
 }
}
