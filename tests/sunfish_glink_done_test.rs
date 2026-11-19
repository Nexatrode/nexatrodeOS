include!("sunfish_glink_rx_test.rs");
#[path="../rust/src/pixel_glink_done.rs"] mod pixel_glink_done;
fn completion_fixture<'a>(buffer:&'a mut pixel_glink_rx::Buffer)->(Vec<u8>,pixel_glink_rx::CompletionCheckpoint<'a>,qcom_modem_status::Item){
    let(mut m,cap,rx)=receive_fixture();let mut lock=0;let mut door=[0;4];
    struct Peer{base:*mut u8,door:*const u32,rx:usize,now:u64}impl Clock for Peer{fn frequency(&self)->u64{1_000_000}fn now(&mut self)->u64{self.now+=50000;unsafe{if self.door.add(3).read_volatile()==1<<12{let m=std::slice::from_raw_parts_mut(self.base,SMEM_BYTES);w32(m,DESCRIPTOR.offset,64);w32(m,DESCRIPTOR.offset+12,656);m[self.rx+584..self.rx+656].copy_from_slice(&data_packet());}}self.now}}
    let mut clock=Peer{base:m.as_mut_ptr(),door:door.as_ptr(),rx:rx.offset,now:0};let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let once=unsafe{pixel_glink_rx::ReceiveOnce::new(cap,buffer,m.as_mut_ptr().add(PART),door.as_mut_ptr())};
    let cap=match once.into_completion_checkpoint(&mut w,&mut clock,|_|{}){Ok(c)=>c,Err(e)=>panic!("completion fixture receive failed: {:?}",e.report)};(m,cap,rx)
}
struct DoneIo{base:*mut u8,lock:*const u32,rx:usize,bytes:usize,tails:usize,heads:usize,notifications:usize,fault:u8}
impl DoneIo{fn new(m:&mut[u8],lock:*const u32,rx:usize)->Self{Self{base:m.as_mut_ptr(),lock,rx,bytes:0,tails:0,heads:0,notifications:0,fault:0}}fn revoke(&mut self){unsafe{let m=std::slice::from_raw_parts_mut(self.base,SMEM_BYTES);let p=smem_item(m,435).unwrap();w32(m,p.offset+36,0)}}}
impl pixel_glink_done::Io for DoneIo{
    fn byte(&mut self,i:usize,v:u8){assert_eq!(unsafe{*self.lock},1);assert_eq!(i,self.bytes);assert_eq!(v,pixel_glink_done::DONE[i]);assert_eq!((self.tails,self.heads),(0,0));self.bytes+=1;if !(self.fault==1 && i==0){unsafe{self.base.add(TX.offset+64+i).write_volatile(v)}}if self.fault==4 && i==7{self.revoke()}}
    fn tail(&mut self){assert_eq!(unsafe{*self.lock},1);assert_eq!((self.bytes,self.tails,self.heads),(8,0,0));self.tails+=1;if self.fault!=2{unsafe{self.base.add(DESCRIPTOR.offset+8).cast::<u32>().write_volatile(656u32.to_le())}}if self.fault==5{self.revoke()}if self.fault==11{unsafe{std::slice::from_raw_parts_mut(self.base,SMEM_BYTES)[self.rx+8..self.rx+656].fill(0xdd)}}}
    fn head(&mut self){assert_eq!(unsafe{*self.lock},1);assert_eq!((self.tails,self.heads),(1,0));self.heads+=1;if self.fault!=3{unsafe{self.base.add(DESCRIPTOR.offset+4).cast::<u32>().write_volatile(72u32.to_le())}}if self.fault==6{self.revoke()}}
    fn notify(&mut self){assert_eq!(unsafe{*self.lock},0);assert_eq!((self.bytes,self.tails,self.heads,self.notifications),(8,1,1,0));self.notifications+=1;unsafe{let m=std::slice::from_raw_parts_mut(self.base,SMEM_BYTES);if self.fault!=7{w32(m,DESCRIPTOR.offset,72)}if self.fault==8{let p=smem_item(m,435).unwrap();w32(m,p.offset+36,1)}if self.fault==9{m[self.rx+8..self.rx+656].fill(0xcc);m[self.rx+656..self.rx+664].copy_from_slice(&[13,0,0,0,0,0,0,0]);w32(m,DESCRIPTOR.offset+12,664)}if self.fault==10{m[TX.offset]^=1}}}
}
#[test]
fn one_nonreusable_done_consumes_only_the_verified_queue_and_preserves_private_payload(){
    let mut buffer=pixel_glink_rx::Buffer::new();let(mut m,mut cap,rx)=completion_fixture(&mut buffer);let before=m.clone();let mut lock=0;let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let mut io=DoneIo::new(&mut m,&lock,rx.offset);let r=pixel_glink_done::run(&mut w,&mut cap,&mut io,&mut Time{now:0,step:50000},|_|{});
    assert!(r.passed(),"{r:?}");assert_eq!((io.bytes,io.tails,io.heads,io.notifications),(8,1,1,1));assert_eq!(r.words,[72,72,656,656,0,0,0,0]);assert_eq!(r.available,0);assert!(cap.parts().5.verified(&data_packet()[16..68]));assert_eq!(lock,0);
    for i in 0..m.len(){if !(TX.offset+64..TX.offset+72).contains(&i) && !(DESCRIPTOR.offset..DESCRIPTOR.offset+12).contains(&i){assert_eq!(m[i],before[i],"unexpected completion write {i:x}");}}
}
#[test]
fn changed_queue_payload_guard_or_identity_prevents_any_completion_stores(){
    for fault in 0..8{let mut buffer=pixel_glink_rx::Buffer::new();let(mut m,mut cap,rx)=completion_fixture(&mut buffer);match fault{0=>m[rx.offset+100]^=1,1=>m[rx.offset+600]^=1,2=>w32(&mut m,DESCRIPTOR.offset+12,664),3=>m[TX.offset+72]=1,4=>{let p=smem_item(&m,435).unwrap();w32(&mut m,p.offset+36,0)},5=>m[PART+48+20]^=1,6=>w32(&mut m,DESCRIPTOR.offset+8,16),_=>{let b=cap.parts().5;unsafe{(b as *mut pixel_glink_rx::Buffer).cast::<u8>().write(0)}}};let before=m.clone();let mut lock=0;let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let mut io=DoneIo::new(&mut m,&lock,rx.offset);let r=pixel_glink_done::run(&mut w,&mut cap,&mut io,&mut Time{now:0,step:50000},|_|{});assert!(r.error.is_some(),"fault{fault}: {r:?}");assert_eq!((io.bytes,io.tails,io.heads,io.notifications),(0,0,0,0));assert_eq!(m,before);}
}
#[test]
fn partial_done_publication_and_ready_loss_stop_without_retries_or_cleanup(){
    for fault in 1..=8{let mut buffer=pixel_glink_rx::Buffer::new();let(mut m,mut cap,rx)=completion_fixture(&mut buffer);let mut lock=0;let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let mut io=DoneIo::new(&mut m,&lock,rx.offset);io.fault=fault;let r=pixel_glink_done::run(&mut w,&mut cap,&mut io,&mut Time{now:0,step:50000},|_|{});assert!(!r.passed());assert_eq!(r.error,Some(match fault{1=>Error::Publication,2|3=>Error::Outbound,4|5|6=>Error::ReadyLost,7=>Error::Timeout,_=>Error::Fatal}),"fault{fault}: {r:?}");assert_eq!(io.bytes,8);assert_eq!(io.tails,usize::from(![1,4].contains(&fault)));assert_eq!(io.heads,usize::from([3,6,7,8].contains(&fault)));assert_eq!(io.notifications,usize::from(fault>=7));assert!(cap.parts().5.verified(&data_packet()[16..68]));assert_eq!(lock,0);}
}
#[test]
fn completion_never_rereads_consumed_peer_bytes_and_retains_later_unclassified_suffix(){
    let mut buffer=pixel_glink_rx::Buffer::new();let(mut m,mut cap,rx)=completion_fixture(&mut buffer);let mut lock=0;let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let mut io=DoneIo::new(&mut m,&lock,rx.offset);io.fault=9;let r=pixel_glink_done::run(&mut w,&mut cap,&mut io,&mut Time{now:0,step:50000},|_|{});assert!(r.passed(),"{r:?}");assert_eq!(r.words[2..4],[656,664]);assert_eq!(r.available,8);assert!(m[rx.offset+8..rx.offset+656].iter().all(|v|*v==0xcc));assert_eq!(&m[rx.offset+656..rx.offset+664],&[13,0,0,0,0,0,0,0]);assert!(cap.parts().5.verified(&data_packet()[16..68]));
}
#[test]
fn changed_full_tx_after_notification_cannot_claim_done_consumption(){
    let mut buffer=pixel_glink_rx::Buffer::new();let(mut m,mut cap,rx)=completion_fixture(&mut buffer);let mut lock=0;let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let mut io=DoneIo::new(&mut m,&lock,rx.offset);io.fault=10;let r=pixel_glink_done::run(&mut w,&mut cap,&mut io,&mut Time{now:0,step:50000},|_|{});assert_eq!(r.error,Some(Error::Publication));assert_eq!((io.bytes,io.tails,io.heads,io.notifications),(8,1,1,1));assert!(!r.retired);
}
#[test]
fn native_completion_adapter_has_exact_aperture_and_terminal_success_or_failure(){
    for failed in [false,true]{let mut buffer=pixel_glink_rx::Buffer::new();let(mut m,cap,rx)=completion_fixture(&mut buffer);let before=m.clone();let mut locks=[0x11,0,0x33];let mut door=[0x44,0x55,0x66,0,0x88];
        struct Peer{base:*mut u8,door:*const u32,now:u64,failed:bool}impl Clock for Peer{fn frequency(&self)->u64{1_000_000}fn now(&mut self)->u64{self.now+=50000;if !self.failed && unsafe{self.door.add(3).read_volatile()}==1<<12{unsafe{self.base.add(DESCRIPTOR.offset).cast::<u32>().write_volatile(72u32.to_le())}}self.now}}
        let mut clock=Peer{base:m.as_mut_ptr(),door:door.as_ptr(),now:0,failed};let mut w=unsafe{Window::new(m.as_ptr(),locks.as_mut_ptr().add(1))};let mut once=unsafe{pixel_glink_done::CompleteOnce::new(cap,m.as_mut_ptr().add(PART),door.as_mut_ptr())};let r=once.run(&mut w,&mut clock,|_|{});assert_eq!(r.passed(),!failed,"{r:?}");let after=m.clone();assert_eq!(once.run(&mut w,&mut clock,|_|{}).error,Some(Error::Outbound));assert_eq!(m,after);assert_eq!(locks,[0x11,0,0x33]);assert_eq!(door,[0x44,0x55,0x66,1<<12,0x88]);
        for i in 0..m.len(){if !(TX.offset+64..TX.offset+72).contains(&i) && !(DESCRIPTOR.offset..DESCRIPTOR.offset+12).contains(&i){assert_eq!(m[i],before[i],"native completion aperture {i:x}");}}assert_eq!(&m[rx.offset..rx.offset+0x4000],&before[rx.offset..rx.offset+0x4000]);
    }
}
#[test]
fn stopped_clock_cannot_publish_a_completion(){
    let mut buffer=pixel_glink_rx::Buffer::new();let(mut m,mut cap,rx)=completion_fixture(&mut buffer);let before=m.clone();let mut lock=0;let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let mut io=DoneIo::new(&mut m,&lock,rx.offset);let r=pixel_glink_done::run(&mut w,&mut cap,&mut io,&mut Time{now:0,step:0},|_|{});assert_eq!(r.error,Some(Error::Clock));assert_eq!((io.bytes,io.tails,io.heads,io.notifications),(0,0,0,0));assert_eq!(m,before);
}
#[test]
fn already_spent_or_failed_receive_cannot_mint_a_completion_checkpoint(){
    for spent in [false,true]{let(mut m,cap,_)=receive_fixture();let mut lock=0;let mut door=[0;4];let mut buffer=pixel_glink_rx::Buffer::new();let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let mut once=unsafe{pixel_glink_rx::ReceiveOnce::new(cap,&mut buffer,m.as_mut_ptr().add(PART),door.as_mut_ptr())};if spent{assert_eq!(once.run(&mut w,&mut Time{now:0,step:0},|_|{}).error,Some(Error::Clock))}let before=m.clone();let stopped=match once.into_completion_checkpoint(&mut w,&mut Time{now:0,step:0},|_|{}){Err(e)=>e,Ok(_)=>panic!("failed receive minted completion")};assert_eq!(stopped.report.error,Some(if spent{Error::Outbound}else{Error::Clock}));assert_eq!(m,before);}
}
#[test]
fn consumed_bytes_may_change_immediately_at_tail_publication(){
    let mut buffer=pixel_glink_rx::Buffer::new();let(mut m,mut cap,rx)=completion_fixture(&mut buffer);let mut lock=0;let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let mut io=DoneIo::new(&mut m,&lock,rx.offset);io.fault=11;let r=pixel_glink_done::run(&mut w,&mut cap,&mut io,&mut Time{now:0,step:50000},|_|{});assert!(r.passed(),"{r:?}");assert!(m[rx.offset+8..rx.offset+656].iter().all(|v|*v==0xdd));assert!(cap.parts().5.verified(&data_packet()[16..68]));
}
#[test]
fn completion_sample_forbids_released_rx_reads_and_detects_late_ready_loss(){
    for revoke in [false,true]{let mut buffer=pixel_glink_rx::Buffer::new();let(mut m,mut cap,rx)=completion_fixture(&mut buffer);m[TX.offset+64..TX.offset+72].copy_from_slice(&pixel_glink_done::DONE);w32(&mut m,DESCRIPTOR.offset,72);w32(&mut m,DESCRIPTOR.offset+4,72);w32(&mut m,DESCRIPTOR.offset+8,656);let peer=smem_item(&m,435).unwrap();let before=m.clone();let(ready,base,_,_,_,b)=cap.parts();
        struct Spy<'a>{m:&'a[u8],rx:usize,peer:usize,revoke:bool,passes:std::cell::Cell<u32>}impl Read for Spy<'_>{fn read8(&self,at:usize)->u8{assert!(!(self.rx+8..self.rx+656).contains(&at),"read released RX bytes");if at==TX.offset+TX.size-1{self.passes.set(self.passes.get()+1)}if self.revoke && at==self.peer+36 && self.passes.get()>=2{return 0}self.m[at]}}
        let spy=Spy{m:&m,rx:rx.offset,peer:peer.offset,revoke,passes:std::cell::Cell::new(0)};let mut r=pixel_glink_done::Report{error:None,stage:8,byte_writes:8,tail_store:true,head_store:true,notifications:1,tx_consumed:false,words:[64,72,656,656,0,0,0,0],available:0,guards:false,retired:false,samples:1};let saved=data_packet()[16..68].try_into().unwrap();let result=pixel_glink_done::sample(&spy,base,ready,&saved,b,&mut r);assert_eq!(result,if revoke{Err(Error::ReadyLost)}else{Ok(())});assert_eq!(r.passed(),!revoke);assert_eq!(m,before);
    }
}
#[test]
fn changed_second_completion_wire_copy_is_rejected_before_cursor_publication(){
    let mut buffer=pixel_glink_rx::Buffer::new();let(m,mut cap,rx)=completion_fixture(&mut buffer);let(ready,base,old,c,prior,b)=cap.parts();
    struct Flip<'a>{m:&'a[u8],rx:usize,seen:std::cell::Cell<u32>}impl Read for Flip<'_>{fn read8(&self,at:usize)->u8{if at==self.rx+655{let n=self.seen.get();self.seen.set(n+1);if n>0{return self.m[at]^1}}self.m[at]}}
    let spy=Flip{m:&m,rx:rx.offset,seen:std::cell::Cell::new(0)};let s=pixel_glink_receive::inspect_ref(&spy,ready).unwrap();assert_eq!(s.inventory.items,base.inventory.items);assert_eq!(pixel_glink_done::queue(&spy,&s,old,c,b,prior),Err(Error::Unstable));
}
