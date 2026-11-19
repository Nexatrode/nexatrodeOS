include!("sunfish_glink_version_test.rs");
use pixel_glink_pending::{MAX_BYTES,parse};

fn control_queue()->Vec<u8>{
    let mut b=vec![2,0,1,0,10,0,0,0];b.extend_from_slice(b"glink_ssr\0");b.resize(24,0xa5);
    b.extend_from_slice(&[1,0,1,0,0,0,0,0]);
    b.extend_from_slice(&[2,0,2,0,7,0,0,0]);b.extend_from_slice(b"IPCRTR\0");b.resize(48,0x5a);
    b.extend_from_slice(&[13,0,0,0,0,0,0,0]);b
}
fn pending_fixture(bytes:&[u8])->(Vec<u8>,pixel_glink_receive::Report,pixel_smp2p_handshake::ReadyObservation,qcom_modem_status::Item){
    let(mut m,fresh,ready,rx)=version_fixture();let baseline=fresh.inspect(&RawRead(m.as_ptr()),&ready).unwrap().receive;
    m[TX.offset..TX.offset+16].copy_from_slice(&PACKETS);w32(&mut m,DESCRIPTOR.offset,16);w32(&mut m,DESCRIPTOR.offset+4,16);w32(&mut m,DESCRIPTOR.offset+8,8);
    m[rx.offset+8..rx.offset+8+bytes.len()].copy_from_slice(bytes);w32(&mut m,DESCRIPTOR.offset+12,(8+bytes.len())as u32);
    (m,baseline,ready,rx)
}
#[test]
fn parses_aligned_open_names_and_ack_without_interpreting_padding_as_messages(){
    let b=control_queue();let p=parse(&b);assert_eq!(p.error,None);assert!(!p.unknown_command);assert!(p.ack_in_prefix);assert_eq!(p.count,4);assert_eq!(p.parsed_bytes,b.len());
    assert_eq!(&p.packets[0].name[..p.packets[0].name_len],b"glink_ssr");assert_eq!(p.packets[1].offset,24);assert_eq!(&p.packets[2].name[..p.packets[2].name_len],b"IPCRTR");assert_eq!(p.packets[3].offset,48);
}
#[test]
fn malformed_names_and_truncated_or_unknown_packets_are_not_followed(){
    for n in [0,1,33,u32::MAX]{let mut b=control_queue();b[4..8].copy_from_slice(&n.to_le_bytes());let p=parse(&b);assert_eq!(p.error,Some(Error::Payload));assert_eq!(p.count,0);assert!(!p.ack_in_prefix);}
    for index in [8,17]{let mut b=control_queue();b[index]=if index==8{0}else{b'X'};let p=parse(&b);assert_eq!(p.error,Some(Error::Payload));assert_eq!(p.count,0);}
    let mut b=control_queue();b[8]=0xff;assert_eq!(parse(&b).error,Some(Error::Payload));
    assert_eq!(parse(&control_queue()[..16]).error,Some(Error::Bounds));assert_eq!(parse(&[0;7]).error,Some(Error::Bounds));assert_eq!(parse(&[0;MAX_BYTES+8]).error,Some(Error::Bounds));
    let mut b=control_queue();b[0]=9;let p=parse(&b);assert!(p.unknown_command);assert_eq!(p.error,None);assert_eq!(p.count,0);assert_eq!(p.parsed_bytes,0);assert!(!p.ack_in_prefix);
}
#[test]
fn copied_pending_queue_is_readonly_and_keeps_live_status_and_indices(){
    let b=control_queue();let(m,baseline,ready,_)=pending_fixture(&b);let before=m.clone();let r=pixel_glink_pending::read(&RawRead(m.as_ptr()),&baseline,&ready,16,8).unwrap();
    assert_eq!(&r.bytes[..r.captured],&b);assert_eq!(r.captured,b.len());assert_eq!(r.available,b.len()as u32);assert_eq!(r.words[..4],[16,16,8,64]);assert!(r.parsed.ack_in_prefix);assert_eq!(m,before);
}
#[test]
fn pending_reader_never_reads_beyond_128_byte_prefix_even_with_more_queued(){
    let b=vec![0;160];let(m,baseline,ready,rx)=pending_fixture(&b);let before=m.clone();
    struct Bounded<'a>{m:&'a[u8],start:usize}
    impl Read for Bounded<'_>{fn read8(&self,at:usize)->u8{if(self.start..self.start+pixel_glink_receive::FIFO_BYTES).contains(&at){assert!(at<self.start+8+MAX_BYTES,"read outside bounded prefix: {at:x}");}self.m[at]}}
    let r=pixel_glink_pending::read(&Bounded{m:&m,start:rx.offset},&baseline,&ready,16,8).unwrap();assert_eq!(r.captured,MAX_BYTES);assert_eq!(r.available,160);assert_eq!(r.parsed.count,16);assert_eq!(r.parsed.parsed_bytes,128);assert_eq!(m,before);
}
#[test]
fn byte_instability_or_ready_revocation_during_queue_copy_blocks_snapshot(){
    for revoke in [false,true]{
        let b=control_queue();let(m,baseline,ready,rx)=pending_fixture(&b);let before=m.clone();let peer=smem_item(&m,435).unwrap();
        struct Changed<'a>{m:&'a[u8],target:usize,peer:usize,reads:std::cell::Cell<u32>,revoke:bool}
        impl Read for Changed<'_>{fn read8(&self,at:usize)->u8{
            if at==self.target{let n=self.reads.get();self.reads.set(n+1);if !self.revoke && n>0{return self.m[at]^1}}
            if self.revoke && at==self.peer+36 && self.reads.get()>0{return 0}self.m[at]
        }}
        let spy=Changed{m:&m,target:rx.offset+8+b.len()-1,peer:peer.offset,reads:std::cell::Cell::new(0),revoke};
        let r=pixel_glink_pending::read(&spy,&baseline,&ready,16,8);assert!(matches!(r,Err(e)if e==if revoke{Error::ReadyLost}else{Error::Unstable}));assert_eq!(m,before);
    }
}
#[test]
fn descriptor_progress_during_bounded_copy_is_rejected_without_touching_memory(){
    let b=control_queue();let(m,baseline,ready,rx)=pending_fixture(&b);let before=m.clone();
    struct Appended<'a>{m:&'a[u8],target:usize,seen:std::cell::Cell<bool>}
    impl Read for Appended<'_>{fn read8(&self,at:usize)->u8{if at==self.target{self.seen.set(true)}if at==DESCRIPTOR.offset+12 && self.seen.get(){return 72}self.m[at]}}
    let r=pixel_glink_pending::read(&Appended{m:&m,target:rx.offset+8+b.len()-1,seen:std::cell::Cell::new(false)},&baseline,&ready,16,8);assert!(matches!(r,Err(Error::Unstable)));assert_eq!(m,before);
}
#[test]
fn pending_mode_keeps_original_fixed_send_and_finds_ack_behind_open_without_consuming_it(){
    struct QueueIo(VersionIo);
    impl Io for QueueIo{
        fn byte(&mut self,i:usize,v:u8){self.0.byte(i,v)}fn head(&mut self){self.0.head()}fn tail(&mut self){self.0.tail()}
        fn notify(&mut self){self.0.notify();let b=control_queue();unsafe{let m=std::slice::from_raw_parts_mut(self.0.base,SMEM_BYTES);m[self.0.rx.offset+8..self.0.rx.offset+8+b.len()].copy_from_slice(&b);w32(m,DESCRIPTOR.offset+12,(8+b.len())as u32);}}
    }
    let(mut m,fresh,ready,rx)=version_fixture();let mut lock=0;let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let mut io=QueueIo(VersionIo::new(&mut m,&lock,rx));
    let r=pixel_glink_version::exchange_pending(&mut w,fresh,ready,&mut io,&mut Time{now:0,step:50000},|_|{});
    assert_eq!(r.error,Some(Error::Payload));assert!(r.tx_consumed);assert!(!r.ack_seen);assert!(!r.passed());let s=r.pending.unwrap().unwrap();assert!(s.parsed.ack_in_prefix);assert_eq!(s.parsed.count,4);
    assert_eq!((io.0.bytes,io.0.head_calls,io.0.tail_calls,io.0.notifications),(16,1,1,1));assert_eq!(io.0.events[16..],["head","tail","notify"]);assert_eq!(r.words[2],8);assert_eq!(u32::from_le_bytes(m[DESCRIPTOR.offset+8..DESCRIPTOR.offset+12].try_into().unwrap()),8);assert_eq!(lock,0);
}
#[test]
fn native_pending_adapter_cannot_be_reused_through_the_original_entrypoint(){
    let(mut m,fresh,ready,rx)=version_fixture();let before=m.clone();let mut locks=[0x1234,0,0x5678];let mut door=[0x11,0x22,0x33,0,0x55];
    struct QueueClock{base:*mut u8,door:*const u32,rx:usize,now:u64}
    impl Clock for QueueClock{fn frequency(&self)->u64{1_000_000}fn now(&mut self)->u64{self.now+=50000;unsafe{if self.door.add(3).read_volatile()==1<<12{
        let b=control_queue();let m=std::slice::from_raw_parts_mut(self.base,SMEM_BYTES);w32(m,DESCRIPTOR.offset,16);w32(m,DESCRIPTOR.offset+12,(8+b.len())as u32);m[self.rx+8..self.rx+8+b.len()].copy_from_slice(&b);
    }}self.now}}
    let mut clock=QueueClock{base:m.as_mut_ptr(),door:door.as_ptr(),rx:rx.offset,now:0};let mut w=unsafe{Window::new(m.as_ptr(),locks.as_mut_ptr().add(1))};let mut once=unsafe{pixel_glink_version::VersionOnce::new(fresh,m.as_mut_ptr().add(PART),door.as_mut_ptr())};
    let r=once.run_pending(&mut w,ready,&mut clock,|_|{});assert_eq!(r.error,Some(Error::Payload));assert!(r.pending.unwrap().unwrap().parsed.ack_in_prefix);assert_eq!(door,[0x11,0x22,0x33,1<<12,0x55]);assert_eq!(locks,[0x1234,0,0x5678]);
    let queue_len=control_queue().len();
    for i in 0..m.len(){if !(TX.offset..TX.offset+16).contains(&i) && !(DESCRIPTOR.offset..DESCRIPTOR.offset+16).contains(&i) && !(rx.offset+8..rx.offset+8+queue_len).contains(&i){assert_eq!(m[i],before[i],"native pending changed {i:x}");}}
    let original=m.clone();let(_,_,another,_)=version_fixture();let r=once.run(&mut w,another,&mut clock,|_|{});assert_eq!(r.error,Some(Error::Outbound));assert_eq!(m,original);
}
