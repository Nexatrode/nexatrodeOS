//! Header-only metadata. Never used to admit, publish, release or route a packet.
use super::*;
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub struct ElfTxSlot{
 pub kind:u8,pub flags:u8,pub op:u16,pub len:u16,pub sent:u16,pub transaction:u16,pub message:u16,
 pub source:Address,pub destination:Address,pub remote:u32,pub qrtr_type:u32,
}
#[derive(Clone,Copy,Debug)]
pub struct ElfTxFull{pub at:Option<u64>,pub slots:[ElfTxSlot;TX_SLOTS]}
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub struct ElfTxFree{pub at:Option<u64>,pub slot:u8,pub cause:u8,pub empty_mask:u8,pub original:ElfTxSlot}
#[derive(Clone,Copy,Debug)]
pub struct ElfTxSlots{
 request:RequestHandle,pub first:ElfTxFull,pub latest:ElfTxFull,pub failures:u32,
 pub first_free:Option<ElfTxFree>,pub admitted:Option<u64>,pub admitted_slot:Option<u8>,
 pub admitted_empty:u8,pub finished:bool,
}
const _:()={assert!(::core::mem::size_of::<ElfTxSlots>()<=1024);assert!(MAX_DATAGRAM_BYTES<=u16::MAX as usize);};
impl Session{
 pub fn elf_tx_slots(&self)->Option<&ElfTxSlots>{self.elf_tx_slots.as_ref()}
 fn elf_tx_slot(&self,index:usize)->ElfTxSlot{
  let t=&self.transmit[index];let missing=Address{node:u32::MAX,port:u32::MAX};
  let(source,destination)=t.kind.qrtr_addresses().unwrap_or_else(||if t.len>=32{(Address{node:word(&t.bytes,8),port:word(&t.bytes,12)},Address{node:word(&t.bytes,24),port:word(&t.bytes,28)})}else{(missing,missing)});
  let linux= {#[cfg(feature="sunfish-linux-ipa-qmi")]{matches!(t.kind,TxKind::LinuxIpa{..})} #[cfg(not(feature="sunfish-linux-ipa-qmi"))]{false}};
  let qmi=t.len>=39 && (linux || matches!(t.kind,TxKind::Reply{..}|TxKind::IpaRequest|TxKind::IpaIndication{..}|TxKind::WlfwRequest|TxKind::ServregNotifRequest));
  let flags=u8::from(t.len!=0)|(u8::from(t.sent!=0)<<1)|(u8::from(t.len!=0&&t.sent==t.len)<<2)|(u8::from(t.glink_done)<<3)|(u8::from(t.qrtr_done)<<4)|(u8::from(t.remote_slot.is_some())<<5)|(u8::from(t.reference_flow.is_some())<<6);
  ElfTxSlot{kind:if t.len==0{0}else{t.kind.diagnostic_kind()},flags,
   op:if t.len>=34&&matches!(t.kind,TxKind::TftpReply{..}){u16::from_be_bytes([t.bytes[32],t.bytes[33]])}else if qmi{u16::from(t.bytes[32])}else if t.len>=32{u16::try_from(word(&t.bytes,4)).unwrap_or(u16::MAX)}else{u16::MAX},
   len:t.len as u16,sent:t.sent as u16,transaction:if qmi{u16::from_le_bytes([t.bytes[33],t.bytes[34]])}else{u16::MAX},message:if qmi{u16::from_le_bytes([t.bytes[35],t.bytes[36]])}else{u16::MAX},source,destination,
   remote:t.remote_slot.and_then(|i|self.remote[i]).map_or(u32::MAX,|r|r.id),qrtr_type:if t.len>=32{word(&t.bytes,4)}else{u32::MAX}}
 }
 fn elf_tx_empty(&self)->u8{self.transmit[..TX_SLOTS].iter().enumerate().fold(0,|m,(i,t)|m|(u8::from(t.len==0)<<i))}
 pub(super) fn observe_elf_tx_full(&mut self,h:RequestHandle){
  if !self.elf_window_request_matches(h) || self.elf_tx_slots.as_ref().is_some_and(|c|c.finished){return;}
  let full=ElfTxFull{at:self.intent_ack_clock.map(|c|c()),slots:core::array::from_fn(|i|self.elf_tx_slot(i))};
  if let Some(c)=self.elf_tx_slots.as_mut(){c.latest=full;c.failures=c.failures.saturating_add(1);}else{self.elf_tx_slots=Some(ElfTxSlots{request:h,first:full,latest:full,failures:1,first_free:None,admitted:None,admitted_slot:None,admitted_empty:0,finished:false});}
 }
 pub(super) fn observe_elf_tx_freed(&mut self,index:usize,cause:u8){
  if index>=TX_SLOTS || self.transmit[index].len!=0 || !self.elf_tx_slots.as_ref().is_some_and(|c|!c.finished&&c.first_free.is_none()){return;}
  let at=self.intent_ack_clock.map(|c|c());let empty_mask=self.elf_tx_empty();let c=self.elf_tx_slots.as_mut().unwrap();c.first_free=Some(ElfTxFree{at,slot:index as u8,cause,empty_mask,original:c.first.slots[index]});
 }
 pub(super) fn observe_elf_tx_admitted(&mut self,h:RequestHandle,index:usize){
  if !self.elf_tx_slots.as_ref().is_some_and(|c|!c.finished&&c.request==h){return;}
  let at=self.intent_ack_clock.map(|c|c());let empty=self.elf_tx_empty();let c=self.elf_tx_slots.as_mut().unwrap();c.admitted=at;c.admitted_slot=Some(index as u8);c.admitted_empty=empty;c.finished=true;
 }
}
