include!("sunfish_glink_intents_test.rs");
#[path="../rust/src/pixel_glink_rx.rs"] mod pixel_glink_rx;
fn observed_shape_controls()->Vec<u8>{let mut b=vec![];for id in 0..30{add_intents(&mut b,&[(if id<16{128}else if id<24{512}else if id<29{1152}else{8320},id)]);}b.extend_from_slice(&[7,0,2,0,52,0,0,0]);assert_eq!(b.len(),488);b}
fn receive_fixture()->(Vec<u8>,pixel_glink_channel::ChannelCheckpoint,qcom_modem_status::Item){
    let(mut m,next,rx)=channel_fixture();let mut lock=0;let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};
    struct InventoryIo(ChannelIo);impl pixel_glink_channel::Io for InventoryIo{fn byte(&mut self,i:usize,v:u8){self.0.byte(i,v)}fn head(&mut self){self.0.head()}fn notify(&mut self){self.0.notify();unsafe{let m=std::slice::from_raw_parts_mut(self.0.base,SMEM_BYTES);m[self.0.rx+96..self.0.rx+584].copy_from_slice(&observed_shape_controls());w32(m,DESCRIPTOR.offset+12,584);}}}
    let mut io=InventoryIo(ChannelIo::new(&mut m,&lock,rx.offset));let cap=match pixel_glink_channel::checkpoint_run(&mut w,next,&mut io,&mut Time{now:0,step:50000},|_|{}){Ok(c)=>c,Err(r)=>panic!("receive fixture checkpoint: {r:?}")};(m,cap,rx)
}
fn data_packet()->[u8;72]{let mut b=[0xee;72];b[..16].copy_from_slice(&[9,0,2,0,1,0,0,0,52,0,0,0,0,0,0,0]);for(i,v)in [1u32,2,3,0xfffffffe,0,20,1,0xfffffffe,2,0,0,0,0].into_iter().enumerate(){b[16+4*i..20+4*i].copy_from_slice(&v.to_le_bytes());}b}
struct RxIo{base:*mut u8,lock:*const u32,rx:usize,bytes:usize,heads:usize,notifications:usize,fault:u8}
impl RxIo{fn new(m:&mut[u8],lock:*const u32,rx:usize)->Self{Self{base:m.as_mut_ptr(),lock,rx,bytes:0,heads:0,notifications:0,fault:0}}}
impl pixel_glink_rx::Io for RxIo{
    fn byte(&mut self,i:usize,v:u8){assert_eq!(unsafe{*self.lock},1);assert_eq!(i,self.bytes);assert_eq!(v,pixel_glink_rx::PACKETS[i]);assert_eq!(self.heads,0);self.bytes+=1;unsafe{if !(self.fault==1 && i==0){self.base.add(TX.offset+40+i).write_volatile(v)}if self.fault==7 && i==23{let m=std::slice::from_raw_parts_mut(self.base,SMEM_BYTES);let p=smem_item(m,435).unwrap();w32(m,p.offset+36,0)}}}
    fn head(&mut self){assert_eq!(unsafe{*self.lock},1);self.heads+=1;if self.fault!=2{unsafe{self.base.add(DESCRIPTOR.offset+4).cast::<u32>().write_volatile(64u32.to_le())}}}
    fn notify(&mut self){assert_eq!(unsafe{*self.lock},0);self.notifications+=1;unsafe{let m=std::slice::from_raw_parts_mut(self.base,SMEM_BYTES);if self.fault==3{return}w32(m,DESCRIPTOR.offset,64);w32(m,DESCRIPTOR.offset+12,656);m[self.rx+584..self.rx+656].copy_from_slice(&data_packet());match self.fault{4=>m[self.rx+588]=2,5=>w32(m,self.rx+592,65),6=>{let p=smem_item(m,435).unwrap();w32(m,p.offset+36,1)},8=>m[self.rx+100]^=1,9=>w32(m,self.rx+596,1),_=>{}}}}
}
#[test]
fn fixed_intent_and_request_ack_receive_one_guarded_packet_without_consuming_rx(){
    let(mut m,cap,rx)=receive_fixture();let before=m.clone();let mut lock=0;let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let mut io=RxIo::new(&mut m,&lock,rx.offset);let mut buffer=pixel_glink_rx::Buffer::new();let r=pixel_glink_rx::run(&mut w,cap,&mut buffer,&mut io,&mut Time{now:0,step:50000},|_|{});
    assert!(r.passed(),"{r:?}");assert_eq!((io.bytes,io.heads,io.notifications),(24,1,1));assert_eq!(r.words,[64,64,8,656,0,0,0,0]);assert_eq!((r.chunk,r.left,r.copied),(52,0,52));assert!(r.guards && r.qrtr_v1);assert_eq!(r.qrtr_words,[1,2,3,0xfffffffe,0,20,1,0xfffffffe]);assert_eq!(r.control_word,2);assert_eq!(&buffer.bytes()[..52],&data_packet()[16..68]);assert_eq!(&buffer.bytes()[52..],&[0;12]);assert!(buffer.guards());assert_eq!(lock,0);
    for i in 0..m.len(){if !(TX.offset+40..TX.offset+64).contains(&i) && !(DESCRIPTOR.offset..DESCRIPTOR.offset+8).contains(&i) && !(DESCRIPTOR.offset+12..DESCRIPTOR.offset+16).contains(&i) && !(rx.offset+584..rx.offset+656).contains(&i){assert_eq!(m[i],before[i],"unexpected receive write {i:x}");}}
}
#[test]
fn changed_complete_inventory_or_ready_indices_route_or_tx_prevent_advertisement(){
    for fault in 0..6{let(mut m,cap,rx)=receive_fixture();match fault{0=>m[rx.offset+100]^=1,1=>w32(&mut m,DESCRIPTOR.offset+8,16),2=>m[TX.offset+64]=1,3=>w32(&mut m,DESCRIPTOR.offset+12,592),4=>m[PART+48+20]^=1,_=>{let p=smem_item(&m,435).unwrap();w32(&mut m,p.offset+36,0)}}let before=m.clone();let mut lock=0;let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let mut io=RxIo::new(&mut m,&lock,rx.offset);let mut buffer=pixel_glink_rx::Buffer::new();let r=pixel_glink_rx::run(&mut w,cap,&mut buffer,&mut io,&mut Time{now:0,step:50000},|_|{});assert!(r.error.is_some());assert_eq!((io.bytes,io.heads,io.notifications),(0,0,0));assert!(buffer.empty());assert_eq!(m,before);}
}
#[test]
fn partial_publication_wrong_packet_ready_loss_and_timeout_never_write_buffer_or_retry(){
    for fault in 1..=9{let(mut m,cap,rx)=receive_fixture();let mut lock=0;let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let mut io=RxIo::new(&mut m,&lock,rx.offset);io.fault=fault;let mut buffer=pixel_glink_rx::Buffer::new();let r=pixel_glink_rx::run(&mut w,cap,&mut buffer,&mut io,&mut Time{now:0,step:50000},|_|{});assert!(!r.passed());assert_eq!(r.error,Some(match fault{1=>Error::Publication,2=>Error::Outbound,3=>Error::Timeout,4|5|9=>Error::Payload,6=>Error::Fatal,7=>Error::ReadyLost,_=>Error::Unstable}));assert_eq!(io.bytes,24);assert_eq!(io.heads,usize::from(![1,7].contains(&fault)));assert_eq!(io.notifications,usize::from(fault>=3 && fault!=7));assert!(buffer.empty());assert_eq!(u32::from_le_bytes(m[DESCRIPTOR.offset+8..DESCRIPTOR.offset+12].try_into().unwrap()),8);assert_eq!(lock,0);}
}
#[test]
fn damaged_guard_or_nonzero_buffer_prevents_any_receive_promise(){
    for offset in [0usize,16,95]{let(mut m,cap,rx)=receive_fixture();let before=m.clone();let mut lock=0;let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let mut io=RxIo::new(&mut m,&lock,rx.offset);let mut buffer=pixel_glink_rx::Buffer::new();unsafe{(&mut buffer as *mut pixel_glink_rx::Buffer).cast::<u8>().add(offset).write(1)}let r=pixel_glink_rx::run(&mut w,cap,&mut buffer,&mut io,&mut Time{now:0,step:50000},|_|{});assert_eq!(r.error,Some(Error::Publication));assert_eq!((io.bytes,io.heads,io.notifications),(0,0,0));assert_eq!(m,before);}
}
#[test]
fn packet_decoder_rejects_foreign_ids_fragmentation_oversize_and_bad_lengths(){
    for fault in 0..5{let mut b=data_packet();match fault{0=>b[0]=12,1=>b[2]=1,2=>b[4]=2,3=>b[8..12].copy_from_slice(&u32::MAX.to_le_bytes()),_=>b[12]=1};assert_eq!(pixel_glink_rx::packet(&b),Err(Error::Payload));}
    for len in 0..80{if len!=72{assert!(pixel_glink_rx::packet(&vec![0;len]).is_err());}}assert_eq!(pixel_glink_rx::packet(&data_packet()).unwrap().1,52);
}
#[test]
fn native_receive_adapter_preserves_canaries_and_is_terminal_after_success(){
    let(mut m,cap,rx)=receive_fixture();let before=m.clone();let mut locks=[0x11,0,0x33];let mut door=[0x44,0x55,0x66,0,0x88];let mut buffer=pixel_glink_rx::Buffer::new();
    struct Peer{base:*mut u8,door:*const u32,rx:usize,now:u64}impl Clock for Peer{fn frequency(&self)->u64{1_000_000}fn now(&mut self)->u64{self.now+=50000;unsafe{if self.door.add(3).read_volatile()==1<<12{let m=std::slice::from_raw_parts_mut(self.base,SMEM_BYTES);w32(m,DESCRIPTOR.offset,64);w32(m,DESCRIPTOR.offset+12,656);m[self.rx+584..self.rx+656].copy_from_slice(&data_packet());}}self.now}}
    let mut clock=Peer{base:m.as_mut_ptr(),door:door.as_ptr(),rx:rx.offset,now:0};let mut w=unsafe{Window::new(m.as_ptr(),locks.as_mut_ptr().add(1))};let mut once=unsafe{pixel_glink_rx::ReceiveOnce::new(cap,&mut buffer,m.as_mut_ptr().add(PART),door.as_mut_ptr())};let r=once.run(&mut w,&mut clock,|_|{});assert!(r.passed());assert_eq!(door,[0x44,0x55,0x66,1<<12,0x88]);assert_eq!(locks,[0x11,0,0x33]);let completed=m.clone();assert_eq!(once.run(&mut w,&mut clock,|_|{}).error,Some(Error::Outbound));drop(once);assert_eq!(m,completed);assert!(buffer.guards());
    for i in 0..m.len(){if !(TX.offset+40..TX.offset+64).contains(&i) && !(DESCRIPTOR.offset..DESCRIPTOR.offset+8).contains(&i) && !(DESCRIPTOR.offset+12..DESCRIPTOR.offset+16).contains(&i) && !(rx.offset+584..rx.offset+656).contains(&i){assert_eq!(m[i],before[i],"native receive changed {i:x}");}}
}
#[test]
fn stopped_clock_consumes_adapter_without_publishing_an_intent(){
    struct Stopped;impl Clock for Stopped{fn frequency(&self)->u64{1_000_000}fn now(&mut self)->u64{0}}
    let(mut m,cap,_)=receive_fixture();let before=m.clone();let mut lock=0;let mut door=[0;4];let mut buffer=pixel_glink_rx::Buffer::new();let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let mut once=unsafe{pixel_glink_rx::ReceiveOnce::new(cap,&mut buffer,m.as_mut_ptr().add(PART),door.as_mut_ptr())};assert_eq!(once.run(&mut w,&mut Stopped,|_|{}).error,Some(Error::Clock));assert_eq!(once.run(&mut w,&mut Stopped,|_|{}).error,Some(Error::Outbound));drop(once);assert_eq!(m,before);assert_eq!(door,[0;4]);assert!(buffer.empty());
}
fn rx_report()->pixel_glink_rx::Report{pixel_glink_rx::Report{error:None,stage:8,byte_writes:24,head_store:true,notifications:1,tx_consumed:false,words:[40,64,8,584,0,0,0,0],available:576,header:None,chunk:0,left:0,copied:0,guards:false,qrtr_words:[0;8],control_word:0,qrtr_v1:false,samples:1}}
fn stage_receive_model(m:&mut[u8],rx:usize){m[TX.offset+40..TX.offset+64].copy_from_slice(&pixel_glink_rx::PACKETS);w32(m,DESCRIPTOR.offset,64);w32(m,DESCRIPTOR.offset+4,64);w32(m,DESCRIPTOR.offset+12,656);m[rx+584..rx+656].copy_from_slice(&data_packet());}
#[test]
fn partial_published_header_waits_without_copy_then_accepts_complete_packet(){
    let(mut m,cap,rx)=receive_fixture();let(ready,base,old,c,_)=cap.into_parts();stage_receive_model(&mut m,rx.offset);w32(&mut m,DESCRIPTOR.offset+12,592);let mut buffer=pixel_glink_rx::Buffer::new();let mut report=rx_report();
    struct Bounded<'a>{m:&'a[u8],rx:usize,end:usize}impl Read for Bounded<'_>{fn read8(&self,at:usize)->u8{if(self.rx..self.rx+0x4000).contains(&at){assert!(at<self.rx+self.end)}self.m[at]}}
    assert_eq!(pixel_glink_rx::sample(&Bounded{m:&m,rx:rx.offset,end:592},&base,&ready,&old,&c,&mut buffer,&mut report),Ok(()));assert_eq!(report.copied,0);assert!(buffer.empty());w32(&mut m,DESCRIPTOR.offset+12,656);let before=m.clone();assert_eq!(pixel_glink_rx::sample(&Bounded{m:&m,rx:rx.offset,end:656},&base,&ready,&old,&c,&mut buffer,&mut report),Ok(()));assert!(report.passed());assert_eq!(m,before);
}
#[test]
fn changed_second_packet_copy_or_revoked_ready_never_reaches_local_buffer(){
    for revoke in [false,true]{let(mut m,cap,rx)=receive_fixture();let(ready,base,old,c,_)=cap.into_parts();stage_receive_model(&mut m,rx.offset);let peer=smem_item(&m,435).unwrap();let before=m.clone();let mut buffer=pixel_glink_rx::Buffer::new();let mut report=rx_report();
        struct Fault<'a>{m:&'a[u8],rx:usize,peer:usize,revoke:bool,seen:std::cell::Cell<u32>}impl Read for Fault<'_>{fn read8(&self,at:usize)->u8{if at==self.rx+655{let n=self.seen.get();self.seen.set(n+1);if !self.revoke && n>0{return self.m[at]^1}}if self.revoke && at==self.peer+36 && self.seen.get()>0{return 0}self.m[at]}}
        assert_eq!(pixel_glink_rx::sample(&Fault{m:&m,rx:rx.offset,peer:peer.offset,revoke,seen:std::cell::Cell::new(0)},&base,&ready,&old,&c,&mut buffer,&mut report),Err(if revoke{Error::ReadyLost}else{Error::Unstable}));assert_eq!(report.copied,0);assert!(buffer.empty());assert_eq!(m,before);
    }
}
#[test]
fn ready_revocation_during_post_copy_tx_verification_cannot_claim_verified_receive(){
    let(mut m,cap,rx)=receive_fixture();let(ready,base,old,c,_)=cap.into_parts();stage_receive_model(&mut m,rx.offset);let peer=smem_item(&m,435).unwrap();let before=m.clone();let mut buffer=pixel_glink_rx::Buffer::new();let mut report=rx_report();
    struct Revoked<'a>{m:&'a[u8],peer:usize,tx_passes:std::cell::Cell<u32>}impl Read for Revoked<'_>{fn read8(&self,at:usize)->u8{if at==TX.offset+TX.size-1{self.tx_passes.set(self.tx_passes.get()+1)}if at==self.peer+36 && self.tx_passes.get()>=3{return 0}self.m[at]}}
    let spy=Revoked{m:&m,peer:peer.offset,tx_passes:std::cell::Cell::new(0)};assert_eq!(pixel_glink_rx::sample(&spy,&base,&ready,&old,&c,&mut buffer,&mut report),Err(Error::ReadyLost));assert_eq!(report.copied,0);assert!(buffer.guards());assert_eq!(&buffer.bytes()[..52],&data_packet()[16..68]);assert_eq!(m,before);
}
#[test]
fn unsupported_control_queue_cannot_mint_a_receive_continuation(){
    let(mut m,next,rx)=channel_fixture();let mut lock=0;let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};
    struct BadQueue(ChannelIo);impl pixel_glink_channel::Io for BadQueue{fn byte(&mut self,i:usize,v:u8){self.0.byte(i,v)}fn head(&mut self){self.0.head()}fn notify(&mut self){self.0.notify();unsafe{let m=std::slice::from_raw_parts_mut(self.0.base,SMEM_BYTES);let mut b=observed_shape_controls();b[480]=9;m[self.0.rx+96..self.0.rx+584].copy_from_slice(&b);w32(m,DESCRIPTOR.offset+12,584);}}}
    let mut io=BadQueue(ChannelIo::new(&mut m,&lock,rx.offset));assert!(matches!(pixel_glink_channel::checkpoint_run(&mut w,next,&mut io,&mut Time{now:0,step:50000},|_|{}),Err(r)if r.error==Some(Error::Payload)));
}
#[test]
fn valid_captured_prefix_without_complete_queue_cannot_mint_receive_continuation(){
    let(mut m,next,rx)=channel_fixture();let mut lock=0;let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};
    struct Prefix(ChannelIo);impl pixel_glink_channel::Io for Prefix{fn byte(&mut self,i:usize,v:u8){self.0.byte(i,v)}fn head(&mut self){self.0.head()}fn notify(&mut self){self.0.notify();unsafe{let m=std::slice::from_raw_parts_mut(self.0.base,SMEM_BYTES);for b in m[self.0.rx+96..self.0.rx+608].chunks_exact_mut(8){b.copy_from_slice(&[13,0,0,0,0,0,0,0]);}w32(m,DESCRIPTOR.offset+12,624);}}}
    let mut io=Prefix(ChannelIo::new(&mut m,&lock,rx.offset));let checkpoint=match pixel_glink_channel::checkpoint_run(&mut w,next,&mut io,&mut Time{now:0,step:50000},|_|{}){Ok(checkpoint)=>checkpoint,Err(report)=>panic!("bounded channel capture: {report:?}")};
    let after_capture=m.clone();let mut buffer=pixel_glink_rx::Buffer::new();let mut rxio=RxIo::new(&mut m,&lock,rx.offset);let report=pixel_glink_rx::run(&mut w,checkpoint,&mut buffer,&mut rxio,&mut Time{now:0,step:50000},|_|{});
    assert_eq!(report.error,Some(Error::Payload));assert_eq!((rxio.bytes,rxio.heads,rxio.notifications),(0,0,0));assert_eq!(m,after_capture);assert!(buffer.empty());
}
#[test]
fn complete_first_frame_can_be_copied_with_a_later_unclassified_suffix_left_untouched(){
    let(mut m,cap,rx)=receive_fixture();let(ready,base,old,c,_)=cap.into_parts();stage_receive_model(&mut m,rx.offset);w32(&mut m,DESCRIPTOR.offset+12,664);m[rx.offset+656..rx.offset+664].copy_from_slice(&[13,0,0,0,0,0,0,0]);let before=m.clone();let mut buffer=pixel_glink_rx::Buffer::new();let mut report=rx_report();
    struct Bytes<'a>(&'a[u8]);impl Read for Bytes<'_>{fn read8(&self,at:usize)->u8{self.0[at]}}
    assert_eq!(pixel_glink_rx::sample(&Bytes(&m),&base,&ready,&old,&c,&mut buffer,&mut report),Ok(()));assert!(report.passed());assert_eq!(report.words[2..4],[8,664]);assert_eq!(report.available,656);assert_eq!(m,before);
}
