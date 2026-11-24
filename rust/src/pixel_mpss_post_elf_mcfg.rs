//! Existing startup outcomes beside a bounded, identity-bound MCFG observation.
use super::*;
impl RuntimeDomain {
    pub(super) fn show_tftp_completion(&self,d:&Display,owner:&ModemOwner){
        for row in 2..=28 {d.replace_line(row,"");}
        let trace=owner.service_registry_trace();let elf=trace.elf;let exchange=trace.elf_exchange;
        let cap=owner.post_elf_mcfg();let m=cap.and_then(|c|c.mcfg.as_ref());
        let age=|at|self.time_from_us(Some(self.runtime_started),at);
        let row=|n,label,values:&[u32]|{d.header_label(n,label);let mut fields=[(0,8);4];for(i,v)in values.iter().enumerate(){fields[i]=(*v,8);}d.header_row(n,true,&fields[..values.len()]);};
        d.replace_line(1,"POST ELF MCFG TRACE");d.hex(2,"RUNTIME ERROR",self.error);
        row(3,"ELF R Q P D",&[age(elf.and_then(|e|e.received)),age(elf.and_then(|e|e.queued)),age(elf.and_then(|e|e.published)),age(elf.and_then(|e|e.done))]);
        let credit=exchange.and_then(|e|e.host_credit);
        row(4,"ELF BODY DONE CREDIT CONF",&[exchange.map_or(u32::MAX,|e|u32::from(e.qmi_matches_reference)),exchange.map_or(u32::MAX,|e|u32::from(e.completed)),credit.map_or(u32::MAX,|c|u32::from(c.requested)|(u32::from(c.completed)<<1)|c.before_reply.map_or(0,|v|(u32::from(v[0])<<2)|(u32::from(v[1])<<3))),exchange.map_or(u32::MAX,|e|e.confirm_rx)]);
        row(5,"MCFG PEER LEN FLAGS FILE",&[m.map_or(u32::MAX,|m|m.peer.port),m.map_or(u32::MAX,|m|m.bytes),m.map_or(0,|m|m.flags),m.and_then(|m|m.file_before).unwrap_or(u32::MAX)]);
        row(6,"OPT MASK FLAGS UNK KEY",&[m.map_or(0,|m|m.options.present),m.map_or(0,|m|m.options.flags),m.map_or(0,|m|m.options.unknown_count),m.map_or(u32::MAX,|m|m.options.unknown_key)]);
        let opts=m.map_or([u32::MAX;7],|m|m.options.values);
        row(7,"BLK TIME WIN TSIZE",&opts[..4]);row(8,"RSIZE SEEK UNLINK",&opts[4..]);
        row(9,"SERVER RESULT OP NUM LEN",&[m.and_then(|m|m.server_result).unwrap_or(u32::MAX),m.map_or(u32::MAX,|m|m.reply_opcode),m.map_or(u32::MAX,|m|m.reply_number),m.map_or(0,|m|m.reply_bytes)]);
        row(10,"REPLY MASK FLAGS PORT ID",&[m.map_or(0,|m|m.reply_options.present),m.map_or(0,|m|m.reply_options.flags),m.map_or(u32::MAX,|m|m.transfer_port),m.map_or(u32::MAX,|m|m.remote_intent)]);
        let opts=m.map_or([u32::MAX;7],|m|m.reply_options.values);
        row(11,"R BLK TIME WIN TSIZE",&opts[..4]);row(12,"R RSIZE SEEK UNLINK",&opts[4..]);
        row(13,"RX QUEUE PUB DONE US",&[age(m.and_then(|m|m.received)),age(m.and_then(|m|m.queued)),age(m.and_then(|m|m.published)),age(m.and_then(|m|m.done))]);
        row(14,"CLIENT FIRST LAST COUNTS",&[m.and_then(|m|m.first_client).unwrap_or(u32::MAX),m.and_then(|m|m.last_client).unwrap_or(u32::MAX),m.map_or(0,|m|diagnostic::packed_counts(m.client_counts[0],m.client_counts[1])),m.map_or(0,|m|m.client_counts[2])]);
        row(15,"CLIENT FIRST LAST CLOSE",&[age(m.and_then(|m|m.first_client_at)),age(m.and_then(|m|m.last_client_at)),age(m.and_then(|m|m.close_at)),m.map_or(u32::MAX,|m|m.close_port)]);
        row(16,"JOURNAL N OMIT BAD QWAIT",&[cap.map_or(0,|c|c.total),cap.map_or(0,|c|c.omitted),cap.map_or(0,|c|c.bad),m.map_or(0,|m|diagnostic::packed_counts(m.queue_waits[0],m.queue_waits[1]))]);
        if let Some(c)=cap {for(i,h)in c.journal.iter().enumerate(){if let Some(h)=h {
            d.header_row(17+i/2,i%2!=0,&[(age(h.at),8),(u32::from(h.tx),1),(u32::from(h.kind),2),(h.source,8),(h.destination,8),(u32::from(h.tag),1),(h.a,8),(h.b,8)]);
        }}}
        let original=owner.original_tftp_write_trace();
        row(25,"ORIG ACK ELF DONE",&[age(original.and_then(|w|w.ack.done_at)),age(elf.and_then(|e|e.done))]);
        let(writes,bytes,reads)=self.server_check.mcfg_progress();
        row(26,"MCFG W FILE READ IPA",&[writes,bytes.map_or(u32::MAX,|n|n as u32),reads,u32::from(self.ipa_complete_seen)]);
        let q=self.wlan_query.snapshot(owner);row(27,"PD STATE WLAN WLFW",&[q[1],owner.tftp_wire_counts()[2],q[7]]);
        row(28,"OTHER KIND PEER LEN OP",&cap.and_then(|c|c.other).unwrap_or([u32::MAX;4]));
    }
}
