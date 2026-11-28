//! First exact MCFG read after accepted ELF, including requests before ELF receipt.
use super::*;
impl RuntimeDomain {
 pub(super) fn show_first_mcfg_response(&self,d:&Display,owner:&ModemOwner){
  for n in 2..=28{d.replace_line(n,"");}d.replace_line(1,"FIRST MCFG RESPONSE");
  let age=|t|self.time_from_us(Some(self.runtime_started),t);let row=|n,label,v:&[u32]|{d.header_label(n,label);let mut f=[(0,8);6];for(i,v)in v.iter().enumerate(){f[i]=(*v,8);}d.header_row(n,true,&f[..v.len()]);};
  let s=owner.service_registry_trace();let e=s.elf;let old=owner.registry_window_trace().predecessor;let p=old.and_then(|p|p.publication);let cap=owner.post_elf_mcfg();let m=cap.and_then(|c|c.mcfg.as_ref());let a=m.map(|m|&m.admission);let seq=m.and_then(|m|m.record_sequence());
  row(2,"ERROR DETAIL CLOSE STAGE TX3 CALLS MASK",&[self.error,self.detail,owner.pending_mcfg_close().map_or(u32::MAX,|p|p.port),owner.mcfg_close_trace().stage,p.map_or(u32::MAX,|p|p.calls),p.map_or(u32::MAX,|p|p.mask)]);
  row(3,"TX3 Q PUB CORE ELF RX PUB OWNER",&[age(old.and_then(|p|p.queued_at)),age(old.and_then(|p|p.published_at)),age(old.and_then(|p|p.done_at)),age(e.and_then(|e|e.received)),age(e.and_then(|e|e.published)),age(e.and_then(|e|e.done))]);
  let ex=s.elf_exchange;let c=ex.and_then(|e|e.host_credit);let f=owner.elf_reply_fifo();row(4,"ELF MATCH CREDIT ID CORE OWNER ORIG ACK",&[ex.map_or(u32::MAX,|e|u32::from(e.qmi_matches_reference)),c.map_or(u32::MAX,|c|u32::from(c.requested)|(u32::from(c.remote_intent.is_some())<<1)|(u32::from(c.completed)<<2)),f.map_or(u32::MAX,|f|f.remote_intent),age(f.and_then(|f|f.core_done_at)),age(e.and_then(|e|e.done)),age(owner.original_tftp_write_trace().and_then(|w|w.ack.done_at))]);
  row(5,"MCFG PEER FLAGS RECORD LO HI BYTES FILE",&[m.map_or(u32::MAX,|m|m.peer.port),m.map_or(0,|m|m.flags),seq.map_or(u32::MAX,|n|n as u32),seq.map_or(u32::MAX,|n|(n>>32)as u32),m.map_or(u32::MAX,|m|m.bytes),m.and_then(|m|m.file_before).unwrap_or(u32::MAX)]);
  row(6,"MCFG RX DISPATCH Q PUB DONE CLOSE",&[age(m.and_then(|m|m.received)),age(m.and_then(|m|m.dispatched)),age(m.and_then(|m|m.queued)),age(m.and_then(|m|m.published)),age(m.and_then(|m|m.done)),age(m.and_then(|m|m.close_at))]);
  row(7,"SERVER RESULT OP NUM BYTES PORT INTENT",&[m.and_then(|m|m.server_result).unwrap_or(u32::MAX),m.map_or(u32::MAX,|m|m.reply_opcode),m.map_or(u32::MAX,|m|m.reply_number),m.map_or(0,|m|m.reply_bytes),m.map_or(u32::MAX,|m|m.transfer_port),m.map_or(u32::MAX,|m|m.remote_intent)]);
  row(8,"REQUEST MASK FLAGS UNK KEY CLIENT FIRST LAST",&[m.map_or(0,|m|m.options.present),m.map_or(0,|m|m.options.flags),m.map_or(0,|m|m.options.unknown_count),m.map_or(u32::MAX,|m|m.options.unknown_key),m.and_then(|m|m.first_client).unwrap_or(u32::MAX),m.and_then(|m|m.last_client).unwrap_or(u32::MAX)]);
  let opts=m.map_or([u32::MAX;7],|m|m.options.values);row(9,"REQ BLOCK TIME WIN TSIZE RSIZE SEEK",&opts[..6]);
  let opts=m.map_or([u32::MAX;7],|m|m.reply_options.values);row(10,"REPLY MASK FLAGS BLOCK TIME WIN TSIZE",&[m.map_or(0,|m|m.reply_options.present),m.map_or(0,|m|m.reply_options.flags),opts[0],opts[1],opts[2],opts[3]]);
  row(11,"REPLY RSIZE SEEK UNLINK REQ UNLINK CLIENT US",&[opts[4],opts[5],opts[6],m.map_or(u32::MAX,|m|m.options.values[6]),age(m.and_then(|m|m.first_client_at)),age(m.and_then(|m|m.last_client_at))]);
  row(12,"CLIENT ACK ERROR REPEAT RXEV OMIT CANCEL",&[m.map_or(0,|m|m.client_counts[0]),m.map_or(0,|m|m.client_counts[1]),m.map_or(0,|m|m.client_counts[2]),owner.mcfg_rx_veto_trace().total,owner.mcfg_rx_veto_trace().omitted,age(m.and_then(|m|m.canceled_at))]);
  row(13,"QUEUE FIRST LAST TRIES FLOW BUSY CAPACITY",&[age(a.and_then(|a|a.first_attempt)),age(a.and_then(|a|a.last_attempt)),a.map_or(0,|a|a.attempts),m.map_or(0,|m|m.queue_waits[0]),m.map_or(0,|m|m.queue_waits[1])]);
  let b=a.map_or([0;4],|a|a.block);row(14,"BLOCK CHECK KIND OP FLAGS LEN SENT",&[b[0],b[1],b[2],b[3],a.map_or(0,|a|a.block_len),a.map_or(0,|a|a.block_sent)]);
  row(15,"BLOCK PUB DONE RECORD LO HI PASSES OMIT",&[age(a.and_then(|a|a.block_published)),age(a.and_then(|a|a.block_completed)),a.and_then(|a|a.block_sequence).map_or(u32::MAX,|n|n as u32),a.and_then(|a|a.block_sequence).map_or(u32::MAX,|n|(n>>32)as u32),a.map_or(0,|a|a.passes),a.map_or(0,|a|a.omitted)]);
  let w=a.and_then(|a|a.records[0]);row(16,"FIRST WRITER START END GATES CMD PARAM RESULT",&[age(w.and_then(|w|w.start)),age(w.and_then(|w|w.end)),w.map_or(u32::MAX,|w|w.gates),w.map_or(u32::MAX,|w|w.command),w.map_or(u32::MAX,|w|w.parameter),w.map_or(u32::MAX,|w|w.result)]);
  let v=owner.mcfg_queued_turn_trace();row(17,"QUEUED TURN CALL WRITE RXVETO BLOCK JOURNAL BAD",&[v.calls,v.writes,v.rx_veto,v.blocked,cap.map_or(0,|c|c.total),cap.map_or(0,|c|c.bad)]);
  row(18,"OTHER PATH PEER BYTES OP JOURNAL OMIT",&[cap.and_then(|c|c.other).map_or(u32::MAX,|v|v[0]),cap.and_then(|c|c.other).map_or(u32::MAX,|v|v[1]),cap.and_then(|c|c.other).map_or(u32::MAX,|v|v[2]),cap.and_then(|c|c.other).map_or(u32::MAX,|v|v[3]),cap.map_or(0,|c|c.omitted)]);
  let(writes,file,done)=self.server_check.mcfg_progress();let close=owner.mcfg_close_trace();row(19,"MCFG WRITES FILE READS CLOSE OBS COMMIT",&[writes,file.map_or(u32::MAX,|n|n as u32),done,close.observed,close.committed]);
  if let Some(c)=cap{for(i,h)in c.journal.iter().enumerate(){if let Some(h)=h{d.header_row(20+i/2,i%2!=0,&[(age(h.at),8),(u32::from(h.tx),1),(u32::from(h.kind),2),(h.source,8),(h.destination,8),(u32::from(h.tag),1),(h.a,8),(h.b,8)]);}}}
  let q=self.wlan_query.snapshot(owner);let ipa=owner.linux_ipa_report();row(28,"IPA PD WLAN WLFW PENDING FAILED",&[u32::from(self.ipa_complete_seen),q[1],owner.tftp_wire_counts()[2],q[7],ipa.and_then(|r|r.pending_message_id).map_or(u32::MAX,u32::from),ipa.map_or(u32::MAX,|r|u32::from(r.failed))]);
 }
}
