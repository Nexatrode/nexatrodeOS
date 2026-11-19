//! Real observer/decoder on host RAM; auth replies/clock are synthetic.
extern crate self as personal_os_rust_server;
#[path="../rust/src/qcom_mdt.rs"] mod qcom_mdt;
#[path="../rust/src/qcom_pas_metadata.rs"] mod qcom_pas_metadata;
#[path="../rust/src/qcom_pas_init.rs"] mod qcom_pas_init;
#[path="../rust/src/qcom_pas_mem_setup.rs"] mod qcom_pas_mem_setup;
#[path="../rust/src/qcom_modem_image.rs"] mod qcom_modem_image;
#[path="../rust/src/qcom_pas_auth.rs"] mod qcom_pas_auth;
#[path="../rust/src/qcom_modem_status.rs"] mod qcom_modem_status;
#[path="../rust/src/pixel_modem_status.rs"] mod pixel_modem_status;
use qcom_modem_status::{directory,decode,Error,Read,SMEM_BYTES,PAYLOAD_BYTES};
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
#[test]
fn batched_directory_matches_individual_lookups_and_rejects_duplicate_items(){
    for cached in [false,true] {
        let mut m=fixture(); publish(&mut m,2,cached);
        let batch=qcom_modem_status::allocation_directories(&Bytes(&m),[435,479,497]).unwrap();
        for (n,id) in [435,479,497].iter().enumerate(){
            assert_eq!(batch[n],qcom_modem_status::allocation_directory(&Bytes(&m),*id).unwrap());
        }
    }
    let mut m=fixture();publish(&mut m,2,false);
    let h=0x10020+16+344;w16(&mut m,h,0xa5a5);w16(&mut m,h+2,435);w32(&mut m,h+4,8);
    w32(&mut m,0x1000c,32+16+344+16+8);
    assert_eq!(qcom_modem_status::allocation_directories(&Bytes(&m),[435,479]),Err(Error::Duplicate));
}
#[test]
fn batched_directory_rejects_the_same_malformed_metadata_and_reads_less(){
    use std::cell::Cell;
    struct Counted<'a>{bytes:&'a[u8], reads:Cell<usize>}
    impl Read for Counted<'_>{fn read8(&self,at:usize)->u8{self.reads.set(self.reads.get()+1);self.bytes[at]}}
    let mut m=fixture();publish(&mut m,2,false);
    let input=Counted{bytes:&m,reads:Cell::new(0)};
    let targets=[435,428,478,479,497];
    let batch=qcom_modem_status::allocation_directories(&input,targets).unwrap();
    let batch_reads=input.reads.replace(0);
    for (index,id) in targets.iter().enumerate(){
        assert_eq!(batch[index],qcom_modem_status::allocation_directory(&input,*id).unwrap());
    }
    eprintln!("batch reads: {batch_reads}; five individual scans: {}", input.reads.get());
    assert!(batch_reads*2<input.reads.get());
    for (at,value) in [(192,0),(92,13<<16),(SMEM_BYTES-4096+8,u32::MAX),
        (SMEM_BYTES-4096+32,u32::MAX),(0x10008,0x1001),(0x1000c,31),(0x10010,0x1001),
        (0x10020,0),(0x10024,u32::MAX)]{
        let mut bad=m.clone();w32(&mut bad,at,value);
        assert_eq!(qcom_modem_status::allocation_directories(&Bytes(&bad),targets).unwrap_err(),
            qcom_modem_status::allocation_directory(&Bytes(&bad),435).unwrap_err(),"at {at:x}");
    }
}
#[test]
fn aligned_metadata_word_reader_matches_byte_reader_and_reduces_accesses(){
    use std::cell::Cell;
    struct WordView<'a>{bytes:&'a[u8], bytes_read:Cell<usize>, words_read:Cell<usize>}
    impl Read for WordView<'_>{
        fn read8(&self,at:usize)->u8{self.bytes_read.set(self.bytes_read.get()+1);self.bytes[at]}
        fn read_le32(&self,at:usize)->u32{
            self.words_read.set(self.words_read.get()+1);
            u32::from_le_bytes(self.bytes[at..at+4].try_into().unwrap())
        }
    }
    let mut original=fixture();publish(&mut original,2,false);
    let targets=[435,428,478,479,497];
    let optimized=WordView{bytes:&original,bytes_read:Cell::new(0),words_read:Cell::new(0)};
    assert_eq!(qcom_modem_status::allocation_directories(&optimized,targets),
        qcom_modem_status::allocation_directories(&Bytes(&original),targets));
    assert!(optimized.words_read.get()>0);
    let operations=optimized.bytes_read.get()+optimized.words_read.get();
    let byte_equivalent=optimized.bytes_read.get()+4*optimized.words_read.get();
    eprintln!("metadata read operations: {operations}; byte reads: {byte_equivalent}");
    assert!(operations<byte_equivalent);
    let table=SMEM_BYTES-4096;
    for cached in [false,true] {
        let mut fixture=fixture();publish(&mut fixture,2,cached);
        let header=if cached{0x10fe0}else{0x10020};
        for at in (92..96).chain(192..208).chain(table..table+96)
            .chain(0x10000..0x10020).chain(header..header+16){
            let mut changed=fixture.clone();changed[at]^=0x80;
            let optimized=WordView{bytes:&changed,bytes_read:Cell::new(0),words_read:Cell::new(0)};
            assert_eq!(qcom_modem_status::allocation_directories(&optimized,targets),
                qcom_modem_status::allocation_directories(&Bytes(&changed),targets),"cached {cached} offset {at:x}");
        }
    }
}
#[test]
fn batched_lookup_matches_independent_parser_across_byte_mutations(){
    let targets=[435,428,478,479,497];
    for cached in [false,true]{
        let mut original=fixture();publish(&mut original,2,cached);
        let table=SMEM_BYTES-4096;
        let header=if cached{0x10fe0}else{0x10020};
        for at in (92..96).chain(192..208).chain(table..table+96)
            .chain(0x10000..0x10020).chain(header..header+16){
            let mut changed=original.clone();changed[at]^=0x80;
            let expected:Result<Vec<_>,_>=targets.iter().map(|id|
                qcom_modem_status::allocation_directory(&Bytes(&changed),*id)).collect();
            let actual=qcom_modem_status::allocation_directories(&Bytes(&changed),targets).map(Vec::from);
            assert_eq!(actual,expected,"cached={cached}, changed offset {at:x}");
        }
    }
}
fn pending_auth()->qcom_pas_auth::AuthOnce {
    let p=qcom_pas_metadata::Prepared{metadata_physical:0x80001000,metadata_length:9100,program_headers:31,loadable_segments:29,linked_start:0x8b000000,linked_end:0x94800000};
    let fresh=qcom_pas_mem_setup::tests::consumed();
    qcom_pas_auth::AuthOnce::after_image(qcom_modem_image::tests::auth_fixture(true),p,&fresh).unwrap()
}
fn auth()->qcom_pas_auth::AuthOnce {
    let mut a=pending_auth();let fresh=qcom_pas_mem_setup::tests::consumed();
    struct Secure;impl qcom_pas_init::Transport for Secure {
        fn invoke(&mut self,r:&qcom_pas_init::Request)->Result<[u64;8],u64>{Ok([0,u64::from(r.words()[0]==0x42000601),0,0,0,0,0,0])}
    }
    a.check(&mut Secure,|_|{}).unwrap();a.authenticate(&mut Secure,&fresh,|_|{}).unwrap();a
}
struct Time{now:u64,step:u64}
impl Clock for Time {fn now(&mut self)->u64{self.now=self.now.wrapping_add(self.step);self.now}fn frequency(&self)->u64{1_000_000}}
#[test]
fn decodes_both_allocation_lists_and_keeps_every_byte_unchanged(){
    for cached in [false,true] {
        let mut m=fixture();assert!(directory(&Bytes(&m)).unwrap().item.is_none());publish(&mut m,6,cached);
        let original=m.clone();let d=directory(&Bytes(&m)).unwrap();let s=qcom_modem_status::payload(&Bytes(&m),d.item.unwrap()).unwrap();
        assert_eq!(s.slave_kernel,Some(6));assert!(s.ready_bit());assert!(!s.fatal_bit());assert_eq!(m,original);
    }
}
#[test]
fn malformed_directory_never_leads_to_a_payload_access(){
    for (at,value) in [(192,0),(92,13<<16),(SMEM_BYTES-4096+8,u32::MAX),
        (SMEM_BYTES-4096+32,u32::MAX),(0x10008,0x1001),(0x1000c,31),(0x10010,0x1001)] {
        let mut m=fixture();w32(&mut m,at,value);assert!(directory(&Bytes(&m)).is_err(),"at {at:x}");
    }
    for cached in [false,true]{
        let mut m=fixture();publish(&mut m,2,cached);
        let h=if cached{0x10fe0}else{0x10020};
        for (off,val) in [(0,0),(4,u32::MAX),(8,344)]{
            let mut bad=m.clone();w32(&mut bad,h+off,val);assert!(directory(&Bytes(&bad)).is_err());
        }
    }
}
#[test]
fn wrong_direction_version_count_and_duplicate_status_names_are_rejected(){
    let good=payload(2);assert!(decode(&good).unwrap().ready_bit());
    for (at,val) in [(0,0),(4,2),(4,0x201),(8,0),(10,1),(12,17),(14,17),(16,4)]{
        let mut bad=good;if at==8||at==10||at==12||at==14{w16(&mut bad,at,val as u16);}else{w32(&mut bad,at,val);}
        assert!(decode(&bad).is_err(),"at {at:x}");
    }
    let mut bad=good;w16(&mut bad,14,2);bad[40..60].copy_from_slice(&good[20..40]);assert_eq!(decode(&bad),Err(Error::Duplicate));
    for value in [1,3,10,130]{let s=decode(&payload(value)).unwrap();assert!(!s.ready_bit());}
}
#[test]
fn slave_kernel_bits_match_atoll_modem_status_not_wlan_indications(){
    let err_fatal=decode(&payload(1)).unwrap();
    assert!(err_fatal.fatal_bit());
    assert!(!err_fatal.ready_bit());

    let err_ready=decode(&payload(2)).unwrap();
    assert!(!err_ready.fatal_bit());
    assert!(err_ready.ready_bit());

    // Bit 2 is modem proxy-unvote; it is not the err-fatal indication.
    let proxy_unvote=decode(&payload(4)).unwrap();
    assert!(!proxy_unvote.fatal_bit());
    assert!(!proxy_unvote.ready_bit());
}
#[test]
fn passive_observer_requires_absence_before_auth_and_preserves_smem_and_lock_neighbors(){
    for cached in [false,true]{
        let mut m=fixture();let mut locks=[0x12345678,0,0xa5a5a5a5];
        let mut w=unsafe{Window::new(m.as_ptr(),locks.as_mut_ptr().add(1))};let baseline=w.before_auth().unwrap();
        publish(&mut m,6,cached);let original=m.clone();
        let r=w.observe(baseline,auth(),&mut Time{now:0,step:1000},|_|{});
        assert!(r.ready_bit_seen);assert_eq!(r.error,None);assert_eq!(r.samples,1);assert_eq!(m,original);assert_eq!(locks,[0x12345678,0,0xa5a5a5a5]);
        assert!(matches!(w.before_auth(),Err(Error::BaselinePresent)));
    }
}
#[test]
fn missing_status_times_out_and_fatal_status_never_becomes_ready(){
    assert_eq!(Error::Fatal.code(), 0x0e); // Local parser diagnostic, not the peer status word.
    for value in [None,Some(0),Some(1),Some(10),Some(130)]{
        let mut m=fixture();let mut lock=0;let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let b=w.before_auth().unwrap();
        if let Some(v)=value{publish(&mut m,v,false);}
        let original=m.clone();let r=w.observe(b,auth(),&mut Time{now:0,step:50000},|_|{});
        assert!(!r.ready_bit_seen);assert_eq!(r.error,Some(if value==Some(1){Error::Fatal}else{Error::Timeout}));
        assert!(r.samples<=1024);assert_eq!(lock,0);assert_eq!(m,original);
    }
}
#[test]
fn frozen_clock_and_changed_routes_stop_without_a_false_ready_result(){
    let mut m=fixture();let mut lock=0;let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let b=w.before_auth().unwrap();
    let r=w.observe(b,auth(),&mut Time{now:0,step:0},|_|{});assert_eq!(r.error,Some(Error::Clock));assert!(!r.ready_bit_seen);
    let b=w.before_auth().unwrap();w32(&mut m,SMEM_BYTES-4096+32,0x12000);
    let part=m[0x10000..0x11000].to_vec();m[0x12000..0x13000].copy_from_slice(&part);
    let r=w.observe(b,auth(),&mut Time{now:0,step:1000},|_|{});assert_eq!(r.error,Some(Error::RouteChanged));assert_eq!(lock,0);
}
#[test]
fn changing_payload_is_detected_without_repairs(){
    use std::cell::Cell;
    struct Changing {reads:Cell<usize>,data:[u8;PAYLOAD_BYTES]}
    impl Read for Changing {fn read8(&self,at:usize)->u8{let n=self.reads.get();self.reads.set(n+1);self.data[at]^u8::from(n==PAYLOAD_BYTES+36)}}
    let m=Changing{reads:Cell::new(0),data:payload(2)};
    assert_eq!(qcom_modem_status::payload(&m,qcom_modem_status::Item{offset:0,size:340}),Err(Error::Unstable));assert_eq!(m.reads.get(),680);
}
#[test]
fn baseline_must_remain_absent_and_unchanged_until_auth(){
    let mut m=fixture();let mut lock=0;let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};
    let b=w.before_auth().unwrap();assert_eq!(w.revalidate(&b),Ok(()));
    publish(&mut m,2,false);assert_eq!(w.revalidate(&b),Err(Error::BaselinePresent));assert_eq!(lock,0);
    m=fixture();let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let b=w.before_auth().unwrap();
    // A valid unrelated allocation still changes the pre-auth epoch baseline.
    publish(&mut m,2,false);w16(&mut m,0x10022,999);
    assert_eq!(w.revalidate(&b),Err(Error::RouteChanged));assert_eq!(lock,0);
}
#[test]
fn unreturned_auth_cannot_observe_even_a_valid_ready_item(){
    let mut m=fixture();let mut lock=0;let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let b=w.before_auth().unwrap();
    publish(&mut m,2,false);let original=m.clone();
    let r=w.observe(b,pending_auth(),&mut Time{now:0,step:1000},|_|panic!("no sampling before auth"));
    assert_eq!(r.error,Some(Error::Auth));assert_eq!(r.samples,0);assert!(!r.auth_passed);assert!(!r.ready_bit_seen);
    assert_eq!(m,original);assert_eq!(lock,0);
}
#[test]
fn duplicate_routes_items_and_short_targets_are_rejected(){
    let mut m=fixture();let t=SMEM_BYTES-4096;w32(&mut m,t+8,2);
    let route=m[t+32..t+80].to_vec();m[t+80..t+128].copy_from_slice(&route);
    assert_eq!(directory(&Bytes(&m)),Err(Error::Route));
    m=fixture();publish(&mut m,2,false);publish(&mut m,2,true);
    assert_eq!(directory(&Bytes(&m)),Err(Error::Duplicate));
    m=fixture();publish(&mut m,2,false);w16(&mut m,0x10028,8);
    assert_eq!(directory(&Bytes(&m)),Err(Error::ItemSize));
}
