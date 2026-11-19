include!("sunfish_glink_pending_test.rs");
#[path="../rust/src/pixel_glink_open.rs"] mod pixel_glink_open;
use pixel_glink_open::REQUEST;

fn native_queue()->Vec<u8>{
    let mut b=vec![];
    for(id,name)in [(1u16,b"glink_ssr".as_slice()),(2,b"IPCRTR".as_slice()),(3,b"ssm_rtr_modem_apps".as_slice())]{
        b.extend_from_slice(&2u16.to_le_bytes());b.extend_from_slice(&id.to_le_bytes());b.extend_from_slice(&((name.len()+1)as u32).to_le_bytes());b.extend_from_slice(name);b.push(0);while b.len()%8!=0{b.push(0xa5)}
    }b.extend_from_slice(&[1,0,1,0,0,0,0,0]);assert_eq!(b.len(),80);b
}
fn native_queue_ipcrtr_first()->Vec<u8>{
    let mut b=vec![];
    for(id,name)in [(1u16,b"IPCRTR".as_slice()),(2,b"glink_ssr".as_slice()),(3,b"ssm_rtr_modem_apps".as_slice())]{
        b.extend_from_slice(&2u16.to_le_bytes());b.extend_from_slice(&id.to_le_bytes());b.extend_from_slice(&((name.len()+1)as u32).to_le_bytes());b.extend_from_slice(name);b.push(0);while b.len()%8!=0{b.push(0xa5)}
    }b.extend_from_slice(&[1,0,1,0,0,0,0,0]);assert_eq!(b.len(),80);b
}
fn open_fixture()->(Vec<u8>,pixel_glink_version::VersionCheckpoint,qcom_modem_status::Item){open_fixture_case(false)}
fn open_fixture_for_generation(generation:u64)->(Vec<u8>,pixel_glink_version::VersionCheckpoint,qcom_modem_status::Item){
    open_fixture_order_for_generation(false,false,generation)
}
fn open_fixture_case(uppercase_unused:bool)->(Vec<u8>,pixel_glink_version::VersionCheckpoint,qcom_modem_status::Item){
    open_fixture_order(uppercase_unused,false)
}
fn open_fixture_ipcrtr_first()->(Vec<u8>,pixel_glink_version::VersionCheckpoint,qcom_modem_status::Item){
    open_fixture_order(false,true)
}
fn open_fixture_order(uppercase_unused:bool,ipcrtr_first:bool)->(Vec<u8>,pixel_glink_version::VersionCheckpoint,qcom_modem_status::Item){
    open_fixture_order_for_generation(uppercase_unused,ipcrtr_first,1)
}
fn open_fixture_order_for_generation(uppercase_unused:bool,ipcrtr_first:bool,generation:u64)->(Vec<u8>,pixel_glink_version::VersionCheckpoint,qcom_modem_status::Item){
    struct NativeQueueIo(VersionIo,bool,bool);
    impl Io for NativeQueueIo{fn byte(&mut self,i:usize,v:u8){self.0.byte(i,v)}fn head(&mut self){self.0.head()}fn tail(&mut self){self.0.tail()}
        fn notify(&mut self){self.0.notify();let mut b=if self.2{native_queue_ipcrtr_first()}else{native_queue()};if self.1{b[8..17].make_ascii_uppercase();b[48..66].make_ascii_uppercase();}unsafe{let m=std::slice::from_raw_parts_mut(self.0.base,SMEM_BYTES);m[self.0.rx.offset+8..self.0.rx.offset+88].copy_from_slice(&b);w32(m,DESCRIPTOR.offset+12,88);}}
    }
    let(mut m,fresh,ready,rx)=version_fixture_for_generation(generation);let mut lock=0;let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let mut io=NativeQueueIo(VersionIo::new(&mut m,&lock,rx),uppercase_unused,ipcrtr_first);
    let checkpoint=match pixel_glink_version::checkpoint_exchange(&mut w,fresh,ready,&mut io,&mut Time{now:0,step:50000},|_|{}){Ok(s)=>s,Err(r)=>panic!("checkpoint failed: {r:?}")};assert_eq!(lock,0);(m,checkpoint,rx)
}
struct OpenIo{base:*mut u8,lock:*const u32,rx:usize,bytes:usize,heads:usize,notifications:usize,fault:u8}
impl OpenIo{fn new(m:&mut[u8],lock:*const u32,rx:usize)->Self{Self{base:m.as_mut_ptr(),lock,rx,bytes:0,heads:0,notifications:0,fault:0}}}
impl pixel_glink_open::Io for OpenIo{
    fn byte(&mut self,i:usize,v:u8){assert_eq!(unsafe{*self.lock},1);assert_eq!(i,self.bytes);assert_eq!(v,REQUEST[i]);assert_eq!(self.heads,0);self.bytes+=1;if !(self.fault==1 && i==0){unsafe{self.base.add(TX.offset+16+i).write_volatile(v)}}
        if self.fault==7 && i==15{unsafe{let m=std::slice::from_raw_parts_mut(self.base,SMEM_BYTES);let p=smem_item(m,435).unwrap();w32(m,p.offset+36,0)}}
    }
    fn head(&mut self){assert_eq!(unsafe{*self.lock},1);self.heads+=1;if self.fault!=2{unsafe{self.base.add(DESCRIPTOR.offset+4).cast::<u32>().write_volatile(32u32.to_le())}}}
    fn notify(&mut self){assert_eq!(unsafe{*self.lock},0);self.notifications+=1;unsafe{let m=std::slice::from_raw_parts_mut(self.base,SMEM_BYTES);
        if self.fault==3{return}w32(m,DESCRIPTOR.offset,32);w32(m,DESCRIPTOR.offset+12,96);m[self.rx+88..self.rx+96].copy_from_slice(&[4,0,1,0,0,0,0,0]);
        if self.fault==4{m[self.rx+90]=2}if self.fault==5{w32(m,DESCRIPTOR.offset,48)}if self.fault==6{let p=smem_item(m,435).unwrap();w32(m,p.offset+36,1)}
    }}
}
#[test]
fn fixed_ipcrtr_request_leaves_all_receive_bytes_and_tail_unconsumed(){
    let(mut m,checkpoint,rx)=open_fixture();let before=m.clone();let mut locks=[0x11,0,0x22];let mut w=unsafe{Window::new(m.as_ptr(),locks.as_mut_ptr().add(1))};let mut io=OpenIo::new(&mut m,locks.as_ptr().wrapping_add(1),rx.offset);
    let r=pixel_glink_open::run(&mut w,checkpoint,&mut io,&mut Time{now:0,step:50000},|_|{});assert!(r.passed());assert_eq!(r.words[..4],[32,32,8,96]);assert_eq!(r.new_bytes,8);assert_eq!(r.header,Some(pixel_glink_open::OPEN_ACK));assert_eq!((io.bytes,io.heads,io.notifications),(16,1,1));assert_eq!(locks,[0x11,0,0x22]);
    for i in 0..m.len(){if !(TX.offset+16..TX.offset+32).contains(&i) && !(DESCRIPTOR.offset..DESCRIPTOR.offset+8).contains(&i) && !(DESCRIPTOR.offset+12..DESCRIPTOR.offset+16).contains(&i) && !(rx.offset+88..rx.offset+96).contains(&i){assert_eq!(m[i],before[i],"unexpected write {i:x}");}}
    assert_eq!(&m[rx.offset+8..rx.offset+88],&native_queue());assert_eq!(&m[TX.offset+16..TX.offset+32],&REQUEST);
}
#[test]
fn fixed_ipcrtr_request_accepts_the_observed_offer_order_and_preserves_rx(){
    let(mut m,checkpoint,rx)=open_fixture_ipcrtr_first();let before=m.clone();let mut lock=0;let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let mut io=OpenIo::new(&mut m,&lock,rx.offset);
    let r=pixel_glink_open::run(&mut w,checkpoint,&mut io,&mut Time{now:0,step:50000},|_|{});assert!(r.passed(),"{r:?}");assert_eq!(r.plan_detail,0);assert_eq!(r.guard_stage,8);assert_eq!(r.byte_writes,16);assert_eq!((io.bytes,io.heads,io.notifications),(16,1,1));
    assert_eq!(&m[rx.offset+8..rx.offset+88],&native_queue_ipcrtr_first());assert_eq!(&m[TX.offset+16..TX.offset+32],&REQUEST);assert_eq!(u32::from_le_bytes(m[DESCRIPTOR.offset+8..DESCRIPTOR.offset+12].try_into().unwrap()),8);
    assert_ne!(m,before);
}
#[test]
fn plan_snapshot_copies_the_checkpoint_bytes_without_open_transport_writes(){
    let (m, checkpoint, _) = open_fixture();
    let before = m.clone();
    let report = pixel_glink_open::snapshot_plan(checkpoint);
    assert_eq!(report.available, 80);
    assert_eq!(report.captured, 80);
    assert_eq!(report.plan_detail, 0);
    assert_eq!(&report.bytes[..80], &native_queue());
    assert_eq!(m, before);
}
#[test]
fn changed_checkpoint_bytes_indices_identity_or_tx_prevent_all_new_writes(){
    for fault in 0..5{let(mut m,checkpoint,rx)=open_fixture();match fault{0=>m[rx.offset+32+8]=b'i',1=>w32(&mut m,DESCRIPTOR.offset+8,16),2=>m[TX.offset+32]=1,3=>w32(&mut m,DESCRIPTOR.offset+12,96),_=>m[PART+48+20]^=1};let before=m.clone();let mut lock=0;let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let mut io=OpenIo::new(&mut m,&lock,rx.offset);
        let r=pixel_glink_open::run(&mut w,checkpoint,&mut io,&mut Time{now:0,step:50000},|_|{});assert!(r.error.is_some());assert_eq!((io.bytes,io.heads,io.notifications),(0,0,0));assert_eq!(m,before);
    }
}
#[test]
fn partial_publication_timeout_wrong_ack_tail_and_fatal_reply_never_retry(){
    for fault in 1..=6{let(mut m,checkpoint,rx)=open_fixture();let mut lock=0;let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let mut io=OpenIo::new(&mut m,&lock,rx.offset);io.fault=fault;
        let r=pixel_glink_open::run(&mut w,checkpoint,&mut io,&mut Time{now:0,step:50000},|_|{});assert!(!r.passed());assert_eq!(r.error,Some(match fault{1=>Error::Publication,2|5=>Error::Outbound,3=>Error::Timeout,4=>Error::Payload,_=>Error::Fatal}));assert_eq!(io.bytes,16);assert_eq!(io.heads,usize::from(fault!=1));assert_eq!(io.notifications,usize::from(fault>=3));assert_eq!(lock,0);assert_eq!(u32::from_le_bytes(m[DESCRIPTOR.offset+8..DESCRIPTOR.offset+12].try_into().unwrap()),8);
        assert_eq!(r.byte_writes,io.bytes as u32);assert_eq!(r.guard_stage,match fault{1=>4,2=>6,_=>8});
        if fault==4{assert_eq!(r.header,Some(pixel_glink_receive::Header{command:4,param1:2,param2:0}))}
    }
}
#[test]
fn bounded_reply_parser_can_find_open_ack_around_opaque_intent_metadata(){
    let mut b=vec![5,0,2,0,1,0,0,0];b.extend_from_slice(&[0xff;8]);b.extend_from_slice(&[4,0,1,0,0,0,0,0]);let(ack,first)=pixel_glink_open::reply(&b).unwrap();assert!(ack);assert_eq!(first.unwrap().command,5);
    for bytes in [&[5,0,2,0,16,0,0,0][..],&[5,0,2,0,1,0,0,0][..],&[4,0,2,0,0,0,0,0][..],&[9,0,1,0,0,0,0,0][..]]{assert!(pixel_glink_open::reply(bytes).is_err())}
    assert!(pixel_glink_open::reply(&[0;136]).is_err());
}
#[test]
fn invalid_or_stopped_clock_blocks_new_request(){
    struct Stopped;impl Clock for Stopped{fn frequency(&self)->u64{1_000_000}fn now(&mut self)->u64{0}}
    let(mut m,checkpoint,rx)=open_fixture();let before=m.clone();let mut lock=0;let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let mut io=OpenIo::new(&mut m,&lock,rx.offset);let r=pixel_glink_open::run(&mut w,checkpoint,&mut io,&mut Stopped,|_|{});assert_eq!(r.error,Some(Error::Clock));assert_eq!(io.bytes,0);assert_eq!(m,before);
}
#[test]
fn ready_revocation_after_new_bytes_are_staged_blocks_head_publication(){
    let(mut m,checkpoint,rx)=open_fixture();let mut lock=0;let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let mut io=OpenIo::new(&mut m,&lock,rx.offset);io.fault=7;
    let r=pixel_glink_open::run(&mut w,checkpoint,&mut io,&mut Time{now:0,step:50000},|_|{});assert_eq!(r.error,Some(Error::ReadyLost));assert_eq!((io.bytes,io.heads,io.notifications),(16,0,0));
}
#[test]
fn ready_revocation_during_reply_tx_verification_cannot_claim_open_ack(){
    let(mut m,checkpoint,rx)=open_fixture();let(ready,base,old)=checkpoint.into_parts();m[TX.offset+16..TX.offset+32].copy_from_slice(&REQUEST);w32(&mut m,DESCRIPTOR.offset,32);w32(&mut m,DESCRIPTOR.offset+4,32);w32(&mut m,DESCRIPTOR.offset+12,96);m[rx.offset+88..rx.offset+96].copy_from_slice(&[4,0,1,0,0,0,0,0]);let peer=smem_item(&m,435).unwrap();let before=m.clone();
    struct Revoked<'a>{m:&'a[u8],peer:usize,seen:std::cell::Cell<bool>}
    impl Read for Revoked<'_>{fn read8(&self,at:usize)->u8{if at==TX.offset+TX.size-1{self.seen.set(true)}if at==self.peer+36 && self.seen.get(){return 0}self.m[at]}}
    let spy=Revoked{m:&m,peer:peer.offset,seen:std::cell::Cell::new(false)};
    let mut r=pixel_glink_open::Report{error:None,staged:true,head_store:true,notifications:1,tx_consumed:false,open_ack:false,words:[0;8],available:0,new_bytes:0,header:None,samples:0,guard_stage:0,plan_detail:0,plan_shape:None,byte_writes:0};
    assert!(matches!(pixel_glink_open::sample(&spy,&base,&ready,&old,16,88,&mut r),Err(Error::ReadyLost)));assert!(!r.open_ack);assert!(!r.tx_consumed);assert_eq!(m,before);
}
#[test]
fn native_open_adapter_is_exact_and_terminal_without_any_rx_tail_store(){
    let(mut m,checkpoint,rx)=open_fixture();let before=m.clone();let mut locks=[0x12,0,0x34];let mut door=[0x11,0x22,0x33,0,0x55];
    struct Peer{base:*mut u8,door:*const u32,rx:usize,now:u64}
    impl Clock for Peer{fn frequency(&self)->u64{1_000_000}fn now(&mut self)->u64{self.now+=50000;unsafe{if self.door.add(3).read_volatile()==1<<12{let m=std::slice::from_raw_parts_mut(self.base,SMEM_BYTES);w32(m,DESCRIPTOR.offset,32);w32(m,DESCRIPTOR.offset+12,96);m[self.rx+88..self.rx+96].copy_from_slice(&[4,0,1,0,0,0,0,0]);}}self.now}}
    let mut clock=Peer{base:m.as_mut_ptr(),door:door.as_ptr(),rx:rx.offset,now:0};let mut w=unsafe{Window::new(m.as_ptr(),locks.as_mut_ptr().add(1))};let mut once=unsafe{pixel_glink_open::OpenOnce::new(checkpoint,m.as_mut_ptr().add(PART),door.as_mut_ptr())};let r=once.run(&mut w,&mut clock,|_|{});assert!(r.passed());assert_eq!(door,[0x11,0x22,0x33,1<<12,0x55]);assert_eq!(locks,[0x12,0,0x34]);
    for i in 0..m.len(){if !(TX.offset+16..TX.offset+32).contains(&i) && !(DESCRIPTOR.offset..DESCRIPTOR.offset+8).contains(&i) && !(DESCRIPTOR.offset+12..DESCRIPTOR.offset+16).contains(&i) && !(rx.offset+88..rx.offset+96).contains(&i){assert_eq!(m[i],before[i],"native open changed {i:x}");}}
    let completed=m.clone();assert_eq!(once.run(&mut w,&mut clock,|_|{}).error,Some(Error::Outbound));assert_eq!(m,completed);
}
#[test]
fn reply_reader_stays_inside_128_new_bytes_and_rejects_changed_copies(){
    for unstable in [false,true]{
        let(mut m,checkpoint,rx)=open_fixture();let(ready,base,old)=checkpoint.into_parts();m[TX.offset+16..TX.offset+32].copy_from_slice(&REQUEST);w32(&mut m,DESCRIPTOR.offset,32);w32(&mut m,DESCRIPTOR.offset+4,32);w32(&mut m,DESCRIPTOR.offset+12,248);let before=m.clone();
        struct Bounded<'a>{m:&'a[u8],rx:usize,unstable:bool,reads:std::cell::Cell<u32>}
        impl Read for Bounded<'_>{fn read8(&self,at:usize)->u8{
            if(self.rx..self.rx+0x4000).contains(&at){assert!(at<self.rx+88+128,"read beyond new RX prefix: {at:x}")}
            if at==self.rx+88+127{let n=self.reads.get();self.reads.set(n+1);if self.unstable && n>0{return self.m[at]^1}}self.m[at]
        }}
        let spy=Bounded{m:&m,rx:rx.offset,unstable,reads:std::cell::Cell::new(0)};
        let mut r=pixel_glink_open::Report{error:None,staged:true,head_store:true,notifications:1,tx_consumed:false,open_ack:false,words:[0;8],available:0,new_bytes:0,header:None,samples:0,guard_stage:0,plan_detail:0,plan_shape:None,byte_writes:0};
        assert!(matches!(pixel_glink_open::sample(&spy,&base,&ready,&old,16,88,&mut r),Err(e)if e==if unstable{Error::Unstable}else{Error::Payload}));assert_eq!(m,before);
    }
}
#[test]
fn plan_requires_exact_wire_case_channel_ids_and_complete_ack(){
    let(_,checkpoint,_)=open_fixture();let(_,_,s)=checkpoint.into_parts();assert!(pixel_glink_open::plan(&s).is_ok());
    for fault in 0..5{let mut changed=s;match fault{0=>changed.parsed.packets[1].name[0]=b'i',1=>changed.parsed.packets[1].header.param1=3,2=>changed.parsed.packets[3].header.param2=7,3=>changed.captured=72,_=>changed.words[2]=16};assert_eq!(pixel_glink_open::plan(&changed),Err(Error::Payload));assert_eq!(pixel_glink_open::plan_detail(&changed),[5,4,6,1,2][fault]);}
}
#[test]
fn stopped_open_shape_reports_first_rejected_packet_fields(){
    let(_,checkpoint,_)=open_fixture();let(_,_,snapshot)=checkpoint.into_parts();let mut changed=snapshot;
    changed.parsed.packets[1].header.param1=9;
    let d=pixel_glink_open::plan_shape_diagnostic(&changed).unwrap();
    assert_eq!(d,pixel_glink_open::PlanShapeDiagnostic{packet:2,expected_offset:24,offset:24,expected_id:2,id:9,expected_name_bytes:7,name_bytes:7,command:2,name_prefix:*b"IPCRTR\0\0\0\0\0\0",name_len:6,packets:[
        pixel_glink_open::PlanPacketDiagnostic{offset:0,command:2,id:1,bytes:10,name_prefix:*b"glink_ssr\0\0\0",name_len:9},
        pixel_glink_open::PlanPacketDiagnostic{offset:24,command:2,id:9,bytes:7,name_prefix:*b"IPCRTR\0\0\0\0\0\0",name_len:6},
        pixel_glink_open::PlanPacketDiagnostic{offset:40,command:2,id:3,bytes:19,name_prefix:*b"ssm_rtr_mode",name_len:18},
        pixel_glink_open::PlanPacketDiagnostic{offset:72,command:1,id:1,bytes:0,name_prefix:[0;12],name_len:0},
    ]});
    assert_eq!(pixel_glink_open::plan(&changed),Err(Error::Payload));
    assert_eq!(pixel_glink_open::plan_shape_diagnostic(&snapshot),None);
}
#[test]
fn unused_channel_name_case_does_not_block_exact_ipcrtr_request(){
    let(mut m,checkpoint,rx)=open_fixture_case(true);let mut lock=0;let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let mut io=OpenIo::new(&mut m,&lock,rx.offset);
    let r=pixel_glink_open::run(&mut w,checkpoint,&mut io,&mut Time{now:0,step:50000},|_|{});assert!(r.passed());assert_eq!(r.plan_detail,0);assert_eq!(r.guard_stage,8);assert_eq!(r.byte_writes,16);assert_eq!((io.bytes,io.heads,io.notifications),(16,1,1));
    assert_eq!(&m[rx.offset+8+8..rx.offset+8+17],b"GLINK_SSR");assert_eq!(&m[rx.offset+8+48..rx.offset+8+66],b"SSM_RTR_MODEM_APPS");assert_eq!(u32::from_le_bytes(m[DESCRIPTOR.offset+8..DESCRIPTOR.offset+12].try_into().unwrap()),8);
}
