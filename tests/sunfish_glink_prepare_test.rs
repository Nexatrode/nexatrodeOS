include!("sunfish_smp2p_handshake_test.rs");
#[path="../rust/src/pixel_glink_prepare.rs"] mod pixel_glink_prepare;
use pixel_glink_prepare::{Preparation,Write,START,END,WINDOW,DESCRIPTOR,TX};
fn prepared_memory()->(Vec<u8>,Handshake,qcom_pas_auth::AuthOnce){
    let mut m=mapped();m[PART+32..PART+WINDOW].fill(0x6b);
    let mut lock=0;let mut door=[0;4];let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let b=w.before_auth().unwrap();
    let mut a=available();let mut h=unsafe{Handshake::new(m.as_mut_ptr().add(PART),door.as_mut_ptr())};
    h.prepare(&mut w,&b,permit(&mut a)).unwrap();
    // No use of h's doorbell pointer occurs in this pre-AUTH stage.
    (m,h,a)
}
fn memory_permit(a:qcom_pas_auth::AuthOnce)->qcom_pas_auth::GlinkMemoryPermit{a.into_glink_memory_preparation(&qcom_pas_mem_setup::tests::consumed()).unwrap()}
struct RawRead(*const u8);impl Read for RawRead{fn read8(&self,at:usize)->u8{assert!(at<SMEM_BYTES);unsafe{self.0.add(at).read_volatile()}}}
struct Spy{base:*mut u8,writes:Vec<(usize,u8)>,commits:usize,drop_first:bool,clobber_guard:bool}
impl Spy{fn new(m:&mut[u8])->Self{Self{base:m.as_mut_ptr(),writes:vec![],commits:0,drop_first:false,clobber_guard:false}}}
impl Write for Spy{
    fn byte(&mut self,at:usize,value:u8){assert!((PART+START..PART+END).contains(&at));self.writes.push((at,value));
        if !(self.drop_first && at==PART+START){unsafe{self.base.add(at).write_volatile(value)}}
    }
    fn cursor(&mut self,value:u32){
        assert_eq!(value,END as u32);assert_eq!(self.writes.len(),END-START);self.commits+=1;
        unsafe{self.base.add(PART+12).cast::<u32>().write_unaligned(value.to_le());
            if self.clobber_guard{self.base.add(PART+WINDOW-1).write_volatile(0);}}
    }
}
#[test]
fn initializes_two_fresh_items_before_one_cursor_commit_preserving_every_other_byte(){
    let(mut m,h,a)=prepared_memory();let original=m.clone();let mut io=Spy::new(&mut m);
    let r=pixel_glink_prepare::stage(&RawRead(m.as_ptr()),&mut io,&h,memory_permit(a)).unwrap();
    assert_eq!(r.descriptor,DESCRIPTOR);assert_eq!(r.tx,TX);assert_eq!(r.free_before,0x188);assert_eq!(r.free_after,0x41c8);
    assert_eq!(r.written,0x4044);assert_eq!(r.guard_bytes,0xfbc);assert_eq!(r.verification_passes,3);assert_eq!(io.commits,1);
    assert_eq!(smem_item(&m,478),Some(DESCRIPTOR));assert_eq!(smem_item(&m,479),Some(TX));assert_eq!(smem_item(&m,480),None);
    assert!(m[DESCRIPTOR.offset..DESCRIPTOR.offset+32].iter().all(|v|*v==0));assert!(m[TX.offset..TX.offset+0x4000].iter().all(|v|*v==0));
    for i in 0..m.len(){if !(PART+START..PART+END).contains(&i) && !(PART+12..PART+16).contains(&i){assert_eq!(m[i],original[i],"changed {i:x}");}}
}
fn smem_item(m:&[u8],id:u16)->Option<qcom_modem_status::Item>{qcom_modem_status::allocation_directory(&Bytes(m),id).unwrap().item}
#[test]
fn native_five_page_adapter_preserves_neighbor_pages_locks_and_all_other_smem(){
    let(mut m,h,a)=prepared_memory();let original=m.clone();let mut locks=[0x1234,0,0x5678];
    let mut w=unsafe{Window::new(m.as_ptr(),locks.as_mut_ptr().add(1))};
    let mut prep=unsafe{Preparation::new(m.as_mut_ptr().add(PART),m.as_mut_ptr().add(PART+4096))};
    let r=prep.run(&mut w,&h,memory_permit(a)).unwrap();assert_eq!(r.tx.size,16384);assert_eq!(locks,[0x1234,0,0x5678]);
    assert_eq!(&m[..PART],&original[..PART]);assert_eq!(&m[PART+WINDOW..],&original[PART+WINDOW..]);
    let after=m.clone();let mut a=available();let _status=permit(&mut a);
    assert!(matches!(prep.run(&mut w,&h,memory_permit(a)),Err(Error::Outbound)));assert_eq!(m,after);
}
#[test]
fn existing_items_or_peer_publication_stop_before_any_write(){
    for id in [435,478,479,480]{
        let(mut m,h,a)=prepared_memory();let at=PART+START;
        w16(&mut m,at,0xa5a5);w16(&mut m,at+2,id);w32(&mut m,at+4,344);w16(&mut m,at+8,4);w16(&mut m,at+10,0);
        w32(&mut m,PART+12,(START+360)as u32);let original=m.clone();let mut io=Spy::new(&mut m);
        assert!(pixel_glink_prepare::stage(&RawRead(m.as_ptr()),&mut io,&h,memory_permit(a)).is_err());assert!(io.writes.is_empty());assert_eq!(m,original);
    }
}
#[test]
fn ap_tamper_bad_route_and_malformed_allocation_are_terminal_before_writes(){
    for fault in 0..4{
        let(mut m,h,a)=prepared_memory();match fault{0=>m[PART+48+20]^=1,1=>w32(&mut m,SMEM_BYTES-4096+48,16),2=>w16(&mut m,PART+32,0),_=>w32(&mut m,PART+16,0x7b000)};
        let original=m.clone();let mut io=Spy::new(&mut m);
        assert!(pixel_glink_prepare::stage(&RawRead(m.as_ptr()),&mut io,&h,memory_permit(a)).is_err());assert!(io.writes.is_empty());assert_eq!(m,original);
    }
}
#[test]
fn incomplete_data_readback_cannot_publish_the_free_cursor(){
    let(mut m,h,a)=prepared_memory();let original_cursor=m[PART+12..PART+16].to_vec();let mut io=Spy::new(&mut m);io.drop_first=true;
    assert!(matches!(pixel_glink_prepare::stage(&RawRead(m.as_ptr()),&mut io,&h,memory_permit(a)),Err(Error::Publication)));
    assert_eq!(io.commits,0);assert_eq!(&m[PART+12..PART+16],original_cursor);assert_eq!(smem_item(&m,478),None);
}
#[test]
fn guard_corruption_after_commit_is_detected_without_rollback(){
    let(mut m,h,a)=prepared_memory();let mut io=Spy::new(&mut m);io.clobber_guard=true;
    assert!(matches!(pixel_glink_prepare::stage(&RawRead(m.as_ptr()),&mut io,&h,memory_permit(a)),Err(Error::Publication)));
    assert_eq!(io.commits,1);assert_eq!(smem_item(&m,479),Some(TX));
}
#[test]
fn unprepared_handshake_cannot_use_a_staged_memory_permit(){
    let(mut m,_h,a)=prepared_memory();let mut door=[0;4];let h=unsafe{Handshake::new(m.as_mut_ptr().add(PART),door.as_mut_ptr())};let mut io=Spy::new(&mut m);
    assert!(matches!(pixel_glink_prepare::stage(&RawRead(m.as_ptr()),&mut io,&h,memory_permit(a)),Err(Error::Outbound)));assert!(io.writes.is_empty());
}
#[test]
fn native_adapter_is_terminal_after_a_rejected_baseline_even_if_bytes_are_restored(){
    let(mut m,h,a)=prepared_memory();let original=m.clone();m[PART+48+20]^=1;
    let mut lock=0;let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};
    let mut prep=unsafe{Preparation::new(m.as_mut_ptr().add(PART),m.as_mut_ptr().add(PART+4096))};
    assert!(matches!(prep.run(&mut w,&h,memory_permit(a)),Err(Error::Outbound)));assert_eq!(lock,0);
    m.copy_from_slice(&original);let mut a=available();let _status=permit(&mut a);
    assert!(matches!(prep.run(&mut w,&h,memory_permit(a)),Err(Error::Outbound)));assert_eq!(m,original);
}
