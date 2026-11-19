include!("sunfish_glink_done_test.rs");
#[path="../rust/src/pixel_qrtr_hello.rs"] mod pixel_qrtr_hello;
fn native_hello_packet()->[u8;72]{let mut p=data_packet();p[24..28].copy_from_slice(&0u32.to_le_bytes());p[40..44].copy_from_slice(&u32::MAX.to_le_bytes());p}
fn send_fixture<'a>(buffer:&'a mut pixel_glink_rx::Buffer)->(Vec<u8>,pixel_glink_done::SendCheckpoint<'a>,qcom_modem_status::Item){
    let(mut m,cap,rx)=receive_fixture();let mut lock=0;let mut door=[0;4];
    struct Peer{base:*mut u8,door:*const u32,rx:usize,now:u64}impl Clock for Peer{fn frequency(&self)->u64{1_000_000}fn now(&mut self)->u64{self.now+=50000;unsafe{if self.door.add(3).read_volatile()==1<<12{let m=std::slice::from_raw_parts_mut(self.base,SMEM_BYTES);let head=u32::from_le_bytes(m[DESCRIPTOR.offset+4..DESCRIPTOR.offset+8].try_into().unwrap());if head==64{w32(m,DESCRIPTOR.offset,64);w32(m,DESCRIPTOR.offset+12,656);m[self.rx+584..self.rx+656].copy_from_slice(&native_hello_packet());}else if head==72{w32(m,DESCRIPTOR.offset,72)}}}self.now}}
    let mut clock=Peer{base:m.as_mut_ptr(),door:door.as_ptr(),rx:rx.offset,now:0};let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let once=unsafe{pixel_glink_rx::ReceiveOnce::new(cap,buffer,m.as_mut_ptr().add(PART),door.as_mut_ptr())};let cap=match once.into_completion_checkpoint(&mut w,&mut clock,|_|{}){Ok(c)=>c,Err(e)=>panic!("send fixture receive: {:?}",e.report)};door[3]=0;let once=unsafe{pixel_glink_done::CompleteOnce::new(cap,m.as_mut_ptr().add(PART),door.as_mut_ptr())};let cap=match once.into_send_checkpoint(&mut w,&mut clock,|_|{}){Ok(c)=>c,Err(e)=>panic!("send fixture done: {:?}",e.report)};(m,cap,rx)
}
struct HelloIo{base:*mut u8,lock:*const u32,rx:usize,bytes:usize,heads:usize,notifications:usize,fault:u8}
impl HelloIo{fn new(m:&mut[u8],lock:*const u32,rx:usize)->Self{Self{base:m.as_mut_ptr(),lock,rx,bytes:0,heads:0,notifications:0,fault:0}}}
impl pixel_qrtr_hello::Io for HelloIo{
    fn byte(&mut self,i:usize,v:u8){assert_eq!(unsafe{*self.lock},1);assert_eq!(i,self.bytes);assert_eq!(v,pixel_qrtr_hello::PACKET[i]);assert_eq!(self.heads,0);self.bytes+=1;unsafe{if !(self.fault==1 && i==0){self.base.add(TX.offset+72+i).write_volatile(v)}if self.fault==15 && i==71{let m=std::slice::from_raw_parts_mut(self.base,SMEM_BYTES);let p=smem_item(m,435).unwrap();w32(m,p.offset+36,0)}}}
    fn head(&mut self){assert_eq!(unsafe{*self.lock},1);self.heads+=1;if self.fault!=2{unsafe{self.base.add(DESCRIPTOR.offset+4).cast::<u32>().write_volatile(144u32.to_le())}}}
    fn notify(&mut self){assert_eq!(unsafe{*self.lock},0);self.notifications+=1;unsafe{let m=std::slice::from_raw_parts_mut(self.base,SMEM_BYTES);if self.fault==3{return}w32(m,DESCRIPTOR.offset,144);let mut b=vec![6,0,2,0,0,0,0,0];match self.fault{4=>b[2]=1,5=>b[4]=1,6=>b[0]=9,7=>{let p=smem_item(m,435).unwrap();w32(m,p.offset+36,1)},8=>b=vec![5,0,2,0,2,0,0,0,128,0,0,0,30,0,0,0],9=>{b=vec![7,0,2,0,64,0,0,0,14,0,2,0,0,0,0,0]},10=>{b=vec![5,0,2,0,1,0,0,0,128,0,0,0,30,0,0,0,6,0,2,0,0,0,0,0]},11=>w32(m,DESCRIPTOR.offset+8,664),12=>w32(m,DESCRIPTOR.offset,136),13=>{b=vec![];for _ in 0..65{b.extend_from_slice(&[13,0,0,0,0,0,0,0])}},14=>m[TX.offset]^=1,16=>b[0]=14,_=>{}}m[self.rx+656..self.rx+656+b.len()].copy_from_slice(&b);w32(m,DESCRIPTOR.offset+12,(656+b.len())as u32)}}
}
#[test]
fn fixed_hello_wire_is_glink_data_with52_byte_qrtr_v1_control_and_zero_padding(){
    let p=pixel_qrtr_hello::PACKET;assert_eq!(&p[..16],&[9,0,1,0,0,0,0,0,52,0,0,0,0,0,0,0]);let words:Vec<u32>=p[16..68].chunks_exact(4).map(|x|u32::from_le_bytes(x.try_into().unwrap())).collect();assert_eq!(words,[1,2,1,0xfffffffe,0,20,0,0xfffffffe,2,0,0,0,0]);assert_eq!(&p[68..],&[0;4]);
}
#[test]
fn owned_current_peer_intent_gates_one_hello_and_exact_done_or_reuse_reply(){
    for fault in [0,9,10,16]{let mut buffer=pixel_glink_rx::Buffer::new();let(mut m,mut cap,rx)=send_fixture(&mut buffer);let before=m.clone();let mut lock=0;let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let mut io=HelloIo::new(&mut m,&lock,rx.offset);io.fault=fault;let r=pixel_qrtr_hello::run(&mut w,&mut cap,&mut io,&mut Time{now:0,step:50000},|_|{});assert!(r.passed(),"{r:?}");assert_eq!((io.bytes,io.heads,io.notifications),(72,1,1));assert_eq!(r.capacity,128);assert_eq!(r.words[..3],[144,144,656]);assert_eq!(r.done_command,if [9,16].contains(&fault){14}else{6});assert_eq!(r.requests,usize::from(fault==9)as u32);assert_eq!(r.advertised,usize::from(fault==10)as u32);assert_eq!(lock,0);let(completion,_)=cap.parts();assert!(completion.parts().5.verified(&native_hello_packet()[16..68]));
        for i in 0..m.len(){if !(TX.offset+72..TX.offset+144).contains(&i) && !(DESCRIPTOR.offset..DESCRIPTOR.offset+8).contains(&i) && !(DESCRIPTOR.offset+12..DESCRIPTOR.offset+16).contains(&i) && !(rx.offset+656..rx.offset+r.words[3]as usize).contains(&i){assert_eq!(m[i],before[i],"unexpected HELLO write {i:x}");}}
    }
}
#[test]
fn changed_ready_identity_cursor_tx_or_private_guard_prevents_hello_byte_stores(){
    for fault in 0..6{let mut buffer=pixel_glink_rx::Buffer::new();let(mut m,mut cap,rx)=send_fixture(&mut buffer);match fault{0=>w32(&mut m,DESCRIPTOR.offset+8,664),1=>m[TX.offset+144]=1,2=>{let p=smem_item(&m,435).unwrap();w32(&mut m,p.offset+36,0)},3=>m[PART+48+20]^=1,4=>w32(&mut m,DESCRIPTOR.offset+12,664),_=>{let(completion,_)=cap.parts();let b=completion.parts().5;unsafe{(b as *mut pixel_glink_rx::Buffer).cast::<u8>().write(0)}}};let before=m.clone();let mut lock=0;let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let mut io=HelloIo::new(&mut m,&lock,rx.offset);let r=pixel_qrtr_hello::run(&mut w,&mut cap,&mut io,&mut Time{now:0,step:50000},|_|{});assert!(r.error.is_some(),"fault{fault}: {r:?}");assert_eq!((io.bytes,io.heads,io.notifications),(0,0,0));assert_eq!(m,before);}
}
#[test]
fn hello_partial_publication_bad_completion_ready_loss_and_timeout_are_terminal(){
    for fault in [1,2,3,4,5,6,7,8,11,12,13,14,15]{let mut buffer=pixel_glink_rx::Buffer::new();let(mut m,mut cap,rx)=send_fixture(&mut buffer);let mut lock=0;let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let mut io=HelloIo::new(&mut m,&lock,rx.offset);io.fault=fault;let r=pixel_qrtr_hello::run(&mut w,&mut cap,&mut io,&mut Time{now:0,step:50000},|_|{});assert!(!r.passed(),"fault{fault}: {r:?}");assert_eq!(r.error,Some(match fault{1|14=>Error::Publication,2|11=>Error::Outbound,3|8|12=>Error::Timeout,4|5|6=>Error::Payload,7=>Error::Fatal,13=>Error::Bounds,_=>Error::ReadyLost}),"fault{fault}: {r:?}");assert_eq!(io.bytes,72);assert_eq!(io.heads,usize::from(![1,15].contains(&fault)));assert_eq!(io.notifications,usize::from(![1,2,15].contains(&fault)));assert_eq!(u32::from_le_bytes(m[DESCRIPTOR.offset+8..DESCRIPTOR.offset+12].try_into().unwrap()),if fault==11{664}else{656});assert_eq!(lock,0);}
}
#[test]
fn control_parser_is_bounded_exact_about_done_id_and_reports_stopping_header(){
    for bytes in [vec![6,0,2,0,0,0,0,0,6,0,2,0,0,0,0,0],vec![14,0,2,0,1,0,0,0],vec![5,0,2,0,64,0,0,0],vec![7,0,1,0,52,0,0,0],vec![9,0,2,0,1,0,0,0]]{let mut r=hello_report();assert_eq!(pixel_qrtr_hello::controls(&bytes,&mut r),Err(Error::Payload));assert!(r.header.is_some());assert!(!r.complete)}
    for n in [1,7,513,520]{let mut r=hello_report();assert_eq!(pixel_qrtr_hello::controls(&vec![0;n],&mut r),Err(Error::Bounds))}
    let mut r=hello_report();assert_eq!(pixel_qrtr_hello::controls(&[5,0,2,0,2,0,0,0,128,0,0,0,30,0,0,0],&mut r),Ok(()));assert!(!r.complete);assert_eq!(r.parsed,0);
}
fn hello_report()->pixel_qrtr_hello::Report{pixel_qrtr_hello::Report{error:None,stage:8,capacity:128,reserved:true,byte_writes:72,head_store:true,notifications:1,tx_consumed:false,words:[72,144,656,656,0,0,0,0],available:0,captured:0,parsed:0,records:0,done_seen:false,done_command:0,requests:0,request_size:0,advertised:0,complete:false,stop_offset:0,header:None,guards:false,samples:1}}
#[test]
fn retained_advertisement_must_prove_unique_intent_zero_with_sufficient_capacity(){
    for fault in 0..4{let mut buffer=pixel_glink_rx::Buffer::new();let(_,mut cap,_)=send_fixture(&mut buffer);let(completion,_)=cap.parts();let(_,_,_,c,_,_)=completion.parts();let mut bytes=c.bytes;match fault{0=>bytes[8..12].copy_from_slice(&51u32.to_le_bytes()),1=>bytes[12..16].copy_from_slice(&31u32.to_le_bytes()),2=>bytes[28..32].copy_from_slice(&0u32.to_le_bytes()),_=>bytes[0]=9};let bad=pixel_glink_channel::Capture{bytes,size:c.size,state:c.state};assert!(pixel_qrtr_hello::capacity(&bad).is_err());}
}
#[test]
fn native_hello_adapter_exact_writes_and_no_retry_after_success_or_timeout(){
    for failed in [false,true]{let mut buffer=pixel_glink_rx::Buffer::new();let(mut m,cap,rx)=send_fixture(&mut buffer);let before=m.clone();let mut locks=[0x11,0,0x33];let mut door=[0x44,0x55,0x66,0,0x88];
        struct Peer{base:*mut u8,door:*const u32,rx:usize,now:u64,failed:bool}impl Clock for Peer{fn frequency(&self)->u64{1_000_000}fn now(&mut self)->u64{self.now+=50000;unsafe{if !self.failed && self.door.add(3).read_volatile()==1<<12{let m=std::slice::from_raw_parts_mut(self.base,SMEM_BYTES);w32(m,DESCRIPTOR.offset,144);w32(m,DESCRIPTOR.offset+12,664);m[self.rx+656..self.rx+664].copy_from_slice(&[14,0,2,0,0,0,0,0]);}}self.now}}
        let mut clock=Peer{base:m.as_mut_ptr(),door:door.as_ptr(),rx:rx.offset,now:0,failed};let mut w=unsafe{Window::new(m.as_ptr(),locks.as_mut_ptr().add(1))};let mut once=unsafe{pixel_qrtr_hello::HelloOnce::new(cap,m.as_mut_ptr().add(PART),door.as_mut_ptr())};let r=once.run(&mut w,&mut clock,|_|{});assert_eq!(r.passed(),!failed,"{r:?}");let after=m.clone();assert_eq!(once.run(&mut w,&mut clock,|_|{}).error,Some(Error::Outbound));assert_eq!(m,after);assert_eq!(locks,[0x11,0,0x33]);assert_eq!(door,[0x44,0x55,0x66,1<<12,0x88]);assert_eq!(&m[rx.offset..rx.offset+656],&before[rx.offset..rx.offset+656]);assert_eq!(u32::from_le_bytes(m[DESCRIPTOR.offset+8..DESCRIPTOR.offset+12].try_into().unwrap()),656);
        for i in 0..m.len(){if !(TX.offset+72..TX.offset+144).contains(&i) && !(DESCRIPTOR.offset..DESCRIPTOR.offset+8).contains(&i) && !(DESCRIPTOR.offset+12..DESCRIPTOR.offset+16).contains(&i) && !(rx.offset+656..rx.offset+664).contains(&i){assert_eq!(m[i],before[i],"native HELLO aperture {i:x}");}}
    }
}
#[test]
fn failed_or_spent_completion_cannot_mint_send_authority(){
    for spent in [false,true]{let mut buffer=pixel_glink_rx::Buffer::new();let(mut m,cap,_)=completion_fixture(&mut buffer);let mut lock=0;let mut door=[0;4];let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let mut once=unsafe{pixel_glink_done::CompleteOnce::new(cap,m.as_mut_ptr().add(PART),door.as_mut_ptr())};if spent{assert_eq!(once.run(&mut w,&mut Time{now:0,step:0},|_|{}).error,Some(Error::Clock))}let before=m.clone();let e=match once.into_send_checkpoint(&mut w,&mut Time{now:0,step:0},|_|{}){Err(e)=>e,Ok(_)=>panic!("failed completion minted send")};assert_eq!(e.report.error,Some(if spent{Error::Outbound}else{Error::Clock}));assert_eq!(m,before);}
}
#[test]
fn stopped_clock_prevents_hello_publication(){
    let mut buffer=pixel_glink_rx::Buffer::new();let(mut m,mut cap,rx)=send_fixture(&mut buffer);let before=m.clone();let mut lock=0;let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let mut io=HelloIo::new(&mut m,&lock,rx.offset);let r=pixel_qrtr_hello::run(&mut w,&mut cap,&mut io,&mut Time{now:0,step:0},|_|{});assert_eq!(r.error,Some(Error::Clock));assert_eq!((io.bytes,io.heads,io.notifications),(0,0,0));assert_eq!(m,before);
}
#[test]
fn hello_snapshot_rejects_unstable_bytes_or_late_ready_loss_and_never_reads_released_rx(){
    for fault in 0..3{let mut buffer=pixel_glink_rx::Buffer::new();let(mut m,mut cap,rx)=send_fixture(&mut buffer);m[TX.offset+72..TX.offset+144].copy_from_slice(&pixel_qrtr_hello::PACKET);w32(&mut m,DESCRIPTOR.offset,144);w32(&mut m,DESCRIPTOR.offset+4,144);w32(&mut m,DESCRIPTOR.offset+12,664);m[rx.offset+656..rx.offset+664].copy_from_slice(&[14,0,2,0,0,0,0,0]);let peer=smem_item(&m,435).unwrap();let before=m.clone();let(completion,_)=cap.parts();let(ready,base,_,_,_,b)=completion.parts();let saved=b.snapshot();
        struct Spy<'a>{m:&'a[u8],rx:usize,peer:usize,fault:u8,seen:std::cell::Cell<u32>,tx_passes:std::cell::Cell<u32>}impl Read for Spy<'_>{fn read8(&self,at:usize)->u8{assert!(!(self.rx+8..self.rx+656).contains(&at),"read released RX");if at==TX.offset+TX.size-1{self.tx_passes.set(self.tx_passes.get()+1)}if at==self.rx+663{let n=self.seen.get();self.seen.set(n+1);if self.fault==1 && n==3{return self.m[at]^1}}if self.fault==2 && at==self.peer+36 && self.tx_passes.get()>=2{return 0}self.m[at]}}
        let spy=Spy{m:&m,rx:rx.offset,peer:peer.offset,fault,seen:std::cell::Cell::new(0),tx_passes:std::cell::Cell::new(0)};let mut r=hello_report();let result=pixel_qrtr_hello::sample(&spy,base,ready,b,&saved,&mut r);assert_eq!(result,match fault{0=>Ok(()),1=>Err(Error::Unstable),_=>Err(Error::ReadyLost)},"fault{fault}: {r:?}");assert_eq!(r.passed(),fault==0);assert_eq!(m,before);
    }
}
