include!("sunfish_glink_prepare_test.rs");
#[path="../rust/src/pixel_glink_inventory.rs"] mod pixel_glink_inventory;
#[path="../rust/src/pixel_glink_receive.rs"] mod pixel_glink_receive;
#[path="../rust/src/pixel_glink_boot.rs"] mod pixel_glink_boot;
use pixel_glink_boot::{Boot,BootIdentity};
fn boot_fixture()->(Vec<u8>,Handshake,pixel_modem_status::Baseline,Boot,Box<[u32;4]>){
    boot_fixture_for_generation(1)
}
fn boot_fixture_for_generation(generation:u64)->(Vec<u8>,Handshake,pixel_modem_status::Baseline,Boot,Box<[u32;4]>){
    let mut m=mapped();let mut lock=0;let mut door=Box::new([0;4]);let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let b=w.before_auth().unwrap();
    let mut a=available();let mut h=unsafe{Handshake::new_for_generation(m.as_mut_ptr().add(PART),door.as_mut_ptr(),generation)}.unwrap();h.prepare(&mut w,&b,permit(&mut a)).unwrap();
    let boot=unsafe{Boot::prepare(&mut w,&h,a,&qcom_pas_mem_setup::tests::consumed(),m.as_mut_ptr().add(PART),m.as_mut_ptr().add(PART+4096))}.unwrap();
    (m,h,b,boot,door)
}
#[derive(Default)]struct AuthCalls{calls:usize,fault:Option<Result<[u64;8],u64>>}
impl qcom_pas_init::Transport for AuthCalls{fn invoke(&mut self,r:&qcom_pas_init::Request)->Result<[u64;8],u64>{
    assert_eq!(*r.words(),[0x42000205,1,4,0,0,0,0,0]);self.calls+=1;self.fault.unwrap_or(Ok([0;8]))
}}
fn rx_allocation(m:&mut[u8])->qcom_modem_status::Item{
    let cursor=u32::from_le_bytes(m[PART+12..PART+16].try_into().unwrap())as usize;let at=PART+cursor;
    w16(m,at,0xa5a5);w16(m,at+2,480);w32(m,at+4,0x4000);w16(m,at+8,0);w16(m,at+10,0);
    m[at+16..at+16+0x4000].fill(0);w32(m,PART+12,(cursor+16+0x4000)as u32);
    qcom_modem_status::Item{offset:at+16,size:0x4000}
}
fn started_fixture()->(Vec<u8>,BootIdentity,pixel_smp2p_handshake::ReadyObservation,qcom_modem_status::Item){
    let(mut m,mut h,b,boot,mut door)=boot_fixture();let mut lock=0;let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let mut io=AuthCalls::default();
    let started=match boot.authenticate(&mut w,&h,&mut io,&qcom_pas_mem_setup::tests::consumed(),|_|{}){Ok(s)=>s,Err(_)=>panic!("synthetic boot failed")};assert_eq!(io.calls,1);
    let(auth,identity)=started.into_parts();let peer=u32::from_le_bytes(m[PART+12..PART+16].try_into().unwrap())as usize+PART+16;
    remote(&mut m,Some(6));w32(&mut m,peer+4,1);let rx=rx_allocation(&mut m);
    let r=h.observe(&mut w,b,auth,&mut Time{now:0,step:50000},|_|{});assert!(r.status.ready_bit_seen);assert_eq!(door[3],1<<14);door[3]=0;
    (m,identity,h.take_ready_observation().unwrap(),rx)
}
#[test]
fn real_prepare_auth_and_ready_gates_produce_only_a_readonly_poststart_identity(){
    let(m,identity,ready,_)=started_fixture();let original=m.clone();let mut locks=[0x1234,0,0x5678];let mut w=unsafe{Window::new(m.as_ptr(),locks.as_mut_ptr().add(1))};
    let r=pixel_glink_boot::read(&mut w,identity,ready).unwrap();assert_eq!(r.receive.inventory.present_mask(),7);assert_eq!(r.receive.words,[0;8]);
    assert_eq!(r.tx_zero_passes,2);assert_eq!(r.receive.available,0);assert_eq!(m,original);assert_eq!(locks,[0x1234,0,0x5678]);
}
#[test]
fn any_pre_auth_memory_or_identity_change_blocks_the_secure_call(){
    for fault in 0..6{
        let(mut m,h,_b,boot,_door)=boot_fixture();match fault{0=>m[TX.offset+TX.size-1]=1,1=>m[DESCRIPTOR.offset+4]=8,2=>m[PART+48+20]^=1,
            3=>w16(&mut m,DESCRIPTOR.offset-16,0),4=>remote(&mut m,Some(6)),_=>{rx_allocation(&mut m);}};
        let original=m.clone();let mut lock=0;let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let mut io=AuthCalls::default();
        assert!(boot.authenticate(&mut w,&h,&mut io,&qcom_pas_mem_setup::tests::consumed(),|_|{}).is_err());assert_eq!(io.calls,0);assert_eq!(m,original);
    }
}
#[test]
fn stale_metadata_and_unknown_secure_replies_never_produce_started_identity(){
    for fault in 0..4{
        let(m,h,_b,boot,_door)=boot_fixture();let mut lock=0;let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let mut io=AuthCalls::default();let mut fresh=qcom_pas_mem_setup::tests::consumed();
        match fault{0=>fresh.unstable_bytes=1,1=>io.fault=Some(Err(6)),2=>io.fault=Some(Ok([u64::MAX,0,0,0,0,0,0,0])),_=>io.fault=Some(Ok([0,1,0,0,0,0,0,0]))};
        let result=boot.authenticate(&mut w,&h,&mut io,&fresh,|_|{});assert!(result.is_err());assert_eq!(io.calls,usize::from(fault!=0));
    }
}
#[test]
fn post_auth_ap_index_or_tx_buffer_tampering_is_rejected_without_writes(){
    for fault in 0..6{
        let(mut m,identity,ready,_)=started_fixture();match fault{0=>w32(&mut m,DESCRIPTOR.offset,8),1=>w32(&mut m,DESCRIPTOR.offset+4,8),2=>w32(&mut m,DESCRIPTOR.offset+8,8),
            3=>w32(&mut m,DESCRIPTOR.offset+16,1),4=>m[TX.offset+TX.size-1]=1,_=>w16(&mut m,TX.offset-16+2,999)};
        let original=m.clone();assert!(pixel_glink_boot::inspect(&RawRead(m.as_ptr()),identity,ready).is_err());assert_eq!(m,original);
    }
}
#[test]
fn bounded_remote_header_is_observed_but_not_consumed_or_acknowledged(){
    let(mut m,identity,ready,rx)=started_fixture();m[rx.offset..rx.offset+8].copy_from_slice(&[0,0,1,0,1,0,0,0]);w32(&mut m,DESCRIPTOR.offset+12,8);let original=m.clone();
    let r=pixel_glink_boot::inspect(&RawRead(m.as_ptr()),identity,ready).unwrap();assert!(r.receive.header.unwrap().version_offer());assert_eq!(r.receive.available,8);assert_eq!(m,original);
}
#[test]
fn no_rx_payload_outside_one_pending_header_is_read(){
    let(mut m,identity,ready,rx)=started_fixture();m[rx.offset..rx.offset+8].copy_from_slice(&[0,0,1,0,0,0,0,0]);w32(&mut m,DESCRIPTOR.offset+12,8);
    struct SpyRead<'a>{m:&'a[u8],rx:qcom_modem_status::Item,rx_reads:std::cell::Cell<usize>}
    impl Read for SpyRead<'_>{fn read8(&self,at:usize)->u8{if(self.rx.offset..self.rx.offset+self.rx.size).contains(&at){assert!(at<self.rx.offset+8);self.rx_reads.set(self.rx_reads.get()+1);}self.m[at]}}
    let spy=SpyRead{m:&m,rx,rx_reads:std::cell::Cell::new(0)};let r=pixel_glink_boot::inspect(&spy,identity,ready).unwrap();assert_eq!(r.tx_zero_passes,2);assert_eq!(spy.rx_reads.get(),4*8);
}
#[test]
fn ready_revocation_during_tx_readback_fails_the_final_readonly_gate(){
    let(m,identity,ready,_)=started_fixture();let peer=smem_item(&m,435).unwrap();
    struct Revoked<'a>{m:&'a[u8],peer:usize,passes:std::cell::Cell<usize>}
    impl Read for Revoked<'_>{fn read8(&self,at:usize)->u8{
        if at==TX.offset+TX.size-1{self.passes.set(self.passes.get()+1)}
        if at==self.peer+36 && self.passes.get()>=2{return 0}self.m[at]
    }}
    let spy=Revoked{m:&m,peer:peer.offset,passes:std::cell::Cell::new(0)};
    assert!(matches!(pixel_glink_boot::inspect(&spy,identity,ready),Err(Error::ReadyLost)));assert_eq!(spy.passes.get(),2);
}
