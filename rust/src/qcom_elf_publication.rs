//! Exact first ELF writer observation. Never used by the arbiter or lifecycle.
use super::*;
#[derive(Clone,Copy,Debug,Default,Eq,PartialEq)]
pub struct ElfWriterRecord {
    pub start:Option<u64>,pub end:Option<u64>,pub command:u16,pub parameter:u32,
    pub kind:u8,pub transaction:u16,pub flags:u8,pub result:u8,
}
#[derive(Clone,Copy,Debug,Default)]
pub struct ElfWriterTrace {
    pub passes:u32,pub timed:u32,pub omitted:u32,pub active:bool,pub finished:bool,
    pub first:Option<u64>,pub last:Option<u64>,pub total:Option<u64>,pub max_gap:Option<u64>,
    pub regressed:bool,pub records:[Option<ElfWriterRecord>;8],pub publication:Option<RegistryPublication>,
    current:ElfWriterRecord,
}
impl Session {
    fn elf_writer_slot(&self)->Option<usize>{
        let c=self.elf_window.as_ref().filter(|c|c.admitted.is_some()&&!c.writer.finished)?;
        self.transmit.iter().position(|t|t.len!=0&&t.sent<t.len&&matches!(t.kind,TxKind::Reply{receive,..}
            if c.request==RequestHandle{owner:self.owner,generation:self.generation,intent:self.receive[receive].id}))
    }
    pub(crate) fn begin_elf_writer_pass(&mut self){
        if self.elf_writer_slot().is_none(){return;}
        let at=self.intent_ack_clock.map(|f|f());let c=&mut self.elf_window.as_mut().unwrap().writer;
        c.passes=c.passes.saturating_add(1);c.active=true;c.current=ElfWriterRecord{start:at,transaction:u16::MAX,parameter:u32::MAX,..ElfWriterRecord::default()};
        if c.passes==1{c.first=at;}
        if let(Some(a),Some(b))=(at,c.last){if let Some(gap)=a.checked_sub(b){c.max_gap=Some(c.max_gap.map_or(gap,|g|g.max(gap)));}else{c.regressed=true;}}
    }
    pub(crate) fn elf_writer_header(&mut self,b:&[u8],result:Result<(),WriteError>){
        if let Some(c)=self.elf_window.as_mut().map(|c|&mut c.writer).filter(|c|c.active) && b.len()>=8 {
            c.current.command=u16::from_le_bytes([b[0],b[1]]);c.current.parameter=word(b,4);c.current.result=match result{Ok(())=>1,Err(WriteError::WouldBlock)=>2,Err(WriteError::Uncertain)=>3};
        }
    }
    pub(crate) fn end_elf_writer_pass(&mut self,choice:PublishChoice,blocked:bool){
        let Some(slot)=self.elf_writer_slot()else{return;};
        if !self.elf_window.as_ref().unwrap().writer.active{return;}
        let t=&self.transmit[slot];let fitting=t.remote_slot.is_some()||self.remote.iter().flatten().any(|r|!r.busy&&r.capacity as usize>=t.len);
        let(kind,transaction)=match choice{PublishChoice::Data(i)=>{let t=&self.transmit[i];(t.kind.diagnostic_kind(),match t.kind{TxKind::Reply{route,..}=>route.transaction,_=>u16::MAX})},_=>(0,u16::MAX)};
        let publication=t.registry_publication;let at=self.intent_ack_clock.map(|f|f());let c=&mut self.elf_window.as_mut().unwrap().writer;
        let mut r=c.current;r.end=at;r.kind=kind;r.transaction=transaction;
        r.flags=u8::from(fitting)|(u8::from(matches!(choice,PublishChoice::Data(i)if i==slot))<<1)|(u8::from(blocked||r.result==2)<<2)|(u8::from(r.result==3)<<3);
        if let(Some(a),Some(b))=(r.start,r.end){if let Some(elapsed)=b.checked_sub(a){c.timed=c.timed.saturating_add(1);c.total=Some(c.total.unwrap_or(0).saturating_add(elapsed));}else{c.regressed=true;}}
        let n=c.passes.saturating_sub(1)as usize;
        if n<8{c.records[n]=Some(r);}else{c.records.copy_within(5..8,4);c.records[7]=Some(r);c.omitted=c.omitted.saturating_add(1);}
        c.last=at;c.publication=Some(publication);c.active=false;
    }
    pub(crate) fn end_elf_writer_yield(&mut self){
        // Synthetic no-callback marker, not an on-wire GLINK header.
        if let Some(c)=self.elf_window.as_mut().map(|c|&mut c.writer).filter(|c|c.active){c.current.parameter=0;}
        self.end_elf_writer_pass(PublishChoice::Idle,false);
    }
    pub(crate) fn freeze_elf_writer(&mut self,index:usize){
        let Some(c)=self.elf_window.as_mut().filter(|c|!c.writer.finished)else{return;};
        let t=&self.transmit[index];
        if matches!(t.kind,TxKind::Reply{receive,..}if c.request==RequestHandle{owner:self.owner,generation:self.generation,intent:self.receive[receive].id}){
            c.writer.finished=true;c.writer.publication=Some(t.registry_publication);
        }
    }
}
