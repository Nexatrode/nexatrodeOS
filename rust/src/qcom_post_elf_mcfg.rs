//! Bounded observation only. No policy, ownership, device access or packet contents.
use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct McfgOptionIds {
    pub present: u32, pub flags: u32, pub unknown_count: u32, pub unknown_key: u32,
    /// blksize, timeoutms, wsize, tsize, rsize, seek, unlink; presence disambiguates MAX.
    pub values: [u32; 7],
}
impl Default for McfgOptionIds {
    fn default() -> Self { Self { present: 0, flags: 0, unknown_count: 0,
        unknown_key: u32::MAX, values: [u32::MAX; 7] } }
}
fn field<'a>(bytes: &'a [u8], at: &mut usize) -> Option<&'a [u8]> {
    let tail=bytes.get(*at..)?; let n=tail.iter().position(|b|*b==0)?;
    *at+=n+1; Some(&tail[..n])
}
impl McfgOptionIds {
    /// Flags: malformed=1, bad number=2, duplicate=4, unknown=8, overflow=16, oversized=32, mode=64.
    pub fn read(bytes: &[u8], request: bool) -> Self {
        let mut out=Self::default(); let mut at=2;
        if bytes.len()<2 || bytes.len()>512 {out.flags|=32;return out;}
        if request {
            if field(bytes,&mut at).is_none() {out.flags|=1;return out;}
            if !field(bytes,&mut at).is_some_and(|m|m.eq_ignore_ascii_case(b"octet")) {out.flags|=64;return out;}
        }
        while at<bytes.len() {
            let Some(key)=field(bytes,&mut at).filter(|k|!k.is_empty()) else {out.flags|=1;break;};
            let Some(value)=field(bytes,&mut at).filter(|v|!v.is_empty()) else {out.flags|=1;break;};
            let index=[b"blksize".as_slice(),b"timeoutms",b"wsize",b"tsize",b"rsize",b"seek",b"unlink"]
                .iter().position(|k|*k==key);
            if index.is_none() || index==Some(6) {
                out.flags|=8;out.unknown_count=out.unknown_count.saturating_add(1);
                if out.unknown_key==u32::MAX {let mut k=[0;4];for(i,b)in key.iter().take(4).enumerate(){k[i]=*b;}out.unknown_key=u32::from_be_bytes(k);}
            }
            let Some(i)=index else {continue;};
            if out.present&(1<<i)!=0 {out.flags|=4;continue;}
            out.present|=1<<i;let mut n=0u32;let mut valid=true;
            for c in value {
                if !c.is_ascii_digit(){out.flags|=2;valid=false;break;}
                let Some(next)=n.checked_mul(10).and_then(|n|n.checked_add(u32::from(c-b'0')))else{out.flags|=16;valid=false;break;};n=next;
            }
            if valid {out.values[i]=n;}
        }
        out
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct McfgWireRecord {
    pub at: Option<u64>, pub tx: bool, pub kind: u8, pub source: u32, pub destination: u32,
    pub tag: u8, pub a: u32, pub b: u32,
}
fn category(body:&[u8])->u32 {
    if !matches!(body.get(..2),Some([0,1])|Some([0,2])) {return 0;}
    let mut at=2;let Some(path)=field(body,&mut at)else{return 7;};
    match path {
        b"/readwrite/server_check.txt"=>1,b"/readwrite/mcfg.tmp"=>2,
        b"/readwrite/ota_firewall/ruleset"=>3,b"/readwrite/modem_efs"=>4,
        b"/readonly/vendor/mbn/mcfg_hw/mbn_hw.dig"=>8,
        p if p.ends_with(b"/wlanmdsp.mbn")=>5,_=>6,
    }
}
fn local_tftp(port:u32)->bool {port==0x5000 || crate::qcom_tftp_server_check::is_transfer_port(port) || (0x5100..=0x51ff).contains(&port)}
fn packet(bytes:&[u8],tx:bool,at:Option<u64>)->Option<McfgWireRecord>{
    if bytes.len()<32 || word(bytes,0)!=1{return None;}
    let body=bytes.get(32..32usize.checked_add(word(bytes,20)as usize)?)?;
    let kind=word(bytes,4);if kind>255{return None;}
    let mut h=McfgWireRecord{at,tx,kind:kind as u8,source:word(bytes,12),destination:word(bytes,28),tag:15,a:u32::MAX,b:u32::MAX};
    if matches!(kind,6|7) && body.len()>=12 {h.tag=if kind==6{13}else{12};h.a=word(body,4);h.b=word(body,8);}
    else if matches!(kind,4|5) && body.len()>=20 {h.tag=11;h.a=word(body,4);h.b=word(body,8);}
    else if kind==1 && local_tftp(if tx{h.source}else{h.destination}) && body.len()>=2 {
        let op=u16::from_be_bytes([body[0],body[1]]);
        if (1..=6).contains(&op) {let n=if matches!(op,3|4|5){body.get(2..4).map(|b|u16::from_be_bytes([b[0],b[1]])).unwrap_or(u16::MAX)}else{u16::MAX};h.tag=10;h.a=(u32::from(op)<<16)|u32::from(n);h.b=(category(body)<<24)|body.len()as u32;}
    } else if kind==1 && body.len()>=7 && matches!(body[0],0|2|4)
        && usize::from(u16::from_le_bytes([body[5],body[6]]))+7==body.len() {
        h.tag=body[0];h.a=u32::from(u16::from_le_bytes([body[1],body[2]]));h.b=u32::from(u16::from_le_bytes([body[3],body[4]]));
    }
    Some(h)
}

#[cfg(feature="sunfish-mcfg-writer-trace")]
#[derive(Clone,Copy,Debug,Default)]
pub struct McfgWriterRecord {
    pub start:Option<u64>,pub end:Option<u64>,pub gates:u32,
    pub command:u32,pub parameter:u32,pub kind:u32,pub source:u32,pub destination:u32,pub result:u32,
}
#[cfg(feature="sunfish-mcfg-writer-trace")]
#[derive(Clone,Copy,Debug,Default)]
pub struct McfgAdmissionTrace {
    pub first_attempt:Option<u64>,pub last_attempt:Option<u64>,pub attempts:u32,
    pub block_at:Option<u64>,pub block:[u32;4],pub block_len:u32,pub block_sent:u32,pub block_sequence:Option<u64>,
    pub passes:u32,pub omitted:u32,pub records:[Option<McfgWriterRecord>;8],active:Option<usize>,
}
#[derive(Clone, Copy, Debug)]
pub struct PostMcfgRequest {
    handle:TftpHandle, slot:Option<usize>, pub peer:Address, pub bytes:u32, pub flags:u32,
    pub options:McfgOptionIds, pub file_before:Option<u32>, pub server_result:Option<u32>,
    pub reply_opcode:u32,pub reply_number:u32,pub reply_bytes:u32,pub reply_options:McfgOptionIds,
    pub transfer_port:u32,pub remote_intent:u32,
    pub received:Option<u64>,pub dispatched:Option<u64>,pub queued:Option<u64>,pub published:Option<u64>,pub done:Option<u64>,
    #[cfg(feature="sunfish-mcfg-record-identity")]
    record:Option<crate::qcom_qrtr_flow::Reservation>,
    #[cfg(feature="sunfish-mcfg-writer-trace")]
    pub admission:McfgAdmissionTrace,
    #[cfg(feature="sunfish-mcfg-record-identity")]
    pub canceled_at:Option<u64>,
    pub first_client:Option<u32>,pub last_client:Option<u32>,pub first_client_at:Option<u64>,pub last_client_at:Option<u64>,
    pub client_counts:[u32;3],pub close_at:Option<u64>,pub close_port:u32,pub queue_waits:[u32;2],
}
#[cfg(feature="sunfish-mcfg-record-identity")]
impl PostMcfgRequest {
    /// Existing host-only reservation sequence, for diagnostics only.
    pub fn record_sequence(&self)->Option<u64>{self.record.map(|r|r.sequence())}
}
#[cfg(feature="sunfish-mcfg-record-identity")]
fn mcfg_record_matches(m:&PostMcfgRequest,tx:&Transmit,local:u32)->bool{
    m.flags&256==0 && m.record.is_some() && m.record==tx.reference_flow
        && matches!(tx.kind,TxKind::TftpReply{source,destination}
            if source==Address{node:local,port:m.transfer_port}&&destination==m.peer)
        && tx.len>=32 && word(&tx.bytes,4)==1 && word(&tx.bytes,20)==m.reply_bytes
}

#[derive(Clone, Copy, Debug)]
pub struct PostElfMcfgTrace {
    elf:RequestHandle,local:u32,peer:u32, pub revision:u64,
    pub mcfg:Option<PostMcfgRequest>,
    pub journal:[Option<McfgWireRecord>;16],pub total:u32,pub omitted:u32,pub bad:u32,
    /// First request outside the three temporary names/server-check: category, peer, bytes, opcode.
    pub other:Option<[u32;4]>,
}
impl PostElfMcfgTrace {
    fn push(&mut self,h:McfgWireRecord){
        self.revision=self.revision.saturating_add(1);
        if self.total<16 {self.journal[self.total as usize]=Some(h);} else {self.journal.copy_within(9..16,8);self.journal[15]=Some(h);self.omitted=self.omitted.saturating_add(1);}
        self.total=self.total.saturating_add(1);
    }
    fn wire(&mut self,bytes:&[u8],tx:bool,at:Option<u64>){
        let Some(h)=packet(bytes,tx,at)else{self.bad=self.bad.saturating_add(1);self.revision=self.revision.saturating_add(1);return;};
        let pair=if tx{(h.source,h.destination)}else{(h.destination,h.source)};
        let relevant=(h.kind==1 && (pair==(self.local,self.peer)||local_tftp(pair.0)))
            || matches!(h.kind,3|6|7|13) || (matches!(h.kind,4|5)&&matches!(h.a,49|64|69));
        if relevant {self.push(h);}
    }
}
#[cfg(feature="sunfish-mcfg-writer-trace")]
impl Session {
    fn mcfg_writer_slot(&self)->Option<usize>{
        let m=self.post_elf_mcfg.as_ref()?.mcfg.as_ref()?;let i=m.slot?;let t=&self.transmit[i];
        (m.flags&0x130==0 && t.sent<t.len && mcfg_record_matches(m,t,self.config.local_node)).then_some(i)
    }
    fn mcfg_writer_gates(&self,i:usize)->u32{
        let t=&self.transmit[i];
        u32::from(self.remote_for_transmit(t).is_some())
        |(u32::from(self.ready_remote_for_transmit(t).is_some())<<1)
        |(u32::from(matches!(self.outbound_intent_request,OutboundIntentRequest::Waiting{..}))<<2)
        |(u32::from(self.unsent_flow_predecessor(t))<<3)
        |(u32::from(self.intent_request_ack==Some(true))<<4)
        |(u32::from(self.intent_request_ack==Some(false))<<5)
        |(u32::from(self.intent_ack_backing.is_some())<<6)
        |(u32::from(self.receive.iter().any(|r|r.done_pending))<<7)
        |(u32::from(self.post_elf_mcfg.as_ref().and_then(|c|c.mcfg.as_ref()).is_some_and(|m|m.close_at.is_some()))<<8)
    }
    pub(crate) fn begin_mcfg_writer_pass(&mut self){
        let Some(i)=self.mcfg_writer_slot()else{return;};let gates=self.mcfg_writer_gates(i);let at=self.intent_ack_clock.map(|c|c());
        let c=self.post_elf_mcfg.as_mut().unwrap();let a=&mut c.mcfg.as_mut().unwrap().admission;
        let n=a.passes;a.passes=a.passes.saturating_add(1);let index=if n<8{n as usize}else{a.records.copy_within(5..8,4);a.omitted=a.omitted.saturating_add(1);7};
        a.records[index]=Some(McfgWriterRecord{start:at,gates,parameter:u32::MAX,source:u32::MAX,destination:u32::MAX,..McfgWriterRecord::default()});a.active=Some(index);c.revision=c.revision.saturating_add(1);
    }
    pub(crate) fn mcfg_writer_header(&mut self,b:&[u8],result:Result<(),WriteError>){
        let Some(slot)=self.mcfg_writer_slot()else{return;};let gates=self.mcfg_writer_gates(slot);let at=self.intent_ack_clock.map(|c|c());
        let c=self.post_elf_mcfg.as_mut().unwrap();let a=&mut c.mcfg.as_mut().unwrap().admission;let Some(i)=a.active else{return;};let r=a.records[i].as_mut().unwrap();
        r.end=at;r.gates|=gates<<16;r.result=match result{Ok(())=>1,Err(WriteError::WouldBlock)=>2,Err(WriteError::Uncertain)=>3};
        if b.len()>=8{r.command=u32::from(u16::from_le_bytes([b[0],b[1]]));r.parameter=word(b,4);}
        if matches!(r.command,9|12)&&b.len()>=48{r.kind=word(b,20);r.source=word(b,28);r.destination=word(b,44);}
        c.revision=c.revision.saturating_add(1);
    }
    pub(crate) fn end_mcfg_writer_pass(&mut self,choice:PublishChoice){
        let Some(slot)=self.mcfg_writer_slot()else{return;};let gates=self.mcfg_writer_gates(slot);let at=self.intent_ack_clock.map(|c|c());
        let c=self.post_elf_mcfg.as_mut().unwrap();let a=&mut c.mcfg.as_mut().unwrap().admission;let Some(i)=a.active.take()else{return;};let r=a.records[i].as_mut().unwrap();
        if r.end.is_none(){r.end=at;r.gates|=gates<<16;if r.result==0{r.result=4;}}
        if matches!(choice,PublishChoice::Data(i)if i==slot){r.gates|=1<<25;}c.revision=c.revision.saturating_add(1);
    }
    pub(crate) fn yield_mcfg_writer_pass(&mut self){
        if let Some(a)=self.post_elf_mcfg.as_mut().and_then(|c|c.mcfg.as_mut()).map(|m|&mut m.admission) && let Some(i)=a.active {a.records[i].as_mut().unwrap().result=5;}
    }
}
const _:()={assert!(::core::mem::size_of::<PostElfMcfgTrace>()<=3072);};

impl Session {
    pub fn post_elf_mcfg(&self)->Option<&PostElfMcfgTrace>{self.post_elf_mcfg.as_ref()}
    pub(crate) fn post_mcfg_watches(&self,h:&TftpHandle)->bool{self.post_elf_mcfg.as_ref().and_then(|c|c.mcfg.as_ref()).is_some_and(|m|m.handle==*h && m.server_result.is_none())}
    pub(super) fn start_post_elf_mcfg(&mut self,h:RequestHandle,index:usize){
        if self.post_elf_mcfg.is_some(){return;}
        let rx=&self.receive[index];let at=self.intent_ack_clock.map(|c|c());
        let mut cap=PostElfMcfgTrace{elf:h,local:word(&rx.bytes,28),peer:word(&rx.bytes,12),revision:1,mcfg:None,journal:[None;16],total:0,omitted:0,bad:0,other:None};
        cap.wire(&rx.bytes[..rx.used],false,at);self.post_elf_mcfg=Some(cap);
    }
    pub(super) fn observe_post_mcfg_wire(&mut self,index:usize){
        if let Some(cap)=self.post_elf_mcfg.as_mut(){cap.wire(&self.receive[index].bytes[..self.receive[index].used],false,self.intent_ack_clock.map(|c|c()));}
    }
    pub(super) fn observe_post_mcfg_receive(&mut self,index:usize,h:TftpHandle){
        let after=self.elf_exchange.is_some_and(|e|e.completed);
        let Some(cap)=self.post_elf_mcfg.as_mut()else{return;};
        let rx=&self.receive[index];let Some(end)=32usize.checked_add(word(&rx.bytes,20)as usize)else{return;};let Some(body)=rx.bytes.get(32..end).filter(|_|end<=rx.used)else{return;};
        let source=Address{node:word(&rx.bytes,8),port:word(&rx.bytes,12)};
        let local=word(&rx.bytes,28);let op=body.get(..2).map(|b|u16::from_be_bytes([b[0],b[1]])).unwrap_or(0);
        if after && cap.other.is_none() && local==0x5000 && matches!(op,1|2) && matches!(category(body),5..=8){cap.other=Some([category(body),source.port,body.len()as u32,u32::from(op)]);cap.revision=cap.revision.saturating_add(1);}
        if after && cap.mcfg.is_none() && local==0x5000 && body.starts_with(b"\0\x01/readwrite/mcfg.tmp\0") {
            cap.mcfg=Some(PostMcfgRequest{handle:h,slot:None,peer:source,bytes:body.len()as u32,flags:1,options:McfgOptionIds::read(body,true),file_before:None,server_result:None,
                reply_opcode:u32::MAX,reply_number:u32::MAX,reply_bytes:0,reply_options:McfgOptionIds::default(),transfer_port:u32::MAX,remote_intent:u32::MAX,
                received:self.intent_ack_clock.map(|c|c()),dispatched:None,queued:None,published:None,done:None,first_client:None,last_client:None,first_client_at:None,last_client_at:None,
                #[cfg(feature="sunfish-mcfg-record-identity")]
                record:None,
                #[cfg(feature="sunfish-mcfg-writer-trace")]
                admission:McfgAdmissionTrace::default(),
                #[cfg(feature="sunfish-mcfg-record-identity")]
                canceled_at:None,
                client_counts:[0;3],close_at:None,close_port:u32::MAX,queue_waits:[0;2]});cap.revision=cap.revision.saturating_add(1);return;
        }
        let Some(m)=cap.mcfg.as_mut().filter(|m|m.peer==source && m.flags&128==0)else{return;};
        if op==1 && local==0x5000 {m.client_counts[2]=m.client_counts[2].saturating_add(1);cap.revision=cap.revision.saturating_add(1);}
        if local!=m.transfer_port{return;}
        let num=match op {4=>crate::qcom_tftp::decode_ack(body).ok(),5=>crate::qcom_tftp::decode_error(body).ok().map(|e|e.0),_=>None};
        if let Some(num)=num {let id=(u32::from(op)<<16)|u32::from(num);let at=self.intent_ack_clock.map(|c|c());
            if m.first_client.is_none(){m.first_client=Some(id);m.first_client_at=at;}
            m.last_client=Some(id);m.last_client_at=at;m.flags|=64;m.client_counts[usize::from(op==5)]=m.client_counts[usize::from(op==5)].saturating_add(1);cap.revision=cap.revision.saturating_add(1);}
    }
    pub(crate) fn observe_post_mcfg_dispatch(&mut self,h:&TftpHandle,before:Option<usize>,result:Result<crate::qcom_tftp_server_check::Reply,crate::qcom_tftp_server_check::Error>,bytes:&[u8]){
        let Some(cap)=self.post_elf_mcfg.as_mut()else{return;};
        let Some(m)=cap.mcfg.as_mut().filter(|m|m.handle==*h && m.server_result.is_none())else{return;};
        m.file_before=before.map(|n|n as u32);m.server_result=Some(result.err().map_or(0,|e|e.code()));m.flags|=2;m.dispatched=self.intent_ack_clock.map(|c|c());
        if let Ok(reply)=result && reply.len!=0 {m.flags|=4;m.reply_bytes=reply.len as u32;m.transfer_port=reply.route.source.port;
            if let Some(p)=bytes.get(..reply.len).filter(|p|p.len()>=2){m.reply_opcode=u32::from(u16::from_be_bytes([p[0],p[1]]));
                m.reply_number=if matches!(m.reply_opcode,3|4|5){p.get(2..4).map_or(u32::MAX,|b|u32::from(u16::from_be_bytes([b[0],b[1]])))}else{u32::MAX};
                if m.reply_opcode==6 {m.reply_options=McfgOptionIds::read(p,false);}
            }
        }
        cap.revision=cap.revision.saturating_add(1);
    }
    pub(super) fn observe_post_mcfg_queue(&mut self,h:&TftpHandle,route:TftpReplyRoute,result:Result<usize,Error>){
        let Some(cap)=self.post_elf_mcfg.as_mut()else{return;};
        let Some(m)=cap.mcfg.as_mut().filter(|m|m.handle==*h && m.flags&4!=0 && m.flags&8==0 && m.peer==route.destination && m.transfer_port==route.source.port)else{return;};
        #[cfg(feature="sunfish-mcfg-writer-trace")]
        {
            let a=&mut m.admission;let at=self.intent_ack_clock.map(|c|c());
            if a.attempts==0{a.first_attempt=at;}a.last_attempt=at;a.attempts=a.attempts.saturating_add(1);
            if result.is_err(){let b=self.tftp_queue_block;a.block_at=at;a.block=[u32::from(b.check),u32::from(b.kind),u32::from(b.opcode),u32::from(b.flags)];
                let tx=self.transmit.iter().find(|tx|tx.len!=0&&tx.kind.qrtr_addresses().is_some_and(|(_,p)|p==m.peer));
                a.block_len=tx.map_or(0,|t|t.len as u32);a.block_sent=tx.map_or(0,|t|t.sent as u32);a.block_sequence=tx.and_then(|t|t.reference_flow.map(|r|r.sequence()));
            }
        }
        match result {Ok(slot)=>{m.slot=Some(slot);m.flags|=8;m.queued=self.intent_ack_clock.map(|c|c());
            #[cfg(feature="sunfish-mcfg-record-identity")]
            {m.record=self.transmit[slot].reference_flow;if m.record.is_none(){m.slot=None;m.flags|=512;}}
        },Err(Error::FlowBusy)=>m.queue_waits[0]=m.queue_waits[0].saturating_add(1),Err(Error::NoCapacity)=>m.queue_waits[1]=m.queue_waits[1].saturating_add(1),_=>{}}
        cap.revision=cap.revision.saturating_add(1);
    }
    pub(super) fn observe_post_mcfg_publication(&mut self,index:usize,remote:usize){
        let Some(cap)=self.post_elf_mcfg.as_mut()else{return;};let tx=&self.transmit[index];let at=self.intent_ack_clock.map(|c|c());
        cap.wire(&tx.bytes[..tx.len],true,at);
        if let Some(m)=cap.mcfg.as_mut().filter(|m|m.slot==Some(index) && m.flags&16==0
            && {
                #[cfg(feature="sunfish-mcfg-record-identity")]
                {mcfg_record_matches(m,tx,self.config.local_node)}
                #[cfg(not(feature="sunfish-mcfg-record-identity"))]
                {true}
            }){m.flags|=16;m.published=at;m.remote_intent=self.remote[remote].as_ref().map_or(u32::MAX,|i|i.id);cap.revision=cap.revision.saturating_add(1);}
    }
    pub(super) fn observe_post_mcfg_done(&mut self,index:usize,id:u32){
        let Some(cap)=self.post_elf_mcfg.as_mut()else{return;};let at=self.intent_ack_clock.map(|c|c());
        if let Some(m)=cap.mcfg.as_mut().filter(|m|m.slot==Some(index) && m.flags&16!=0 && m.flags&32==0 && m.remote_intent==id
            && {
                #[cfg(feature="sunfish-mcfg-record-identity")]
                {mcfg_record_matches(m,&self.transmit[index],self.config.local_node)}
                #[cfg(not(feature="sunfish-mcfg-record-identity"))]
                {true}
            }){m.flags|=32;m.done=at;m.slot=None;
            let record=McfgWireRecord{at,tx:false,kind:254,source:m.peer.port,destination:m.transfer_port,tag:14,a:id,b:1};cap.push(record);}
    }
    #[cfg(feature="sunfish-mcfg-record-identity")]
    pub(super) fn observe_post_mcfg_cancel(&mut self,index:usize){
        let Some(cap)=self.post_elf_mcfg.as_mut()else{return;};
        let Some(m)=cap.mcfg.as_mut().filter(|m|m.slot==Some(index)&&m.flags&16==0
            && mcfg_record_matches(m,&self.transmit[index],self.config.local_node))else{return;};
        m.slot=None;m.flags|=256;m.canceled_at=self.intent_ack_clock.map(|clock|clock());
        let record=McfgWireRecord{at:m.canceled_at,tx:false,kind:253,source:m.peer.port,
            destination:m.transfer_port,tag:14,a:1,b:m.reply_bytes};cap.push(record);
    }
    pub(super) fn observe_post_mcfg_close(&mut self,port:u32){
        if let Some(cap)=self.post_elf_mcfg.as_mut() && let Some(m)=cap.mcfg.as_mut().filter(|m|m.peer.port==port && m.flags&128==0){m.flags|=128;m.close_port=port;m.close_at=self.intent_ack_clock.map(|c|c());cap.revision=cap.revision.saturating_add(1);}
    }
    pub(super) fn observe_post_mcfg_elf_done(&mut self,event:Event){
        if let Some(cap)=self.post_elf_mcfg.as_mut() && event==Event::ReplyRxDone(cap.elf){cap.push(McfgWireRecord{at:self.intent_ack_clock.map(|c|c()),tx:false,kind:254,source:cap.peer,destination:cap.local,tag:14,a:cap.elf.intent,b:0});}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn post_mcfg_options_preserve_zero_presence_and_diagnostic_failures(){
        let prefix=b"\0\x01/readwrite/mcfg.tmp\0octet\0";
        let read=|tail:&[u8]|{let mut v=prefix.to_vec();v.extend(tail);McfgOptionIds::read(&v,true)};
        let t=read(b"tsize\x000\0rsize\x000\0");assert_eq!(t.present,24);assert_eq!((t.values[3],t.values[4],t.flags),(0,0,0));
        let t=read(b"rsize\x004294967295\0");assert_eq!((t.present,t.values[4],t.flags),(16,u32::MAX,0));
        let t=read(b"rsize\x004294967296\0");assert_eq!((t.present,t.values[4],t.flags),(16,u32::MAX,16));
        let t=read(b"tsize\x000\0tsize\x001\0");assert_eq!((t.values[3],t.flags),(0,4));
        let t=read(b"unlink\x000\0vendor\x001\0");assert_eq!(t.values[6],0);assert_eq!(t.unknown_count,2);assert_eq!(t.unknown_key,0x756e6c69);assert_eq!(t.flags,8);
        assert_ne!(read(b"tsize\x00x\0").flags&2,0);assert_ne!(read(b"tsize\0").flags&1,0);
        assert_eq!(McfgOptionIds::read(&[0;513],true).flags,32);
        assert_eq!(McfgOptionIds::read(b"\0\x01x\0netascii\0",true).flags,64);
        let r=McfgOptionIds::read(b"\0\x06blksize\x00512\0timeoutms\x001000\0tsize\x001\0wsize\x001\0",false);
        assert_eq!(r.values[..4],[512,1000,1,1]);assert_eq!((r.present,r.flags),(15,0));
    }
    fn qrtr(kind:u32,source:u32,destination:u32,body:&[u8])->Vec<u8>{
        let mut out=vec![0;32];for(i,n)in[1,kind,0,source,0,body.len()as u32,1,destination].iter().enumerate(){out[i*4..i*4+4].copy_from_slice(&n.to_le_bytes());}out.extend(body);out
    }
    #[test]
    fn post_mcfg_journal_has_bounded_head_tail_and_named_control_keys(){
        let elf=RequestHandle{owner:1,generation:1,intent:1};let mut t=PostElfMcfgTrace{elf,local:0x4003,peer:9,revision:1,mcfg:None,journal:[None;16],total:0,omitted:0,bad:0,other:None};
        for i in 0u32..40 {t.wire(&qrtr(1,0x4a,0x5000,&[0,4,(i>>8)as u8,i as u8]),false,Some(u64::from(i)));}
        assert_eq!((t.total,t.omitted),(40,24));assert_eq!(t.journal[0].unwrap().at,Some(0));assert_eq!(t.journal[7].unwrap().at,Some(7));assert_eq!(t.journal[8].unwrap().at,Some(32));assert_eq!(t.journal[15].unwrap().at,Some(39));
        let b:[u8;20]=[6u32,0,0x4a,0,0].into_iter().flat_map(u32::to_le_bytes).collect::<Vec<_>>().try_into().unwrap();
        let h=packet(&qrtr(6,u32::MAX-1,u32::MAX-1,&b),false,None).unwrap();assert_eq!((h.tag,h.a,h.b),(13,0,0x4a));
        let before=t.total;let b:[u8;20]=[4u32,999,1,0,0x21].into_iter().flat_map(u32::to_le_bytes).collect::<Vec<_>>().try_into().unwrap();t.wire(&qrtr(4,0xfffffffe,0xfffffffe,&b),false,None);assert_eq!(t.total,before);
        let mut broken=qrtr(1,9,0x4003,&[]);broken[20..24].copy_from_slice(&u32::MAX.to_le_bytes());assert!(packet(&broken,false,None).is_none());
    }

    #[cfg(feature="sunfish-mcfg-record-identity")]
    #[test]
    fn post_mcfg_identity_unique_token_rejects_same_route_and_body_reuse(){
        let peer=Address{node:0,port:0x4a};let mut table=crate::qcom_qrtr_flow::Table::<8>::new(1).unwrap();let destination=crate::qcom_qrtr_flow::Destination{node:0,port:0x4a};let first=table.reserve(destination).unwrap();let second=table.reserve(destination).unwrap();assert_ne!(first,second);
        let mut m=PostMcfgRequest{handle:TftpHandle{owner:1,generation:1,intent:1},slot:Some(0),peer,bytes:73,flags:15,options:Default::default(),file_before:None,server_result:Some(0),reply_opcode:5,reply_number:1,reply_bytes:19,reply_options:Default::default(),transfer_port:0x5001,remote_intent:u32::MAX,received:None,dispatched:None,queued:None,published:None,done:None,first_client:None,last_client:None,first_client_at:None,last_client_at:None,client_counts:[0;3],close_at:None,close_port:u32::MAX,queue_waits:[0;2],record:Some(first),#[cfg(feature="sunfish-mcfg-writer-trace")] admission:McfgAdmissionTrace::default(),canceled_at:None};
        let mut tx=Transmit::empty();tx.kind=TxKind::TftpReply{source:Address{node:1,port:0x5001},destination:peer};tx.len=51;tx.bytes[4..8].copy_from_slice(&1u32.to_le_bytes());tx.bytes[20..24].copy_from_slice(&19u32.to_le_bytes());tx.reference_flow=Some(second);assert!(!mcfg_record_matches(&m,&tx,1));
        tx.reference_flow=Some(first);assert!(mcfg_record_matches(&m,&tx,1));m.flags|=256;assert!(!mcfg_record_matches(&m,&tx,1));m.flags&=!256;m.record=None;assert!(!mcfg_record_matches(&m,&tx,1));m.record=Some(first);tx.kind=TxKind::Resume;assert!(!mcfg_record_matches(&m,&tx,1));
    }
}
