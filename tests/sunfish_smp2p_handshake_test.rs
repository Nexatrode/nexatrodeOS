// AP allocation/publication against host RAM; peer/clock/auth are synthetic.
extern crate self as personal_os_rust_server;
#[path="../rust/src/qcom_mdt.rs"] mod qcom_mdt;
#[path="../rust/src/qcom_pas_metadata.rs"] mod qcom_pas_metadata;
#[path="../rust/src/qcom_pas_init.rs"] mod qcom_pas_init;
#[path="../rust/src/qcom_pas_mem_setup.rs"] mod qcom_pas_mem_setup;
#[path="../rust/src/qcom_modem_image.rs"] mod qcom_modem_image;
#[path="../rust/src/qcom_pas_auth.rs"] mod qcom_pas_auth;
#[path="../rust/src/qcom_modem_status.rs"] mod qcom_modem_status;
#[path="../rust/src/pixel_modem_status.rs"] mod pixel_modem_status;
use qcom_modem_status::{Error,Read,SMEM_BYTES,PAYLOAD_BYTES};
use pixel_modem_status::{Clock,Window};
fn w16(m:&mut[u8],at:usize,v:u16){m[at..at+2].copy_from_slice(&v.to_le_bytes());}
fn w32(m:&mut[u8],at:usize,v:u32){m[at..at+4].copy_from_slice(&v.to_le_bytes());}
fn fixture()->Vec<u8>{
    let mut m=vec![0;SMEM_BYTES];w32(&mut m,192,1);w32(&mut m,92,12<<16);
    let t=SMEM_BYTES-4096;w32(&mut m,t,0x434f5424);w32(&mut m,t+4,1);w32(&mut m,t+8,1);
    w32(&mut m,t+32,0x10000);w32(&mut m,t+36,0x1000);w16(&mut m,t+46,1);w32(&mut m,t+48,32);
    w32(&mut m,0x10000,0x54525024);w16(&mut m,0x10006,1);w32(&mut m,0x10008,0x1000);
    w32(&mut m,0x1000c,32);w32(&mut m,0x10010,0x1000);m
}
fn payload(value:u32)->[u8;PAYLOAD_BYTES]{
    let mut b=[0;PAYLOAD_BYTES];w32(&mut b,0,0x504d5324);w32(&mut b,4,0x101);
    w16(&mut b,8,1);w16(&mut b,12,16);w16(&mut b,14,1);
    b[20..32].copy_from_slice(b"slave-kernel");w32(&mut b,36,value);b
}
fn publish(m:&mut[u8],value:u32,cached:bool){
    let b=payload(value);
    if cached {
        let h=0x10000+0x1000-32;let at=h-344;
        w16(m,h,0xa5a5);w16(m,h+2,435);w32(m,h+4,344);w16(m,h+8,4);
        w32(m,0x10010,(at-0x10000)as u32);m[at..at+PAYLOAD_BYTES].copy_from_slice(&b);
    } else {
        let h=0x10020;w16(m,h,0xa5a5);w16(m,h+2,435);w32(m,h+4,344);w16(m,h+8,4);
        w32(m,0x1000c,32+16+344);m[h+16..h+16+PAYLOAD_BYTES].copy_from_slice(&b);
    }
}
struct Bytes<'a>(&'a[u8]);impl Read for Bytes<'_>{fn read8(&self,at:usize)->u8{self.0[at]}}
fn pending_auth()->qcom_pas_auth::AuthOnce {
    let p=qcom_pas_metadata::Prepared{metadata_physical:0x80001000,metadata_length:9100,program_headers:31,loadable_segments:29,linked_start:0x8b000000,linked_end:0x94800000};
    let fresh=qcom_pas_mem_setup::tests::consumed();
    qcom_pas_auth::AuthOnce::after_image(qcom_modem_image::tests::auth_fixture(true),p,&fresh).unwrap()
}
struct Secure;
impl qcom_pas_init::Transport for Secure {
    fn invoke(&mut self,r:&qcom_pas_init::Request)->Result<[u64;8],u64>{Ok([0,u64::from(r.words()[0]==0x42000601),0,0,0,0,0,0])}
}
fn available()->qcom_pas_auth::AuthOnce{let mut a=pending_auth();a.check(&mut Secure,|_|{}).unwrap();a}
fn complete(mut a:qcom_pas_auth::AuthOnce)->qcom_pas_auth::AuthOnce{
    a.authenticate(&mut Secure,&qcom_pas_mem_setup::tests::consumed(),|_|{}).unwrap();a
}
fn permit(a:&mut qcom_pas_auth::AuthOnce)->qcom_pas_auth::StatusPermit{a.status_prepare(&qcom_pas_mem_setup::tests::consumed()).unwrap()}
struct Time{now:u64,step:u64}
impl Clock for Time{fn now(&mut self)->u64{self.now=self.now.wrapping_add(self.step);self.now}fn frequency(&self)->u64{1_000_000}}
#[path="../rust/src/pixel_smp2p_handshake.rs"] mod pixel_smp2p_handshake;
use pixel_smp2p_handshake::Handshake;
const PART:usize=0x11d000;
fn mapped()->Vec<u8>{
    let mut m=fixture();m[0x10000..0x11000].fill(0);let t=SMEM_BYTES-4096;
    w32(&mut m,t+32,PART as u32);w32(&mut m,t+36,0x7c000);
    w32(&mut m,PART,0x54525024);w16(&mut m,PART+6,1);w32(&mut m,PART+8,0x7c000);
    w32(&mut m,PART+12,32);w32(&mut m,PART+16,0x7c000);m
}
fn remote(m:&mut[u8],ready:Option<u32>){
    let cursor=u32::from_le_bytes(m[PART+12..PART+16].try_into().unwrap())as usize;
    let h=PART+cursor;w16(m,h,0xa5a5);w16(m,h+2,435);w32(m,h+4,344);w16(m,h+8,4);
    let mut b=payload(ready.unwrap_or(0));if ready.is_none(){w16(&mut b,14,0);}
    m[h+16..h+16+340].copy_from_slice(&b);w32(m,PART+12,(cursor+360)as u32);
}
struct PeerClock{now:u64,base:*mut u8,respond:bool,tamper:bool}
impl Clock for PeerClock{
    fn frequency(&self)->u64{1_000_000}
    fn now(&mut self)->u64{
        self.now+=50000;let m=unsafe{std::slice::from_raw_parts_mut(self.base,SMEM_BYTES)};
        if u32::from_le_bytes(m[PART+12..PART+16].try_into().unwrap())>=752{
            if self.respond{w32(m,PART+408+4,1);w16(m,PART+408+14,1);w32(m,PART+408+36,2);}
            if self.tamper{m[PART+48+20]^=1;}
        }self.now
    }
}
#[test]
fn publishes_inside_empty_first_page_before_auth_and_never_writes_smem_after_auth(){
    let mut m=mapped();let original=m.clone();let mut lock=[0x12345678,0,0xa5a5a5a5];
    let mut door=[0x11111111,0x22222222,0x33333333,0,0x55555555];
    let mut w=unsafe{Window::new(m.as_ptr(),lock.as_mut_ptr().add(1))};let b=w.before_auth().unwrap();
    pixel_smp2p_handshake::check_initial(&mut w,&b).unwrap();let mut a=available();
    let mut h=unsafe{Handshake::new(m.as_mut_ptr().add(PART),door.as_mut_ptr())};
    h.prepare(&mut w,&b,permit(&mut a)).unwrap();assert!(h.prepared());assert_eq!(h.ap_offset(),0x11d030);
    assert_eq!(a.report().issued_mask,3);assert_eq!(a.report().phase,qcom_pas_auth::Phase::Available);
    assert_eq!(door[3],0);assert_eq!(h.last_directory().unwrap().free_uncached,392);
    for i in 0..m.len(){if !(PART+12..PART+16).contains(&i)&&!(PART+32..PART+392).contains(&i){assert_eq!(m[i],original[i],"unexpected write at {i:x}");}}
    let before_remote=m.clone();remote(&mut m,None);let original=m.clone();
    let r=h.observe(&mut w,b,complete(a),&mut PeerClock{now:0,base:m.as_mut_ptr(),respond:true,tamper:false},|_|{});
    assert_eq!(r.status.error,None);assert!(r.status.ready_bit_seen);assert!(r.peer_negotiated);
    assert!(r.published);assert_eq!(r.ap_offset,0x11d030);assert_eq!(r.doorbells,1);
    assert_eq!(door,[0x11111111,0x22222222,0x33333333,1<<14,0x55555555]);assert_eq!(lock,[0x12345678,0,0xa5a5a5a5]);
    for i in 0..m.len(){
        let peer_changes=(PART+412..PART+416).contains(&i)||(PART+422..PART+424).contains(&i)||(PART+444..PART+448).contains(&i);
        if !peer_changes{assert_eq!(m[i],original[i],"AP wrote after AUTH at {i:x}");}
    }
    assert_eq!(&m[PART+32..PART+392],&before_remote[PART+32..PART+392]);
    let ap=qcom_modem_status::directory_for(&Bytes(&m),428).unwrap().item.unwrap();
    assert_eq!(ap.offset,0x11d030);assert_eq!(&m[ap.offset+20..ap.offset+33],b"master-kernel");
    assert_eq!(m[ap.offset+4],1);assert_eq!(&m[ap.offset+5..ap.offset+10],&[0;5]);assert_eq!(&m[ap.offset+16..ap.offset+20],&[0;4]);
}
#[test]
fn missing_or_unnegotiated_peer_times_out_without_allocation_or_ssr_retry(){
    for present in [false,true]{
        let mut m=mapped();let mut lock=0;let mut door=[0;4];let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let b=w.before_auth().unwrap();
        let mut a=available();let mut h=unsafe{Handshake::new(m.as_mut_ptr().add(PART),door.as_mut_ptr())};h.prepare(&mut w,&b,permit(&mut a)).unwrap();
        if present{remote(&mut m,Some(2));w32(&mut m,PART+408+16,1);}let original=m.clone();
        let r=h.observe(&mut w,b,complete(a),&mut Time{now:0,step:50000},|_|{});
        assert_eq!(r.status.error,Some(Error::Timeout));assert!(!r.status.ready_bit_seen);assert_eq!(r.doorbells,u32::from(present));
        assert!(r.published);assert_eq!(m,original);assert_eq!(lock,0);
    }
}
#[test]
fn failed_auth_and_fatal_peer_cannot_notify_or_change_prepared_ap_item(){
    for fatal in [false,true]{
        let mut m=mapped();let mut lock=0;let mut door=[0;4];let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let b=w.before_auth().unwrap();
        let mut a=available();let mut h=unsafe{Handshake::new(m.as_mut_ptr().add(PART),door.as_mut_ptr())};h.prepare(&mut w,&b,permit(&mut a)).unwrap();
        remote(&mut m,Some(if fatal{1}else{2}));let original=m.clone();
        let r=h.observe(&mut w,b,if fatal{complete(a)}else{a},&mut Time{now:0,step:50000},|_|{});
        assert_eq!(r.status.error,Some(if fatal{Error::Fatal}else{Error::Auth}));assert_eq!(m,original);assert_eq!(door,[0;4]);assert_eq!(lock,0);
        if fatal{assert_eq!(r.status.status.unwrap().slave_kernel,Some(1));}
    }
}
#[test]
fn any_allocation_or_route_change_before_preparation_stops_without_writes(){
    for change in 0..3{
        let mut m=mapped();let mut lock=0;let mut door=[0;4];let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let b=w.before_auth().unwrap();
        if change<2{remote(&mut m,None);if change==1{w16(&mut m,PART+34,428);}}else{w32(&mut m,SMEM_BYTES-4096+48,64);}
        let original=m.clone();let mut a=available();let mut h=unsafe{Handshake::new(m.as_mut_ptr().add(PART),door.as_mut_ptr())};
        assert!(h.prepare(&mut w,&b,permit(&mut a)).is_err());assert!(!h.prepared());assert_eq!(m,original);assert_eq!(door,[0;4]);assert_eq!(lock,0);
        assert_eq!(h.last_directory().unwrap().free_uncached,if change<2{392}else{32});
    }
}
#[test]
fn tampered_ap_payload_stops_without_repair_or_second_notification(){
    let mut m=mapped();let mut lock=0;let mut door=[0;4];let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let b=w.before_auth().unwrap();
    let mut a=available();let mut h=unsafe{Handshake::new(m.as_mut_ptr().add(PART),door.as_mut_ptr())};h.prepare(&mut w,&b,permit(&mut a)).unwrap();remote(&mut m,None);
    let r=h.observe(&mut w,b,complete(a),&mut PeerClock{now:0,base:m.as_mut_ptr(),respond:false,tamper:true},|_|{});
    assert_eq!(r.status.error,Some(Error::Outbound));assert!(r.published);assert!(!r.status.ready_bit_seen);assert!(r.doorbells<=1);assert_eq!(lock,0);
}
#[test]
fn remote_growth_beyond_first_page_after_auth_never_causes_a_late_ap_allocation(){
    let mut m=mapped();let mut lock=0;let mut door=[0;4];let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let b=w.before_auth().unwrap();
    let mut a=available();let mut h=unsafe{Handshake::new(m.as_mut_ptr().add(PART),door.as_mut_ptr())};h.prepare(&mut w,&b,permit(&mut a)).unwrap();
    remote(&mut m,Some(2));w32(&mut m,PART+408+4,1);
    let extra=PART+752;w16(&mut m,extra,0xa5a5);w16(&mut m,extra+2,999);w32(&mut m,extra+4,8192);w32(&mut m,PART+12,8960);
    let original=m.clone();let r=h.observe(&mut w,b,complete(a),&mut Time{now:0,step:50000},|_|{});
    assert_eq!(r.status.error,None);assert!(r.status.ready_bit_seen);assert_eq!(r.ap_offset,0x11d030);assert_eq!(r.doorbells,1);assert_eq!(m,original);assert_eq!(lock,0);
}
#[test]
fn dropped_ap_writes_cannot_commit_or_notify_and_preparation_is_not_reissued(){
    let m=mapped();let mut lock=0;let mut door=[0;4];let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let b=w.before_auth().unwrap();let original=m.clone();
    // Separate valid host RAM simulates device writes not appearing in the read view.
    let mut dropped=vec![0;4096];let mut h=unsafe{Handshake::new(dropped.as_mut_ptr(),door.as_mut_ptr())};let mut a=available();
    assert_eq!(h.prepare(&mut w,&b,permit(&mut a)),Err(Error::Publication));assert_eq!(m,original);assert_eq!(door,[0;4]);assert!(!h.prepared());assert_eq!(lock,0);
    assert_eq!(a.report().issued_mask,3);let written=dropped.clone();let mut another=available();
    assert_eq!(h.prepare(&mut w,&b,permit(&mut another)),Err(Error::Outbound));assert_eq!(dropped,written);assert_eq!(m,original);
}
