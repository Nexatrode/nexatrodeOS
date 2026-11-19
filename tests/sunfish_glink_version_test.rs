include!("sunfish_glink_boot_test.rs");
#[path="../rust/src/pixel_glink_pending.rs"] mod pixel_glink_pending;
#[path="../rust/src/pixel_glink_version.rs"] mod pixel_glink_version;
use pixel_glink_version::{Io,PACKETS};
fn version_fixture()->(Vec<u8>,pixel_glink_boot::FreshBoot,pixel_smp2p_handshake::ReadyObservation,qcom_modem_status::Item){
    version_fixture_for_generation(1)
}
fn version_fixture_for_generation(generation:u64)->(Vec<u8>,pixel_glink_boot::FreshBoot,pixel_smp2p_handshake::ReadyObservation,qcom_modem_status::Item){
    let(mut m,mut h,b,boot,mut door)=boot_fixture_for_generation(generation);let mut lock=0;let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let mut io=AuthCalls::default();
    let started=match boot.authenticate(&mut w,&h,&mut io,&qcom_pas_mem_setup::tests::consumed(),|_|{}){Ok(s)=>s,Err(_)=>panic!("synthetic boot failed")};
    let(auth,fresh)=started.into_fresh_parts();let peer=u32::from_le_bytes(m[PART+12..PART+16].try_into().unwrap())as usize+PART+16;
    remote(&mut m,Some(6));w32(&mut m,peer+4,1);let rx=rx_allocation(&mut m);
    m[rx.offset..rx.offset+8].copy_from_slice(&[0,0,1,0,7,0,0,0]);w32(&mut m,DESCRIPTOR.offset+12,8);
    let r=h.observe(&mut w,b,auth,&mut Time{now:0,step:50000},|_|{});assert!(r.status.ready_bit_seen);assert_eq!(door[3],1<<14);door[3]=0;
    (m,fresh,h.take_ready_observation().unwrap(),rx)
}
struct VersionIo{base:*mut u8,lock:*const u32,rx:qcom_modem_status::Item,events:Vec<&'static str>,bytes:usize,head_calls:usize,tail_calls:usize,notifications:usize,fault:u8}
impl VersionIo{fn new(m:&mut[u8],lock:*const u32,rx:qcom_modem_status::Item)->Self{Self{base:m.as_mut_ptr(),lock,rx,events:vec![],bytes:0,head_calls:0,tail_calls:0,notifications:0,fault:0}}
    fn response(&mut self){unsafe{
        let m=std::slice::from_raw_parts_mut(self.base,SMEM_BYTES);w32(m,DESCRIPTOR.offset,16);w32(m,DESCRIPTOR.offset+12,16);
        m[self.rx.offset+8..self.rx.offset+16].copy_from_slice(&[1,0,1,0,0,0,0,0]);
    }}
}
impl Io for VersionIo{
    fn byte(&mut self,index:usize,value:u8){assert_eq!(unsafe{*self.lock},1);assert_eq!(index,self.bytes);assert_eq!(value,PACKETS[index]);assert_eq!(self.head_calls,0);self.bytes+=1;self.events.push("byte");
        if !(self.fault==1 && index==0){unsafe{self.base.add(TX.offset+index).write_volatile(value)}}
    }
    fn head(&mut self){assert_eq!(unsafe{*self.lock},1);assert_eq!(self.bytes,16);self.head_calls+=1;self.events.push("head");
        if self.fault!=2{unsafe{self.base.add(DESCRIPTOR.offset+4).cast::<u32>().write_unaligned(16u32.to_le())}}
        if self.fault==8{self.response();}
    }
    fn tail(&mut self){assert_eq!(unsafe{*self.lock},1);assert_eq!(self.head_calls,1);self.tail_calls+=1;self.events.push("tail");
        if self.fault!=3{unsafe{self.base.add(DESCRIPTOR.offset+8).cast::<u32>().write_unaligned(8u32.to_le())}}
    }
    fn notify(&mut self){assert_eq!(unsafe{*self.lock},0);assert_eq!(self.tail_calls,1);self.notifications+=1;self.events.push("notify");
        if self.fault!=4{self.response();}
        unsafe{let m=std::slice::from_raw_parts_mut(self.base,SMEM_BYTES);
            if self.fault==5{m[self.rx.offset+8]=2;}
            if self.fault==6{w32(m,DESCRIPTOR.offset,32);}
            if self.fault==7{let peer=smem_item(m,435).unwrap();w32(m,peer.offset+36,1);}
        }
    }
}
#[test]
fn fixed_version_exchange_consumes_only_offer_and_leaves_ack_pending(){
    let(mut m,fresh,ready,rx)=version_fixture();let original=m.clone();let mut locks=[0xa5,0,0x5a];let mut w=unsafe{Window::new(m.as_ptr(),locks.as_mut_ptr().add(1))};let mut io=VersionIo::new(&mut m,locks.as_ptr().wrapping_add(1),rx);
    let r=pixel_glink_version::exchange(&mut w,fresh,ready,&mut io,&mut Time{now:0,step:50000},|_|{});
    assert!(r.passed());assert_eq!((r.words[0],r.words[1],r.words[2],r.words[3]),(16,16,8,16));assert_eq!(r.header,Some(pixel_glink_version::ACK));assert_eq!(r.available,8);
    assert_eq!(io.events[16..],["head","tail","notify"]);assert_eq!(io.notifications,1);assert_eq!(locks,[0xa5,0,0x5a]);
    for i in 0..m.len(){if !(TX.offset..TX.offset+16).contains(&i) && !(DESCRIPTOR.offset..DESCRIPTOR.offset+16).contains(&i) && !(rx.offset+8..rx.offset+16).contains(&i){assert_eq!(m[i],original[i],"changed {i:x}");}}
    assert_eq!(&m[TX.offset..TX.offset+16],&PACKETS);assert!(m[TX.offset+16..TX.offset+TX.size].iter().all(|v|*v==0));assert_eq!(&m[rx.offset..rx.offset+8],&original[rx.offset..rx.offset+8]);
}
#[test]
fn wrong_offer_or_dirty_initial_memory_is_rejected_before_any_glink_write(){
    for fault in 0..8{
        let(mut m,fresh,ready,rx)=version_fixture();match fault{0=>w16(&mut m,rx.offset,1),1=>w16(&mut m,rx.offset+2,2),2=>w32(&mut m,rx.offset+4,1),3=>w32(&mut m,DESCRIPTOR.offset+12,0),
            4=>w32(&mut m,DESCRIPTOR.offset+12,16),5=>m[TX.offset+100]=1,6=>w32(&mut m,DESCRIPTOR.offset+4,8),_=>m[PART+48+20]^=1};
        let original=m.clone();let mut lock=0;let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let mut io=VersionIo::new(&mut m,&lock,rx);
        let r=pixel_glink_version::exchange(&mut w,fresh,ready,&mut io,&mut Time{now:0,step:50000},|_|{});assert!(r.error.is_some());assert!(io.events.is_empty());assert_eq!(m,original);assert_eq!(lock,0);
    }
}
#[test]
fn partial_store_failures_stop_without_notification_or_republication(){
    for fault in 1..=3{
        let(mut m,fresh,ready,rx)=version_fixture();let mut lock=0;let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let mut io=VersionIo::new(&mut m,&lock,rx);io.fault=fault;
        let r=pixel_glink_version::exchange(&mut w,fresh,ready,&mut io,&mut Time{now:0,step:50000},|_|{});
        assert!(r.error.is_some());assert!(!r.passed());assert_eq!(io.notifications,0);assert_eq!(io.head_calls,usize::from(fault>1));assert_eq!(io.tail_calls,usize::from(fault>2));assert_eq!(lock,0);
    }
}
#[test]
fn timeout_wrong_reply_bad_remote_tail_and_fatal_state_do_not_trigger_more_writes(){
    for fault in 4..=7{
        let(mut m,fresh,ready,rx)=version_fixture();let mut lock=0;let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let mut io=VersionIo::new(&mut m,&lock,rx);io.fault=fault;
        let r=pixel_glink_version::exchange(&mut w,fresh,ready,&mut io,&mut Time{now:0,step:50000},|_|{});
        assert!(!r.passed());assert_eq!(r.error,Some(match fault{4=>Error::Timeout,5=>Error::Payload,6=>Error::Outbound,_=>Error::Fatal}));
        assert_eq!((io.bytes,io.head_calls,io.tail_calls,io.notifications),(16,1,1,1));assert_eq!(lock,0);
    }
}
#[test]
fn early_remote_consumption_before_doorbell_is_valid_without_touching_remote_owned_words(){
    let(mut m,fresh,ready,rx)=version_fixture();let mut lock=0;let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let mut io=VersionIo::new(&mut m,&lock,rx);io.fault=8;
    let r=pixel_glink_version::exchange(&mut w,fresh,ready,&mut io,&mut Time{now:0,step:50000},|_|{});assert!(r.passed());assert_eq!(io.notifications,1);
}
#[test]
fn stopped_or_invalid_counter_cannot_publish_any_bytes(){
    struct Stopped{freq:u64}impl Clock for Stopped{fn now(&mut self)->u64{0}fn frequency(&self)->u64{self.freq}}
    for freq in [0,1_000_000]{let(mut m,fresh,ready,rx)=version_fixture();let original=m.clone();let mut lock=0;let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let mut io=VersionIo::new(&mut m,&lock,rx);
        let r=pixel_glink_version::exchange(&mut w,fresh,ready,&mut io,&mut Stopped{freq},|_|{});assert_eq!(r.error,Some(Error::Clock));assert!(io.events.is_empty());assert_eq!(m,original);
    }
}
#[test]
fn native_adapter_has_exact_first_page_stores_one_fixed_doorbell_and_no_second_run(){
    let(mut m,fresh,ready,rx)=version_fixture();let before=m.clone();let mut locks=[0x1234,0,0x5678];let mut door=[0x11,0x22,0x33,0,0x55];
    struct PeerTime{base:*mut u8,door:*const u32,rx:usize,now:u64}
    impl Clock for PeerTime{fn frequency(&self)->u64{1_000_000}fn now(&mut self)->u64{self.now+=50000;unsafe{if self.door.add(3).read_volatile()==1<<12{
        let m=std::slice::from_raw_parts_mut(self.base,SMEM_BYTES);w32(m,DESCRIPTOR.offset,16);w32(m,DESCRIPTOR.offset+12,16);m[self.rx+8..self.rx+16].copy_from_slice(&[1,0,1,0,0,0,0,0]);
    }}self.now}}
    let mut clock=PeerTime{base:m.as_mut_ptr(),door:door.as_ptr(),rx:rx.offset,now:0};let mut w=unsafe{Window::new(m.as_ptr(),locks.as_mut_ptr().add(1))};
    let mut once=unsafe{pixel_glink_version::VersionOnce::new(fresh,m.as_mut_ptr().add(PART),door.as_mut_ptr())};let r=once.run(&mut w,ready,&mut clock,|_|{});assert!(r.passed());assert_eq!(door,[0x11,0x22,0x33,1<<12,0x55]);assert_eq!(locks,[0x1234,0,0x5678]);
    for i in 0..m.len(){if !(TX.offset..TX.offset+16).contains(&i) && !(DESCRIPTOR.offset..DESCRIPTOR.offset+16).contains(&i) && !(rx.offset+8..rx.offset+16).contains(&i){assert_eq!(m[i],before[i],"native changed {i:x}");}}
    let original=m.clone();let(_,_,another,_)=version_fixture();let r=once.run(&mut w,another,&mut clock,|_|{});assert_eq!(r.error,Some(Error::Outbound));assert_eq!(m,original);
}
#[test]
fn ready_loss_during_postpublication_tx_verification_cannot_report_ack_success(){
    let(mut m,fresh,ready,rx)=version_fixture();let baseline=fresh.inspect(&RawRead(m.as_ptr()),&ready).unwrap().receive;
    m[TX.offset..TX.offset+16].copy_from_slice(&PACKETS);w32(&mut m,DESCRIPTOR.offset,16);w32(&mut m,DESCRIPTOR.offset+4,16);w32(&mut m,DESCRIPTOR.offset+8,8);w32(&mut m,DESCRIPTOR.offset+12,16);
    m[rx.offset+8..rx.offset+16].copy_from_slice(&[1,0,1,0,0,0,0,0]);let peer=smem_item(&m,435).unwrap();let original=m.clone();
    struct Revoked<'a>{m:&'a[u8],peer:usize,tx_read:std::cell::Cell<bool>}
    impl Read for Revoked<'_>{fn read8(&self,at:usize)->u8{
        if at==TX.offset+TX.size-1{self.tx_read.set(true)}
        if at==self.peer+36 && self.tx_read.get(){return 0}self.m[at]
    }}
    let spy=Revoked{m:&m,peer:peer.offset,tx_read:std::cell::Cell::new(false)};
    assert!(matches!(pixel_glink_version::sample(&spy,&baseline,&ready,0,8),Err(Error::ReadyLost)));assert!(spy.tx_read.get());assert_eq!(m,original);
}
