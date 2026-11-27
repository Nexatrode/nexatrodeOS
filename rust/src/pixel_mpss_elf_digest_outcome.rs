//! Existing exact ELF writer observations beside two identity-bound digest replies.
use super::*;
impl RuntimeDomain {
    pub(super) fn show_elf_digest_outcome(&self,d:&Display,owner:&ModemOwner){
        for n in 2..=28{d.replace_line(n,"");}d.replace_line(1,"ELF WRITER DIGEST REPLY");
        let age=|t|self.time_from_us(Some(self.runtime_started),t);
        let span=|t|self.time_from_us(Some(0),t);
        let row=|n,label,v:&[u32]|{d.header_label(n,label);let mut f=[(0,8);6];for(i,v)in v.iter().enumerate(){f[i]=(*v,8);}d.header_row(n,true,&f[..v.len()]);};
        let s=owner.service_registry_trace();let e=s.elf;let w=owner.elf_window_trace();let writer=w.map(|w|&w.writer);let p=writer.and_then(|w|w.publication);let turn=owner.elf_queued_turn_trace();
        let old=owner.registry_window_trace().predecessor;let tx4=s.txn4;
        row(2,"ERROR DETAIL CLOSE STAGE",&[self.error,self.detail,owner.pending_mcfg_close().map_or(u32::MAX,|p|p.port),owner.mcfg_close_trace().stage]);
        row(3,"TX3 Q PUB CORE TX4 Q PUB OWNER",&[age(old.and_then(|p|p.queued_at)),age(old.and_then(|p|p.published_at)),age(old.and_then(|p|p.done_at)),age(tx4.and_then(|t|t.queued)),age(tx4.and_then(|t|t.published)),age(tx4.and_then(|t|t.done))]);
        row(4,"ELF RX Q PUB DONE TURN FLAGS TX",&[age(e.and_then(|e|e.received)),age(e.and_then(|e|e.queued)),age(e.and_then(|e|e.published)),age(e.and_then(|e|e.done)),turn.flags,turn.published]);
        row(5,"WRITER FIRST LAST PASSES OMIT US MAXGAP",&[age(writer.and_then(|w|w.first)),age(writer.and_then(|w|w.last)),writer.map_or(0,|w|w.passes),writer.map_or(0,|w|w.omitted),span(writer.and_then(|w|w.total)),span(writer.and_then(|w|w.max_gap))]);
        row(6,"PUB CALLS MASK TXN BYTES FLAGS TIMED",&[p.map_or(0,|p|p.calls),p.map_or(0,|p|p.mask),p.map_or(u32::MAX,|p|u32::from(p.transaction)),p.map_or(u32::MAX,|p|u32::from(p.bytes)),writer.map_or(0,|w|u32::from(w.finished)|(u32::from(w.active)<<1)|(u32::from(w.regressed)<<2)),writer.map_or(0,|w|w.timed)]);
        let g=p.map_or([0;8],|p|p.turn_gates);row(7,"TURN GATES 0 1 2 3",&g[..4]);row(8,"TURN GATES 4 5 6 7",&g[4..]);
        let g=p.map_or([0;12],|p|p.grant_gates);row(9,"GRANT GATES 0 1 2 3",&g[..4]);row(10,"GRANT GATES 4 5 6 7",&g[4..8]);row(11,"GRANT GATES 8 9 A B",&g[8..]);
        for i in 0..8{let r=writer.and_then(|w|w.records[i]);row(12+i,"START END CMDKFTXN PARAM RESULT",&[age(r.and_then(|r|r.start)),age(r.and_then(|r|r.end)),r.map_or(u32::MAX,|r|(u32::from(r.command.min(255))<<24)|(u32::from(r.kind.min(15))<<20)|(u32::from(r.flags&15)<<16)|u32::from(r.transaction)),r.map_or(u32::MAX,|r|r.parameter),r.map_or(u32::MAX,|r|u32::from(r.result))]);}
        let early=owner.startup_prerequisites();
        for i in 0..2{let r=early.reads[4+i];let seq=r.and_then(|r|r.record_sequence());
            row(20+i*3,"DIGEST PEER FLAGS RECORD LO HI RESULT",&[r.map_or(u32::MAX,|r|r.peer.port),r.map_or(0,|r|r.flags),seq.map_or(u32::MAX,|s|s as u32),seq.map_or(u32::MAX,|s|(s>>32)as u32),r.map_or(u32::MAX,|r|r.result)]);
            row(21+i*3,"DIGEST RX QUEUE PUB DONE CLOSE",&[age(r.and_then(|r|r.received)),age(r.and_then(|r|r.queued)),age(r.and_then(|r|r.published)),age(r.and_then(|r|r.done)),age(r.and_then(|r|r.close))]);
            row(22+i*3,"DIGEST OP CODE BYTES PORT INTENT CLIENT",&[r.map_or(u32::MAX,|r|r.opcode),r.map_or(u32::MAX,|r|r.number),r.map_or(0,|r|r.reply_bytes),r.map_or(u32::MAX,|r|r.local_port),r.map_or(u32::MAX,|r|r.remote_intent),r.map_or(u32::MAX,|r|r.client)]);
        }
        row(26,"ACK SENT BLOCKED DONE ADS TURN ENTRY END",&[p.map_or(0,|p|p.ack.sent),p.map_or(0,|p|p.ack.blocked),p.map_or(0,|p|p.ack.done_ahead),p.map_or(0,|p|p.ack.ads_ahead),age(turn.entry),age(turn.end)]);
        let ipa=owner.linux_ipa_report();let flags=ipa.and_then(|r|r.protocol.map(|p|u32::from(p.modem_ready)|(u32::from(p.qmi_uc_ready)<<1)|(u32::from(p.indication_requested)<<2)|(u32::from(p.indication_sent)<<3)|(u32::from(r.protocol_complete)<<4)|(u32::from(r.failed)<<5)|(u32::from(r.pending_record)<<6)|(u32::from(r.record_queued)<<7)|(u32::from(r.held_response)<<8)|(u32::from(p.initial_boot)<<9)));
        let credit=s.elf_exchange.and_then(|e|e.host_credit);row(27,"IPA FLAGS PENDING CREDIT DIGEST N OMIT CLOSE",&[flags.unwrap_or(u32::MAX),ipa.and_then(|r|r.pending_message_id).map_or(u32::MAX,u32::from),credit.map_or(u32::MAX,|c|u32::from(c.requested)|(u32::from(c.remote_intent.is_some())<<1)|(u32::from(c.completed)<<2)),early.digest_total,early.digest_omitted,owner.mcfg_close_trace().committed]);
        let q=self.wlan_query.snapshot(owner);let reads=owner.read_sequence();row(28,"HIST IPA PD WLAN WLFW ORIG ACK READ MASK",&[u32::from(self.ipa_complete_seen),q[1],owner.tftp_wire_counts()[2],q[7],age(owner.original_tftp_write_trace().and_then(|w|w.ack.done_at)),diagnostic::packed_counts(reads.total,reads.kinds)]);
    }
}
