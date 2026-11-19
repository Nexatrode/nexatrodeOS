include!("sunfish_glink_open_test.rs");
#[path="../rust/src/pixel_glink_channel.rs"] mod pixel_glink_channel;
#[cfg(feature="sunfish-glink-intents-probe")]
const BOUNDED_RX_BYTES: usize = 512;
#[cfg(not(feature="sunfish-glink-intents-probe"))]
const BOUNDED_RX_BYTES: usize = 128;
#[cfg(feature="sunfish-glink-intents-probe")]
const BOUNDED_DESC_LEN: u32 = 0x280;
#[cfg(not(feature="sunfish-glink-intents-probe"))]
const BOUNDED_DESC_LEN: u32 = 0x248;
#[cfg(feature="sunfish-glink-intents-probe")]
const BOUNDED_AVAILABLE: u32 = 0x278;
#[cfg(not(feature="sunfish-glink-intents-probe"))]
const BOUNDED_AVAILABLE: u32 = 0x240;
#[cfg(feature="sunfish-glink-intents-probe")]
const BOUNDED_RECORDS: u32 = 64;
#[cfg(not(feature="sunfish-glink-intents-probe"))]
const BOUNDED_RECORDS: u32 = 16;
fn channel_fixture()->(Vec<u8>,pixel_glink_open::OpenCheckpoint,qcom_modem_status::Item){
    channel_fixture_for_generation(1)
}
fn channel_fixture_for_generation(generation:u64)->(Vec<u8>,pixel_glink_open::OpenCheckpoint,qcom_modem_status::Item){
    let(mut m,checkpoint,rx)=open_fixture_for_generation(generation);let mut lock=0;let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let mut io=OpenIo::new(&mut m,&lock,rx.offset);
    let next=match pixel_glink_open::checkpoint_run(&mut w,checkpoint,&mut io,&mut Time{now:0,step:50000},|_|{}){Ok(s)=>s,Err(r)=>panic!("request checkpoint: {r:?}")};(m,next,rx)
}
struct ChannelIo{base:*mut u8,lock:*const u32,rx:usize,bytes:usize,heads:usize,notifications:usize,fault:u8}
impl ChannelIo{fn new(m:&mut[u8],lock:*const u32,rx:usize)->Self{Self{base:m.as_mut_ptr(),lock,rx,bytes:0,heads:0,notifications:0,fault:0}}}
fn intent_bytes()->[u8;24]{[5,0,2,0,2,0,0,0,0xff,0xff,0xff,0xff,0xff,0xff,0xff,0xff,0,0,0,0,0,0,0,0]}
impl pixel_glink_channel::Io for ChannelIo{
    fn byte(&mut self,i:usize,v:u8){assert_eq!(unsafe{*self.lock},1);assert_eq!(i,self.bytes);assert_eq!(v,pixel_glink_channel::ACK[i]);assert_eq!(self.heads,0);self.bytes+=1;unsafe{if !(self.fault==1 && i==0){self.base.add(TX.offset+32+i).write_volatile(v)}if self.fault==7 && i==7{let m=std::slice::from_raw_parts_mut(self.base,SMEM_BYTES);let p=smem_item(m,435).unwrap();w32(m,p.offset+36,0)}}}
    fn head(&mut self){assert_eq!(unsafe{*self.lock},1);self.heads+=1;if self.fault!=2{unsafe{self.base.add(DESCRIPTOR.offset+4).cast::<u32>().write_volatile(40u32.to_le())}}}
    fn notify(&mut self){assert_eq!(unsafe{*self.lock},0);self.notifications+=1;unsafe{let m=std::slice::from_raw_parts_mut(self.base,SMEM_BYTES);if self.fault==3{return}w32(m,DESCRIPTOR.offset,40);if self.fault==9{w32(m,DESCRIPTOR.offset+12,BOUNDED_DESC_LEN);for chunk in m[self.rx+96..self.rx+96+BOUNDED_RX_BYTES].chunks_exact_mut(8){chunk.copy_from_slice(&[13,0,0,0,0,0,0,0]);}}else{w32(m,DESCRIPTOR.offset+12,120);m[self.rx+96..self.rx+120].copy_from_slice(&intent_bytes());}if self.fault==4{m[self.rx+98]=1}if self.fault==5{w32(m,DESCRIPTOR.offset,48)}if self.fault==6{let p=smem_item(m,435).unwrap();w32(m,p.offset+36,1)}if self.fault==8{w32(m,DESCRIPTOR.offset+12,96)}}}
}
#[test]
fn actual_request_continuation_gates_one_peer_ack_and_opaque_intent_snapshot(){
    let(mut m,next,rx)=channel_fixture();let before=m.clone();let mut lock=0;let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let mut io=ChannelIo::new(&mut m,&lock,rx.offset);let r=pixel_glink_channel::run(&mut w,next,&mut io,&mut Time{now:0,step:50000},|_|{});
    assert!(r.passed(),"{r:?}");assert_eq!(r.words,[40,40,8,120,0,0,0,0]);assert_eq!((r.records,r.intent_records,r.intent_entries),(1,1,2));assert_eq!((io.bytes,io.heads,io.notifications),(8,1,1));assert_eq!(r.byte_writes,8);assert_eq!(r.guard_stage,8);assert_eq!(&m[TX.offset+32..TX.offset+40],&pixel_glink_channel::ACK);assert_eq!(lock,0);
    for i in 0..m.len(){if !(TX.offset+32..TX.offset+40).contains(&i) && !(DESCRIPTOR.offset..DESCRIPTOR.offset+8).contains(&i) && !(DESCRIPTOR.offset+12..DESCRIPTOR.offset+16).contains(&i) && !(rx.offset+96..rx.offset+120).contains(&i){assert_eq!(m[i],before[i],"unexpected write {i:x}");}}
}
#[test]
fn failed_open_request_cannot_mint_channel_continuation(){
    let(mut m,checkpoint,rx)=open_fixture();let mut lock=0;let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let mut io=OpenIo::new(&mut m,&lock,rx.offset);io.fault=4;
    assert!(matches!(pixel_glink_open::checkpoint_run(&mut w,checkpoint,&mut io,&mut Time{now:0,step:50000},|_|{}),Err(r)if r.error==Some(Error::Payload)));
}
#[test]
fn changed_old_offer_ack_indices_unused_tx_or_route_block_peer_ack_stores(){
    for fault in 0..6{let(mut m,next,rx)=channel_fixture();match fault{0=>m[rx.offset+40]^=1,1=>m[rx.offset+88+2]=2,2=>w32(&mut m,DESCRIPTOR.offset+8,16),3=>w32(&mut m,DESCRIPTOR.offset+12,104),4=>m[TX.offset+40]=1,_=>m[PART+48+20]^=1};let before=m.clone();let mut lock=0;let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let mut io=ChannelIo::new(&mut m,&lock,rx.offset);let r=pixel_glink_channel::run(&mut w,next,&mut io,&mut Time{now:0,step:50000},|_|{});assert!(r.error.is_some());assert_eq!((io.bytes,io.heads,io.notifications),(0,0,0));assert_eq!(m,before);assert_eq!(lock,0);}
}
#[test]
fn partial_ack_publication_and_invalid_reply_stop_without_retry_or_rx_store(){
    for fault in 1..=7{let(mut m,next,rx)=channel_fixture();let mut lock=0;let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let mut io=ChannelIo::new(&mut m,&lock,rx.offset);io.fault=fault;let r=pixel_glink_channel::run(&mut w,next,&mut io,&mut Time{now:0,step:50000},|_|{});
        assert!(!r.passed());assert_eq!(r.error,Some(match fault{1=>Error::Publication,2|5=>Error::Outbound,3=>Error::Timeout,4=>Error::Payload,6=>Error::Fatal,_=>Error::ReadyLost}));assert_eq!(io.bytes,8);assert_eq!(io.heads,usize::from(![1,7].contains(&fault)));assert_eq!(io.notifications,usize::from((3..=6).contains(&fault)));assert_eq!(r.byte_writes,8);assert_eq!(u32::from_le_bytes(m[DESCRIPTOR.offset+8..DESCRIPTOR.offset+12].try_into().unwrap()),8);assert_eq!(lock,0);
        if fault==4{assert_eq!(r.header,Some(pixel_glink_receive::Header{command:5,param1:1,param2:2}));}
    }
}
#[test]
fn ack_consumption_does_not_require_or_invent_an_intent(){
    let(mut m,next,rx)=channel_fixture();let mut lock=0;let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let mut io=ChannelIo::new(&mut m,&lock,rx.offset);io.fault=8;let r=pixel_glink_channel::run(&mut w,next,&mut io,&mut Time{now:0,step:50000},|_|{});assert!(r.passed());assert_eq!((r.new_bytes,r.intent_entries),(0,0));assert_eq!(r.header,None);
}
#[test]
fn bounded_capture_mints_receive_checkpoint_when_available_exceeds_capture_limit(){
    let(mut m,next,rx)=channel_fixture();let mut lock=0;let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let mut io=ChannelIo::new(&mut m,&lock,rx.offset);io.fault=9;
    let checkpoint=match pixel_glink_channel::checkpoint_run(&mut w,next,&mut io,&mut Time{now:0,step:50000},|_|{}){Ok(checkpoint)=>checkpoint,Err(report)=>panic!("bounded receive checkpoint: {report:?}")};
    let(_,_,_,capture,report)=checkpoint.into_parts();assert_eq!(report.available,BOUNDED_AVAILABLE);assert_eq!(report.new_bytes,pixel_glink_channel::MAX_NEW_RX as u32);assert_eq!(capture.size,pixel_glink_channel::MAX_NEW_RX);assert_eq!(report.records,BOUNDED_RECORDS);
}
#[test]
fn control_snapshot_bounds_reject_truncation_unknown_channels_and_data(){
    assert_eq!(pixel_glink_channel::controls(&intent_bytes()),Ok((1,1,2)));assert_eq!(pixel_glink_channel::controls(&[]),Ok((0,0,0)));
    for b in [&[5,0,2,0,1,0,0,0][..],&[5,0,2,0,16,0,0,0][..],&[5,0,1,0,1,0,0,0][..],&[9,0,2,0,0,0,0,0][..],&[4,0,1,0,0,0,0,0][..],&[15,0,1,0,0,0,0,0][..],&[0;136][..]]{assert!(pixel_glink_channel::controls(b).is_err());}
}
#[test]
fn native_peer_ack_adapter_has_exact_writes_and_terminal_reuse(){
    let(mut m,next,rx)=channel_fixture();let before=m.clone();let mut locks=[0x12,0,0x34];let mut door=[0x11,0x22,0x33,0,0x55];
    struct Peer{base:*mut u8,door:*const u32,rx:usize,now:u64}impl Clock for Peer{fn frequency(&self)->u64{1_000_000}fn now(&mut self)->u64{self.now+=50000;unsafe{if self.door.add(3).read_volatile()==1<<12{let m=std::slice::from_raw_parts_mut(self.base,SMEM_BYTES);w32(m,DESCRIPTOR.offset,40);w32(m,DESCRIPTOR.offset+12,120);m[self.rx+96..self.rx+120].copy_from_slice(&intent_bytes());}}self.now}}
    let mut clock=Peer{base:m.as_mut_ptr(),door:door.as_ptr(),rx:rx.offset,now:0};let mut w=unsafe{Window::new(m.as_ptr(),locks.as_mut_ptr().add(1))};let mut once=unsafe{pixel_glink_channel::ChannelOnce::new(next,m.as_mut_ptr().add(PART),door.as_mut_ptr())};let r=once.run(&mut w,&mut clock,|_|{});assert!(r.passed());assert_eq!(door,[0x11,0x22,0x33,1<<12,0x55]);assert_eq!(locks,[0x12,0,0x34]);
    for i in 0..m.len(){if !(TX.offset+32..TX.offset+40).contains(&i) && !(DESCRIPTOR.offset..DESCRIPTOR.offset+8).contains(&i) && !(DESCRIPTOR.offset+12..DESCRIPTOR.offset+16).contains(&i) && !(rx.offset+96..rx.offset+120).contains(&i){assert_eq!(m[i],before[i],"native ACK changed {i:x}");}}
    let completed=m.clone();assert_eq!(once.run(&mut w,&mut clock,|_|{}).error,Some(Error::Outbound));assert_eq!(m,completed);
}
#[test]
fn stopped_clock_blocks_ack_and_consumes_native_adapter(){
    struct Stopped;impl Clock for Stopped{fn frequency(&self)->u64{1_000_000}fn now(&mut self)->u64{0}}
    let(mut m,next,_)=channel_fixture();let before=m.clone();let mut lock=0;let mut door=[0;4];let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let mut once=unsafe{pixel_glink_channel::ChannelOnce::new(next,m.as_mut_ptr().add(PART),door.as_mut_ptr())};assert_eq!(once.run(&mut w,&mut Stopped,|_|{}).error,Some(Error::Clock));assert_eq!(once.run(&mut w,&mut Stopped,|_|{}).error,Some(Error::Outbound));assert_eq!(m,before);assert_eq!(door,[0;4]);
}
#[test]
fn readiness_lost_during_reply_verification_cannot_claim_ack_consumed(){
    let(mut m,next,rx)=channel_fixture();let(ready,base,old,_)=next.into_parts();m[TX.offset+32..TX.offset+40].copy_from_slice(&pixel_glink_channel::ACK);w32(&mut m,DESCRIPTOR.offset,40);w32(&mut m,DESCRIPTOR.offset+4,40);let peer=smem_item(&m,435).unwrap();let before=m.clone();
    struct Revoked<'a>{m:&'a[u8],peer:usize,seen:std::cell::Cell<bool>}impl Read for Revoked<'_>{fn read8(&self,at:usize)->u8{if at==TX.offset+TX.size-1{self.seen.set(true)}if at==self.peer+36 && self.seen.get(){return 0}self.m[at]}}
    let spy=Revoked{m:&m,peer:peer.offset,seen:std::cell::Cell::new(false)};let mut r=pixel_glink_channel::Report{error:None,staged:true,head_store:true,notifications:1,tx_consumed:false,words:[0;8],available:0,new_bytes:0,header:None,records:0,intent_records:0,intent_entries:0,samples:0,byte_writes:8,guard_stage:8,#[cfg(feature="sunfish-glink-intents-probe")] intents:None,#[cfg(feature="sunfish-glink-intents-probe")] intent_scan:None};
    assert_eq!(pixel_glink_channel::sample(&spy,&base,&ready,&old,32,96,&mut r),Err(Error::ReadyLost));assert!(!r.tx_consumed);assert_eq!(m,before);assert_eq!(rx.size,0x4000);
}
#[test]
fn reply_copies_stay_inside_bound_and_reject_unstable_last_byte(){
    for unstable in [false,true]{let(mut m,next,rx)=channel_fixture();let(ready,base,old,_)=next.into_parts();m[TX.offset+32..TX.offset+40].copy_from_slice(&pixel_glink_channel::ACK);w32(&mut m,DESCRIPTOR.offset,40);w32(&mut m,DESCRIPTOR.offset+4,40);w32(&mut m,DESCRIPTOR.offset+12,224);for chunk in m[rx.offset+96..rx.offset+224].chunks_exact_mut(8){chunk.copy_from_slice(&[13,0,0,0,0,0,0,0]);}let before=m.clone();
        struct Bounded<'a>{m:&'a[u8],rx:usize,unstable:bool,reads:std::cell::Cell<u32>}impl Read for Bounded<'_>{fn read8(&self,at:usize)->u8{if(self.rx..self.rx+0x4000).contains(&at){assert!(at<self.rx+96+128)}if at==self.rx+96+127{let n=self.reads.get();self.reads.set(n+1);if self.unstable && n>0{return self.m[at]^1}}self.m[at]}}
        let spy=Bounded{m:&m,rx:rx.offset,unstable,reads:std::cell::Cell::new(0)};let mut r=pixel_glink_channel::Report{error:None,staged:true,head_store:true,notifications:1,tx_consumed:false,words:[0;8],available:0,new_bytes:0,header:None,records:0,intent_records:0,intent_entries:0,samples:0,byte_writes:8,guard_stage:8,#[cfg(feature="sunfish-glink-intents-probe")] intents:None,#[cfg(feature="sunfish-glink-intents-probe")] intent_scan:None};
        let result=pixel_glink_channel::sample(&spy,&base,&ready,&old,32,96,&mut r);if unstable{assert_eq!(result,Err(Error::Unstable));assert!(!r.tx_consumed)}else{assert_eq!(result,Ok(()));assert_eq!(r.records,16);assert!(r.tx_consumed)}assert_eq!(m,before);
    }
}
