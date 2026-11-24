//! One-shot ELF queue boundary, existing phase attribution and startup outcomes.
use super::*;
impl RuntimeDomain {
    pub(super) fn show_tftp_completion(&self,d:&Display,owner:&ModemOwner){
        for n in 2..=28{d.replace_line(n,"");}
        d.replace_line(1,"ELF QUEUED HANDOFF");d.hex(2,"RUNTIME ERROR",self.error);
        let s=owner.service_registry_trace();let e=s.elf;let x=s.elf_exchange;
        let age=|a|self.time_from_us(Some(self.runtime_started),a);
        let delta=|a,b|self.time_from_us(a,b);
        let row=|n,label,v:&[u32]|{d.header_label(n,label);let mut f=[(0,8);4];for(i,v)in v.iter().enumerate(){f[i]=(*v,8);}d.header_row(n,true,&f[..v.len()]);};
        row(3,"ELF RX QUEUE PUB DONE",&[age(e.and_then(|e|e.received)),age(e.and_then(|e|e.queued)),age(e.and_then(|e|e.published)),age(e.and_then(|e|e.done))]);
        row(4,"RX END REG ENTER SELECT",&[age(e.and_then(|e|e.dispatch.receive_pass_end)),age(e.and_then(|e|e.dispatch.registry_entered)),age(e.and_then(|e|e.dispatch.selected))]);
        row(5,"PHASE US RX REG SEL QUEUE",&[delta(e.and_then(|e|e.received),e.and_then(|e|e.dispatch.receive_pass_end)),delta(e.and_then(|e|e.dispatch.receive_pass_end),e.and_then(|e|e.dispatch.registry_entered)),delta(e.and_then(|e|e.dispatch.registry_entered),e.and_then(|e|e.dispatch.selected)),delta(e.and_then(|e|e.dispatch.selected),e.and_then(|e|e.queued))]);
        row(6,"QUEUE PUB RXDONE TOTAL US",&[delta(e.and_then(|e|e.queued),e.and_then(|e|e.published)),delta(e.and_then(|e|e.published),e.and_then(|e|e.done)),delta(e.and_then(|e|e.received),e.and_then(|e|e.published)),delta(e.and_then(|e|e.received),e.and_then(|e|e.done))]);
        let t=owner.elf_queued_turn_trace();
        row(7,"ARM ENTER END FLAGS",&[age(t.armed_at),age(t.entry),age(t.end),t.flags]);
        row(8,"TURN RX TX ACK US",&[t.received,t.published,u32::try_from(t.acknowledged).unwrap_or(u32::MAX),delta(t.entry,t.end)]);
        row(9,"ELF PEER OFFSET ROWS TOTAL",&[e.map_or(u32::MAX,|e|e.peer_port),e.map_or(u32::MAX,|e|e.domain_offset),e.and_then(|e|e.returned_rows).map_or(u32::MAX,u32::from),e.and_then(|e|e.total_domains).map_or(u32::MAX,u32::from)]);
        let c=x.and_then(|e|e.host_credit);
        row(10,"ELF BODY DONE CREDIT CONF",&[x.map_or(u32::MAX,|e|u32::from(e.qmi_matches_reference)),x.map_or(u32::MAX,|e|u32::from(e.completed)),c.map_or(u32::MAX,|c|u32::from(c.requested)|(u32::from(c.completed)<<1)|c.before_reply.map_or(0,|v|(u32::from(v[0])<<2)|(u32::from(v[1])<<3))),x.map_or(u32::MAX,|e|e.confirm_rx)]);
        let waits=e.map_or([0;6],|e|e.waits_by_reason);row(11,"WAIT NONE PEER TXCAP FLOW",&waits[..4]);row(12,"WAIT CREDIT CAP CALLS OMIT",&[waits[4],waits[5],e.map_or(0,|e|e.dispatch.turns.calls),e.map_or(0,|e|e.dispatch.turns.omitted)]);
        row(13,"PRESELECT RUNS",&e.map_or([0;4],|e|e.dispatch.turns.runs));
        let b=e.and_then(|e|e.blocker);
        row(14,"BLOCK TXN FLAGS WAIT PUB",&[b.map_or(u32::MAX,|b|u32::from(b.transaction)),b.map_or(u32::MAX,|b|u32::from(b.flags_at_wait)),age(b.and_then(|b|b.wait)),age(b.and_then(|b|b.published))]);
        row(15,"BLOCK DONE YIELD FIRST",&[age(b.and_then(|b|b.done)),age(b.and_then(|b|b.yielded_at)),age(b.and_then(|b|b.first_turn))]);
        row(16,"TX4 RX QUEUE PUB DONE",&[age(s.txn4.and_then(|t|t.received)),age(s.txn4.and_then(|t|t.queued)),age(s.txn4.and_then(|t|t.published)),age(s.txn4.and_then(|t|t.done))]);
        row(17,"LOC RX QUEUE NAMES DONE",&[s.requests,s.replies_queued,s.queued_names,s.completed_names]);
        row(18,"LOC TMS GPS ELF WLAN",&s.name_counts[..4]);row(19,"LOC PDR DUMP OTHER AVS",&[s.name_counts[4],s.name_counts[5],s.name_counts[6],s.avs_audio_queries]);
        row(20,"ORIG ACK ELF DONE",&[age(owner.original_tftp_write_trace().and_then(|w|w.ack.done_at)),age(e.and_then(|e|e.done))]);
        let cap=owner.post_elf_mcfg();let m=cap.and_then(|c|c.mcfg.as_ref());
        row(21,"MCFG PEER FLAGS ERROR LEN",&[m.map_or(u32::MAX,|m|m.peer.port),m.map_or(0,|m|m.flags),m.map_or(u32::MAX,|m|if m.reply_opcode==5{m.reply_number}else{u32::MAX}),m.map_or(0,|m|m.reply_bytes)]);
        row(22,"MCFG RX PUB DONE CLOSE",&[age(m.and_then(|m|m.received)),age(m.and_then(|m|m.published)),age(m.and_then(|m|m.done)),age(m.and_then(|m|m.close_at))]);
        row(23,"OTHER KIND PEER LEN OP",&cap.and_then(|c|c.other).unwrap_or([u32::MAX;4]));
        let(writes,bytes,reads)=self.server_check.mcfg_progress();row(24,"MCFG W FILE READ IPA",&[writes,bytes.map_or(u32::MAX,|n|n as u32),reads,u32::from(self.ipa_complete_seen)]);
        let q=self.wlan_query.snapshot(owner);row(25,"PD STATE WLAN WLFW",&[q[1],owner.tftp_wire_counts()[2],q[7]]);
        row(26,"JOURNAL N OMIT BAD",&[cap.map_or(0,|c|c.total),cap.map_or(0,|c|c.omitted),cap.map_or(0,|c|c.bad)]);
    }
}
