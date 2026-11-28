//! Original second/third ruleset reply consumption and matching receipt visibility.
use super::*;
/* RULESET_CONTROL_BEGIN */
#[cfg(feature="sunfish-ruleset-control-window")]
#[path="pixel_mpss_ruleset_control.rs"]
mod ruleset_control;
/* RULESET_CONTROL_END */

impl RuntimeDomain{
 pub(super) fn show_ruleset_reply_fifo(&self,d:&Display,owner:&ModemOwner){
/* RULESET_CONTROL_BEGIN */
  #[cfg(feature="sunfish-ruleset-control-window")]
  {self.show_ruleset_control_window(d,owner);return;}
/* RULESET_CONTROL_END */
  for n in 2..=28{d.replace_line(n,"");}d.replace_line(1,"RULESET REPLY FIFO");
  let age=|t|self.time_from_us(Some(self.runtime_started),t);
  let row=|n,label,v:&[u32]|{d.header_label(n,label);let mut f=[(0,8);6];for(i,v)in v.iter().enumerate(){f[i]=(*v,8);}d.header_row(n,true,&f[..v.len()]);};
  let early=owner.startup_prerequisites();let close=owner.mcfg_close_trace();let s=owner.service_registry_trace();let e=s.elf;let ex=s.elf_exchange;let c=ex.and_then(|e|e.host_credit);
  row(2,"ERROR DETAIL CLOSE STAGE RULE N OMIT",&[self.error,self.detail,owner.pending_mcfg_close().map_or(u32::MAX,|p|p.port),close.stage,early.total[0],early.omitted[0]]);
  row(3,"ELF RX Q PUB OWNER MATCH CREDIT",&[age(e.and_then(|e|e.received)),age(e.and_then(|e|e.queued)),age(e.and_then(|e|e.published)),age(e.and_then(|e|e.done)),ex.map_or(u32::MAX,|e|u32::from(e.qmi_matches_reference)),c.map_or(u32::MAX,|c|u32::from(c.requested)|(u32::from(c.remote_intent.is_some())<<1)|(u32::from(c.completed)<<2))]);
  let h=owner.ruleset_handoff_trace();let p=owner.registry_window_trace().predecessor.and_then(|p|p.publication);row(4,"SERVICE CALL RESULT FLAGS WRITER TX3 N MASK",&[h.calls,h.outcome,h.flags,h.writer.calls,p.map_or(u32::MAX,|p|p.calls),p.map_or(u32::MAX,|p|p.mask)]);
  for(n,index)in [1,6].iter().enumerate(){let r=early.reads[*index];let seq=r.and_then(|r|r.record_sequence());let w=owner.ruleset_reply_fifo(n).filter(|w|seq==Some(w.record));let f=w.map(|w|w.fifo);let b=f.and_then(|f|f.completion);let base=5+n*6;
   row(base,if n==0{"RULE2 PEER FLAGS SEQ LO HI INTENT"}else{"RULE3 PEER FLAGS SEQ LO HI INTENT"},&[r.map_or(u32::MAX,|r|r.peer.port),r.map_or(0,|r|r.flags),seq.map_or(u32::MAX,|s|s as u32),seq.map_or(u32::MAX,|s|(s>>32)as u32),w.map_or(u32::MAX,|w|w.intent)]);
   row(base+1,"RX DISPATCH QUEUE PUB DONE CLOSE",&[age(r.and_then(|r|r.received)),age(r.and_then(|r|r.dispatched)),age(r.and_then(|r|r.queued)),age(r.and_then(|r|r.published)),age(r.and_then(|r|r.done)),age(r.and_then(|r|r.close))]);
   row(base+2,"ARM TAIL TX LEFT RX ARM RX CLOSE",&[age(w.and_then(|w|w.armed)),age(f.and_then(|f|f.peer_tail_at)),w.map_or(u32::MAX,|w|w.pending_tx_bytes),w.map_or(u32::MAX,|w|w.unread_at_arm),w.and_then(|w|w.unread_at_close).unwrap_or(u32::MAX)]);
   row(base+3,"ABS VIS AHEAD VIS AT CLOSE FIN EVICT",&[age(b.and_then(|b|b.last_absent)),age(b.and_then(|b|b.first_visible)),b.and_then(|b|b.bytes_ahead).unwrap_or(u32::MAX),b.and_then(|b|b.visible_at_close).map_or(u32::MAX,u32::from),f.map_or(0,|f|u32::from(f.finished)),f.map_or(u32::MAX,|f|f.history_evictions)]);
   row(base+4,"OP CODE PORT BYTES TRIES FLOW CAP",&[r.map_or(u32::MAX,|r|(r.opcode.min(0xffff)<<16)|r.number.min(0xffff)),r.map_or(u32::MAX,|r|r.local_port),r.map_or(u32::MAX,|r|r.reply_bytes),r.map_or(0,|r|r.attempts),r.map_or(0,|r|r.waits[0]),r.map_or(0,|r|r.waits[1])]);
   row(base+5,"REQ BYTES OPT FLAGS RSIZE TMO CLIENT",&[r.map_or(u32::MAX,|r|r.request_bytes),r.map_or(0,|r|r.options_present),r.map_or(0,|r|r.options_flags),r.map_or(u32::MAX,|r|r.rsize),r.map_or(u32::MAX,|r|r.timeout_ms),r.map_or(u32::MAX,|r|r.client)]);
  }
  for(n,index)in [0,7,2,3].iter().enumerate(){let r=early.reads[*index];let seq=r.and_then(|r|r.record_sequence());row(17+n*2,match n{0=>"RULE1 PEER FLAGS SEQ LO HI OP CODE",1=>"RULE4 PEER FLAGS SEQ LO HI OP CODE",2=>"EFS1 PEER FLAGS SEQ LO HI OP CODE",_=>"EFS2 PEER FLAGS SEQ LO HI OP CODE"},&[r.map_or(u32::MAX,|r|r.peer.port),r.map_or(0,|r|r.flags),seq.map_or(u32::MAX,|s|s as u32),seq.map_or(u32::MAX,|s|(s>>32)as u32),r.map_or(u32::MAX,|r|(r.opcode.min(0xffff)<<16)|r.number.min(0xffff))]);row(18+n*2,"RX QUEUE PUB DONE CLOSE",&[age(r.and_then(|r|r.received)),age(r.and_then(|r|r.queued)),age(r.and_then(|r|r.published)),age(r.and_then(|r|r.done)),age(r.and_then(|r|r.close))]);}
  let ipa=owner.linux_ipa_report();let flags=ipa.and_then(|r|r.protocol.map(|p|u32::from(p.modem_ready)|(u32::from(p.qmi_uc_ready)<<1)|(u32::from(p.indication_requested)<<2)|(u32::from(p.indication_sent)<<3)|(u32::from(r.protocol_complete)<<4)|(u32::from(r.failed)<<5)|(u32::from(r.pending_record)<<6)|(u32::from(r.record_queued)<<7)|(u32::from(r.held_response)<<8)|(u32::from(p.initial_boot)<<9)));
  row(25,"LIVE IPA REG DRIVER COMMITS RESP PENDING",&[flags.unwrap_or(u32::MAX),ipa.map_or(u32::MAX,|r|r.registration_requests),ipa.map_or(u32::MAX,|r|r.driver_requests),ipa.map_or(u32::MAX,|r|r.publications_committed),ipa.map_or(u32::MAX,|r|r.responses_accepted),ipa.and_then(|r|r.pending_message_id).map_or(u32::MAX,u32::from)]);
  let f=owner.elf_reply_fifo();let fifo=f.map(|f|f.fifo);let b=fifo.and_then(|f|f.completion);row(26,"ELF TAIL ABS VIS AHEAD CORE OWNER",&[age(fifo.and_then(|f|f.peer_tail_at)),age(b.and_then(|b|b.last_absent)),age(b.and_then(|b|b.first_visible)),b.and_then(|b|b.bytes_ahead).unwrap_or(u32::MAX),age(f.and_then(|f|f.core_done_at)),age(e.and_then(|e|e.done))]);
  let w=owner.original_tftp_write_trace();row(27,"ORIG PEER DATA ACK PUB DONE CLOSE",&[w.map_or(u32::MAX,|w|w.peer.port),age(w.and_then(|w|w.data).and_then(|d|d.received_at)),age(w.and_then(|w|w.ack.published_at)),age(w.and_then(|w|w.ack.done_at)),age(w.and_then(|w|w.timing.first_close_at))]);
  let q=self.wlan_query.snapshot(owner);let(writes,file,_)=self.server_check.mcfg_progress();row(28,"HIST IPA PD WLAN WLFW MCFG WRITES FILE",&[u32::from(self.ipa_complete_seen),q[1],owner.tftp_wire_counts()[2],q[7],writes,file.map_or(u32::MAX,|n|n as u32)]);
 }
}
