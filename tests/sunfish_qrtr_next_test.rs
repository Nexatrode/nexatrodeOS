include!("sunfish_qrtr_hello_test.rs");
#[path="../rust/src/pixel_qrtr_next.rs"] mod pixel_qrtr_next;
fn hello_controls()->[u8;16]{[7,0,2,0,52,0,0,0,14,0,2,0,0,0,0,0]}
fn next_fixture<'a>(first:&'a mut pixel_glink_rx::Buffer)->(Vec<u8>,pixel_qrtr_hello::HelloCheckpoint<'a>,qcom_modem_status::Item){
    let(mut m,cap,rx)=send_fixture(first);let mut lock=0;let mut door=[0;4];
    struct Peer{base:*mut u8,door:*const u32,rx:usize,now:u64}impl Clock for Peer{fn frequency(&self)->u64{1_000_000}fn now(&mut self)->u64{self.now+=50000;unsafe{if self.door.add(3).read_volatile()==1<<12{let m=std::slice::from_raw_parts_mut(self.base,SMEM_BYTES);w32(m,DESCRIPTOR.offset,144);w32(m,DESCRIPTOR.offset+12,672);m[self.rx+656..self.rx+672].copy_from_slice(&hello_controls());}}self.now}}
    let mut clock=Peer{base:m.as_mut_ptr(),door:door.as_ptr(),rx:rx.offset,now:0};let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let once=unsafe{pixel_qrtr_hello::HelloOnce::new(cap,m.as_mut_ptr().add(PART),door.as_mut_ptr())};let cap=match once.into_receive_checkpoint(&mut w,&mut clock,|_|{}){Ok(c)=>c,Err(e)=>panic!("next fixture HELLO: {:?}",e.report)};(m,cap,rx)
}
fn next_packet()->[u8;72]{let mut b=[0xee;72];b[..16].copy_from_slice(&[9,0,2,0,2,0,0,0,52,0,0,0,0,0,0,0]);for(i,v)in [1u32,4,0,0xfffffffe,0,20,1,0xfffffffe,4,0x1234,0x100,0,0x5678].into_iter().enumerate(){b[16+4*i..20+4*i].copy_from_slice(&v.to_le_bytes());}b}
struct NextIo{base:*mut u8,lock:*const u32,rx:usize,bytes:usize,heads:usize,notifications:usize,fault:u8}
impl NextIo{fn new(m:&mut[u8],lock:*const u32,rx:usize)->Self{Self{base:m.as_mut_ptr(),lock,rx,bytes:0,heads:0,notifications:0,fault:0}}}
impl pixel_qrtr_next::Io for NextIo{
    fn byte(&mut self,i:usize,v:u8){assert_eq!(unsafe{*self.lock},1);assert_eq!(i,self.bytes);assert_eq!(v,pixel_qrtr_next::PACKETS[i]);assert_eq!(self.heads,0);self.bytes+=1;unsafe{if !(self.fault==1 && i==0){self.base.add(TX.offset+144+i).write_volatile(v)}if self.fault==10 && i==23{let m=std::slice::from_raw_parts_mut(self.base,SMEM_BYTES);let p=smem_item(m,435).unwrap();w32(m,p.offset+36,0)}}}
    fn head(&mut self){assert_eq!(unsafe{*self.lock},1);self.heads+=1;if self.fault!=2{unsafe{self.base.add(DESCRIPTOR.offset+4).cast::<u32>().write_volatile(168u32.to_le())}}}
    fn notify(&mut self){assert_eq!(unsafe{*self.lock},0);self.notifications+=1;unsafe{let m=std::slice::from_raw_parts_mut(self.base,SMEM_BYTES);if self.fault==3{return}w32(m,DESCRIPTOR.offset,168);w32(m,DESCRIPTOR.offset+12,744);m[self.rx+672..self.rx+744].copy_from_slice(&next_packet());match self.fault{4=>m[self.rx+676]=1,5=>m[self.rx+674]=3,6=>w32(m,self.rx+680,65),7=>w32(m,self.rx+684,1),8=>{let p=smem_item(m,435).unwrap();w32(m,p.offset+36,1)},9=>m[self.rx+656]^=1,11=>m[TX.offset]^=1,_=>{}}}}
}
#[test]
fn fresh_intent2_copies_one_packet_to_disjoint_buffer_and_preserves_first_and_pending_controls(){
    let mut first=pixel_glink_rx::Buffer::new();let(mut m,mut cap,rx)=next_fixture(&mut first);let before=m.clone();let mut next=pixel_glink_rx::Buffer::new();let mut lock=0;let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let mut io=NextIo::new(&mut m,&lock,rx.offset);let r=pixel_qrtr_next::run(&mut w,&mut cap,&mut next,&mut io,&mut Time{now:0,step:50000},|_|{});assert!(r.passed(),"{r:?}");assert_eq!((io.bytes,io.heads,io.notifications),(24,1,1));assert_eq!(r.words,[168,168,656,744,0,0,0,0]);assert_eq!(r.copied,52);assert!(r.qrtr_v1 && r.guards && r.first_guards);assert_eq!(r.body_words,[4,0x1234,0x100,0,0x5678]);assert!(next.verified(&next_packet()[16..68]));let(send,_,_)=cap.parts();let(completion,_)=send.parts();assert!(completion.parts().5.verified(&native_hello_packet()[16..68]));assert_eq!(lock,0);
    for i in 0..m.len(){if !(TX.offset+144..TX.offset+168).contains(&i) && !(DESCRIPTOR.offset..DESCRIPTOR.offset+8).contains(&i) && !(DESCRIPTOR.offset+12..DESCRIPTOR.offset+16).contains(&i) && !(rx.offset+672..rx.offset+744).contains(&i){assert_eq!(m[i],before[i],"unexpected next receive write {i:x}");}}
}
#[test]
fn changed_pending_control_ready_identity_cursor_or_tx_prevents_any_fresh_promise(){
    for fault in 0..6{let mut first=pixel_glink_rx::Buffer::new();let(mut m,mut cap,rx)=next_fixture(&mut first);match fault{0=>m[rx.offset+660]^=1,1=>w32(&mut m,DESCRIPTOR.offset+8,664),2=>m[TX.offset+168]=1,3=>w32(&mut m,DESCRIPTOR.offset+12,680),4=>m[PART+48+20]^=1,_=>{let p=smem_item(&m,435).unwrap();w32(&mut m,p.offset+36,0)}}let before=m.clone();let mut next=pixel_glink_rx::Buffer::new();let mut lock=0;let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let mut io=NextIo::new(&mut m,&lock,rx.offset);let r=pixel_qrtr_next::run(&mut w,&mut cap,&mut next,&mut io,&mut Time{now:0,step:50000},|_|{});assert!(r.error.is_some());assert_eq!((io.bytes,io.heads,io.notifications),(0,0,0));assert!(next.empty());assert_eq!(m,before);}
}
#[test]
fn altered_first_payload_or_either_guard_or_nonempty_next_buffer_prevents_promise(){
    for fault in 0..4{let mut first=pixel_glink_rx::Buffer::new();let(mut m,mut cap,rx)=next_fixture(&mut first);let mut next=pixel_glink_rx::Buffer::new();if fault<2{let(send,_,_)=cap.parts();let(completion,_)=send.parts();let b=completion.parts().5;unsafe{(b as *mut pixel_glink_rx::Buffer).cast::<u8>().add(if fault==0{0}else{55}).write(0xcc)}}else{unsafe{(&mut next as *mut pixel_glink_rx::Buffer).cast::<u8>().add(if fault==2{95}else{16}).write(1)}}let before=m.clone();let mut lock=0;let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let mut io=NextIo::new(&mut m,&lock,rx.offset);let r=pixel_qrtr_next::run(&mut w,&mut cap,&mut next,&mut io,&mut Time{now:0,step:50000},|_|{});assert_eq!(r.error,Some(Error::Publication));assert_eq!((io.bytes,io.heads,io.notifications),(0,0,0));assert_eq!(m,before);}
}
#[test]
fn partial_next_publication_wrong_or_fragmented_packet_and_revocation_never_copy_or_retry(){
    for fault in 1..=11{let mut first=pixel_glink_rx::Buffer::new();let(mut m,mut cap,rx)=next_fixture(&mut first);let mut next=pixel_glink_rx::Buffer::new();let mut lock=0;let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let mut io=NextIo::new(&mut m,&lock,rx.offset);io.fault=fault;let r=pixel_qrtr_next::run(&mut w,&mut cap,&mut next,&mut io,&mut Time{now:0,step:50000},|_|{});assert!(!r.passed());assert_eq!(r.error,Some(match fault{1|11=>Error::Publication,2=>Error::Outbound,3=>Error::Timeout,4|5|6|7=>Error::Payload,8=>Error::Fatal,9=>Error::Unstable,_=>Error::ReadyLost}),"fault{fault}: {r:?}");assert_eq!(io.bytes,24);assert_eq!(io.heads,usize::from(![1,10].contains(&fault)));assert_eq!(io.notifications,usize::from(![1,2,10].contains(&fault)));assert!(next.empty());assert_eq!(u32::from_le_bytes(m[DESCRIPTOR.offset+8..DESCRIPTOR.offset+12].try_into().unwrap()),656);assert_eq!(lock,0);}
}
#[test]
fn next_packet_decoder_never_accepts_retired_intent1_or_wrong_sizes_or_fragmentation(){
    for fault in 0..5{let mut b=next_packet();match fault{0=>b[0]=12,1=>b[2]=1,2=>b[4]=1,3=>b[8..12].copy_from_slice(&u32::MAX.to_le_bytes()),_=>b[12]=1};assert_eq!(pixel_qrtr_next::packet(&b),Err(Error::Payload));}for len in 0..80{if len!=72{assert!(pixel_qrtr_next::packet(&vec![0;len]).is_err())}}assert_eq!(pixel_qrtr_next::packet(&next_packet()).unwrap().1,52);
}
fn stage_next(m:&mut[u8],rx:usize){m[TX.offset+144..TX.offset+168].copy_from_slice(&pixel_qrtr_next::PACKETS);w32(m,DESCRIPTOR.offset,168);w32(m,DESCRIPTOR.offset+4,168);w32(m,DESCRIPTOR.offset+12,744);m[rx+672..rx+744].copy_from_slice(&next_packet());}
fn next_report()->pixel_qrtr_next::Report{pixel_qrtr_next::Report{error:None,stage:8,byte_writes:24,head_store:true,notifications:1,tx_consumed:false,words:[144,168,656,672,0,0,0,0],available:16,header:None,chunk:0,left:0,copied:0,guards:false,first_guards:false,qrtr_words:[0;8],body_words:[0;5],qrtr_v1:false,samples:1}}
#[test]
fn partial_header_waits_readonly_then_copies_complete_frame_without_reading_released_rx(){
    let mut first=pixel_glink_rx::Buffer::new();let(mut m,mut cap,rx)=next_fixture(&mut first);stage_next(&mut m,rx.offset);w32(&mut m,DESCRIPTOR.offset+12,680);let(send,_,c)=cap.parts();let(completion,_)=send.parts();let(ready,base,_,_,_,first)=completion.parts();let saved=c.first_payload;let mut next=pixel_glink_rx::Buffer::new();let mut r=next_report();
    struct Spy<'a>{m:&'a[u8],rx:usize,end:usize}impl Read for Spy<'_>{fn read8(&self,at:usize)->u8{assert!(!(self.rx+8..self.rx+656).contains(&at),"read released bytes");if(self.rx..self.rx+0x4000).contains(&at){assert!(at<self.rx+self.end)}self.m[at]}}
    assert_eq!(pixel_qrtr_next::sample(&Spy{m:&m,rx:rx.offset,end:680},base,ready,c,first,&saved,&mut next,&mut r),Ok(()));assert!(next.empty());assert_eq!(r.copied,0);w32(&mut m,DESCRIPTOR.offset+12,744);let before=m.clone();assert_eq!(pixel_qrtr_next::sample(&Spy{m:&m,rx:rx.offset,end:744},base,ready,c,first,&saved,&mut next,&mut r),Ok(()));assert!(r.passed());assert_eq!(m,before);
}
#[test]
fn changed_second_data_copy_or_late_readiness_loss_cannot_report_next_receive_success(){
    for late in [false,true]{let mut first=pixel_glink_rx::Buffer::new();let(mut m,mut cap,rx)=next_fixture(&mut first);stage_next(&mut m,rx.offset);let peer=smem_item(&m,435).unwrap();let(send,_,c)=cap.parts();let(completion,_)=send.parts();let(ready,base,_,_,_,first)=completion.parts();let saved=c.first_payload;let mut next=pixel_glink_rx::Buffer::new();let mut r=next_report();
        struct Fault<'a>{m:&'a[u8],rx:usize,peer:usize,late:bool,seen:std::cell::Cell<u32>,passes:std::cell::Cell<u32>}impl Read for Fault<'_>{fn read8(&self,at:usize)->u8{if at==self.rx+743{let n=self.seen.get();self.seen.set(n+1);if !self.late && n>0{return self.m[at]^1}}if at==TX.offset+TX.size-1{self.passes.set(self.passes.get()+1)}if self.late && at==self.peer+36 && self.passes.get()>=3{return 0}self.m[at]}}
        let spy=Fault{m:&m,rx:rx.offset,peer:peer.offset,late,seen:std::cell::Cell::new(0),passes:std::cell::Cell::new(0)};assert_eq!(pixel_qrtr_next::sample(&spy,base,ready,c,first,&saved,&mut next,&mut r),Err(if late{Error::ReadyLost}else{Error::Unstable}));assert_eq!(r.copied,0);assert!(!r.passed());assert!(first.verified(&native_hello_packet()[16..68]));if late{assert!(next.verified(&next_packet()[16..68]))}else{assert!(next.empty())}
    }
}
#[test]
fn next_receive_preserves_later_unclassified_suffix_and_pending_completion_request(){
    let mut first=pixel_glink_rx::Buffer::new();let(mut m,mut cap,rx)=next_fixture(&mut first);stage_next(&mut m,rx.offset);m[rx.offset+744..rx.offset+752].copy_from_slice(&[13,0,0,0,0,0,0,0]);w32(&mut m,DESCRIPTOR.offset+12,752);let before=m.clone();let(send,_,c)=cap.parts();let(completion,_)=send.parts();let(ready,base,_,_,_,first)=completion.parts();let saved=c.first_payload;let mut next=pixel_glink_rx::Buffer::new();let mut r=next_report();struct Bytes<'a>(&'a[u8]);impl Read for Bytes<'_>{fn read8(&self,at:usize)->u8{self.0[at]}}assert_eq!(pixel_qrtr_next::sample(&Bytes(&m),base,ready,c,first,&saved,&mut next,&mut r),Ok(()));assert!(r.passed());assert_eq!(r.words[2..4],[656,752]);assert_eq!(m,before);
}
#[test]
fn native_next_adapter_has_exact_aperture_and_terminal_success_or_timeout(){
    for failed in [false,true]{let mut first=pixel_glink_rx::Buffer::new();let(mut m,cap,rx)=next_fixture(&mut first);let before=m.clone();let mut next=pixel_glink_rx::Buffer::new();let mut locks=[0x11,0,0x33];let mut door=[0x44,0x55,0x66,0,0x88];
        struct Peer{base:*mut u8,door:*const u32,rx:usize,now:u64,failed:bool}impl Clock for Peer{fn frequency(&self)->u64{1_000_000}fn now(&mut self)->u64{self.now+=50000;unsafe{if !self.failed && self.door.add(3).read_volatile()==1<<12{let m=std::slice::from_raw_parts_mut(self.base,SMEM_BYTES);w32(m,DESCRIPTOR.offset,168);w32(m,DESCRIPTOR.offset+12,744);m[self.rx+672..self.rx+744].copy_from_slice(&next_packet());}}self.now}}
        let mut clock=Peer{base:m.as_mut_ptr(),door:door.as_ptr(),rx:rx.offset,now:0,failed};let mut w=unsafe{Window::new(m.as_ptr(),locks.as_mut_ptr().add(1))};let mut once=unsafe{pixel_qrtr_next::NextOnce::new(cap,&mut next,m.as_mut_ptr().add(PART),door.as_mut_ptr())};let r=once.run(&mut w,&mut clock,|_|{});assert_eq!(r.passed(),!failed,"{r:?}");let after=m.clone();assert_eq!(once.run(&mut w,&mut clock,|_|{}).error,Some(Error::Outbound));assert_eq!(m,after);assert_eq!(locks,[0x11,0,0x33]);assert_eq!(door,[0x44,0x55,0x66,1<<12,0x88]);
        for i in 0..m.len(){if !(TX.offset+144..TX.offset+168).contains(&i) && !(DESCRIPTOR.offset..DESCRIPTOR.offset+8).contains(&i) && !(DESCRIPTOR.offset+12..DESCRIPTOR.offset+16).contains(&i) && !(rx.offset+672..rx.offset+744).contains(&i){assert_eq!(m[i],before[i],"native next aperture {i:x}");}}
    }
}
#[test]
fn failed_or_spent_hello_cannot_mint_fresh_receive_authority(){
    for spent in [false,true]{let mut first=pixel_glink_rx::Buffer::new();let(mut m,cap,_)=send_fixture(&mut first);let mut lock=0;let mut door=[0;4];let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let mut once=unsafe{pixel_qrtr_hello::HelloOnce::new(cap,m.as_mut_ptr().add(PART),door.as_mut_ptr())};if spent{assert_eq!(once.run(&mut w,&mut Time{now:0,step:0},|_|{}).error,Some(Error::Clock))}let before=m.clone();let e=match once.into_receive_checkpoint(&mut w,&mut Time{now:0,step:0},|_|{}){Err(e)=>e,Ok(_)=>panic!("failed HELLO minted receive")};assert_eq!(e.report.error,Some(if spent{Error::Outbound}else{Error::Clock}));assert_eq!(m,before);}
}
#[test]
fn either_control_order_is_valid_but_duplicates_or_changed_request_do_not_grant_receive_plan(){
    let mut first=pixel_glink_rx::Buffer::new();let(_,mut cap,_)=next_fixture(&mut first);let(_,r,c)=cap.parts();assert_eq!(pixel_qrtr_next::plan(r,c),Ok(()));let mut swapped=c.bytes;swapped[..8].copy_from_slice(&c.bytes[8..16]);swapped[8..16].copy_from_slice(&c.bytes[..8]);let mut b=pixel_qrtr_hello::ControlCapture{bytes:swapped,size:16,first_payload:c.first_payload};assert_eq!(pixel_qrtr_next::plan(r,&b),Ok(()));let head:[u8;8]=b.bytes[..8].try_into().unwrap();b.bytes[8..16].copy_from_slice(&head);assert_eq!(pixel_qrtr_next::plan(r,&b),Err(Error::Payload));b.bytes=c.bytes;b.bytes[4]=64;assert_eq!(pixel_qrtr_next::plan(r,&b),Err(Error::Payload));
}
#[test]
fn stopped_clock_prevents_any_second_receive_promise(){
    let mut first=pixel_glink_rx::Buffer::new();let(mut m,mut cap,rx)=next_fixture(&mut first);let before=m.clone();let mut next=pixel_glink_rx::Buffer::new();let mut lock=0;let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let mut io=NextIo::new(&mut m,&lock,rx.offset);let r=pixel_qrtr_next::run(&mut w,&mut cap,&mut next,&mut io,&mut Time{now:0,step:0},|_|{});assert_eq!(r.error,Some(Error::Clock));assert_eq!((io.bytes,io.heads,io.notifications),(0,0,0));assert_eq!(m,before);
}
