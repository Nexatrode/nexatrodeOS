//! Opening/latest full pool and its first real release; display history is not authority.
use super::*;
impl RuntimeDomain{
 pub(super) fn show_tftp_completion(&self,d:&Display,owner:&ModemOwner){
  for n in 2..=28{d.replace_line(n,"");}d.replace_line(1,if cfg!(feature="sunfish-elf-capacity-retry"){"ELF CAPACITY RETRY"}else{"ELF TX SLOT OWNERS"});d.hex(2,"RUNTIME ERROR",self.error);
  let e=owner.service_registry_trace().elf;let c=owner.elf_tx_slots();let w=owner.elf_window_trace();let age=|t|self.time_from_us(Some(self.runtime_started),t);
  let row=|n,label,v:&[u32]|{d.header_label(n,label);let mut f=[(0,8);4];for(i,v)in v.iter().enumerate(){f[i]=(*v,8);}d.header_row(n,true,&f[..v.len()]);};
  row(3,"ELF RX QUEUE PUB DONE",&[age(e.and_then(|e|e.received)),age(e.and_then(|e|e.queued)),age(e.and_then(|e|e.published)),age(e.and_then(|e|e.done))]);
  row(4,"FIRST LAST FULL US FAILN END",&[age(c.and_then(|c|c.first.at)),age(c.and_then(|c|c.latest.at)),c.map_or(0,|c|c.failures),c.map_or(0,|c|u32::from(c.finished))]);
  let f=c.and_then(|c|c.first_free);row(5,"FIRST FREE US SLOT CAUSE EMPTY",&[age(f.and_then(|f|f.at)),f.map_or(u32::MAX,|f|u32::from(f.slot)),f.map_or(u32::MAX,|f|u32::from(f.cause)),f.map_or(u32::MAX,|f|u32::from(f.empty_mask))]);
  let meta=|r:personal_os_rust_server::qcom_glink_qmi::ElfTxSlot| (u32::from(r.kind)<<24)|(u32::from(r.flags)<<16)|u32::from(r.op);
  let packet=|r:personal_os_rust_server::qcom_glink_qmi::ElfTxSlot| [meta(r),r.source.port,r.destination.port,r.remote];
  let extra=|r:personal_os_rust_server::qcom_glink_qmi::ElfTxSlot| [(u32::from(r.len)<<16)|u32::from(r.sent),(u32::from(r.transaction)<<16)|u32::from(r.message),r.source.node,r.destination.node];
  row(6,"FREED ORIGINAL META SRC DST ID",&f.map_or([u32::MAX;4],|f|packet(f.original)));
  row(7,"FREED ORIGINAL LEN QMI NODES",&f.map_or([u32::MAX;4],|f|extra(f.original)));
  for i in 0..8{let r=c.map(|c|c.latest.slots[i]);row(8+i*2,"LAST SLOT META SRC DST ID",&r.map_or([u32::MAX;4],packet));row(9+i*2,"LEN SENT QMI SRCNODE DSTNODE",&r.map_or([u32::MAX;4],extra));}
  let changed=c.map_or(u32::MAX,|c|c.first.slots.iter().zip(c.latest.slots.iter()).enumerate().fold(0,|m,(i,(a,b))|m|(u32::from(a!=b)<<i)));
  row(24,"CHANGED ADMIT SLOT EMPTY N",&[changed,c.and_then(|c|c.admitted_slot).map_or(u32::MAX,u32::from),c.map_or(u32::MAX,|c|u32::from(c.admitted_empty)),w.and_then(|w|w.admitted).map_or(u32::MAX,|a|u32::from(a.count))]);
  #[cfg(feature="sunfish-elf-capacity-retry")]
  {let t=owner.elf_capacity_turn_trace();row(25,"EARLY ARM ENTER END FLAGS",&[age(t.armed),age(t.entered),age(t.ended),t.flags]);row(26,"EARLY SLOT RESULT ARMS FULL",&[t.free_slot.map_or(u32::MAX,u32::from),u32::from(t.result),t.arms,t.full_skips]);}
  #[cfg(not(feature="sunfish-elf-capacity-retry"))]
  {
  row(25,"RX END REG ENTER SELECT",&[age(e.and_then(|e|e.dispatch.receive_pass_end)),age(e.and_then(|e|e.dispatch.registry_entered)),age(e.and_then(|e|e.dispatch.selected))]);
  let delta=|a,b|self.time_from_us(a,b);row(26,"PHASE US RX REG SELECT Q",&[delta(e.and_then(|e|e.received),e.and_then(|e|e.dispatch.receive_pass_end)),delta(e.and_then(|e|e.dispatch.receive_pass_end),e.and_then(|e|e.dispatch.registry_entered)),delta(e.and_then(|e|e.dispatch.registry_entered),e.and_then(|e|e.dispatch.selected)),delta(e.and_then(|e|e.dispatch.selected),e.and_then(|e|e.queued))]);
  }
  let q=self.wlan_query.snapshot(owner);row(27,"IPA PD STATE WLAN WLFW",&[u32::from(self.ipa_complete_seen),q[1],owner.tftp_wire_counts()[2],q[7]]);
  let(writes,bytes,reads)=self.server_check.mcfg_progress();row(28,"ORIG ACK MCFG W FILE READ",&[age(owner.original_tftp_write_trace().and_then(|w|w.ack.done_at)),writes,bytes.map_or(u32::MAX,|n|n as u32),reads]);
 }
}
