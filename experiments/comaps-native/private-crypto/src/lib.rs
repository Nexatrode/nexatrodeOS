// SPDX-License-Identifier: Apache-2.0
//! Allocation-free Maps vault; a session profile accepts only its derived app key.
#![cfg_attr(target_os="none", no_std)]
#[path="../../../../rust/src/record_crypto.rs"] mod record_crypto;
#[path="../../../../rust/src/vault_key_prototype.rs"] mod vault_key_prototype;
use zeroize::{Zeroize,Zeroizing};
const BANK:usize=4096;const PAYLOAD:usize=2048;const HEADER:usize=128;
const MAGIC:&[u8;8]=b"NXMPS001";
const SESSION_MAGIC:&[u8;8]=b"NXMPS002";
const ENV:usize=vault_key_prototype::ENVELOPE_SIZE;
struct Session { key:[u8;32], envelope:[u8;ENV], open:bool, shared:bool }
static mut SESSION:Session=Session{key:[0;32],envelope:[0;ENV],open:false,shared:false};
#[cfg(all(not(test),target_os="none"))]
#[panic_handler] fn panic(_: &core::panic::PanicInfo)->! {loop {core::hint::spin_loop();}}
fn generation(bank:&[u8])->Option<u64>{
    if bank.len()!=BANK||bank[28..32]!=[0;4]||bank[124..128]!=[0;4]{return None;}
    if &bank[..8]==MAGIC {if !vault_key_prototype::supported_envelope(&bank[32..32+ENV]){return None;}}
    else if &bank[..8]!=SESSION_MAGIC||bank[32..124].iter().any(|b|*b!=0){return None;}
    let g=u64::from_le_bytes(bank[8..16].try_into().ok()?);if g==0||g==u64::MAX{None}else{Some(g)}
}
fn seal(key:&[u8;32],envelope:&[u8;ENV],payload:&[u8;PAYLOAD],nonce:&[u8;12],generation_number:u64,out:&mut[u8;BANK])->bool{
    seal_mode(key,envelope,payload,nonce,generation_number,out,MAGIC)
}
fn seal_mode(key:&[u8;32],envelope:&[u8;ENV],payload:&[u8;PAYLOAD],nonce:&[u8;12],generation_number:u64,out:&mut[u8;BANK],magic:&[u8;8])->bool{
    if generation_number==0||generation_number==u64::MAX{return false;}
    out.fill(0);out[..8].copy_from_slice(magic);out[8..16].copy_from_slice(&generation_number.to_le_bytes());out[16..28].copy_from_slice(nonce);out[32..32+ENV].copy_from_slice(envelope);
    out[HEADER..HEADER+PAYLOAD].copy_from_slice(payload);
    let (header,body)=out.split_at_mut(HEADER);
    let Some(tag)=record_crypto::encrypt(key,nonce,header,&mut body[..PAYLOAD]) else {out.zeroize();return false;};
    body[PAYLOAD..PAYLOAD+16].copy_from_slice(&tag);true
}
fn open(password:&[u8],bank:&[u8],payload:&mut[u8;PAYLOAD])->Option<Zeroizing<[u8;32]>>{
    generation(bank)?;
    let key=vault_key_prototype::open(password,&bank[32..32+ENV])?;
    let mut clear=Zeroizing::new([0;PAYLOAD]);clear.copy_from_slice(&bank[HEADER..HEADER+PAYLOAD]);
    if !record_crypto::decrypt(&key,bank[16..28].try_into().ok()?,&bank[..HEADER],&mut *clear,bank[HEADER+PAYLOAD..HEADER+PAYLOAD+16].try_into().ok()?){return None;}
    payload.copy_from_slice(&*clear);Some(key)
}
// All FFI calls are serialized by the single-threaded Maps PD. Pointers refer
// to fixed-size, nonoverlapping private buffers; never caller-chosen IPC pointers.
#[unsafe(no_mangle)] pub unsafe extern "C" fn mp_lock(){unsafe {let p=&mut *core::ptr::addr_of_mut!(SESSION);p.key.zeroize();p.envelope.zeroize();p.open=false;p.shared=false;}}
#[unsafe(no_mangle)] pub unsafe extern "C" fn mp_opened()->u32 {unsafe {(*core::ptr::addr_of!(SESSION)).open as u32}}
#[unsafe(no_mangle)] pub unsafe extern "C" fn mp_cleared()->u32 {unsafe {let s=&*core::ptr::addr_of!(SESSION);(!s.open&&s.key.iter().all(|x|*x==0)&&s.envelope.iter().all(|x|*x==0)) as u32}}
#[unsafe(no_mangle)] pub unsafe extern "C" fn mp_status(banks:*const u8)->u32{
    let b=unsafe {core::slice::from_raw_parts(banks,2*BANK)};
    if b.iter().all(|x|*x==0){0}else if generation(&b[..BANK]).is_some()||generation(&b[BANK..]).is_some(){1}else{2}
}
#[unsafe(no_mangle)] pub unsafe extern "C" fn mp_enroll(password:*const u8,len:usize,entropy:*const u8,payload:*const u8,out:*mut u8)->u32{
    unsafe {mp_lock();}
    if !(8..=128).contains(&len){return 0;}
    let pass=unsafe {core::slice::from_raw_parts(password,len)};let e=unsafe {&*(entropy as *const[u8;60])};
    if e[..32].iter().all(|b|*b==0)||e[32..].iter().all(|b|*b==0){return 0;}
    let key=Zeroizing::new(<[u8;32]>::try_from(&e[..32]).unwrap());
    let Some(envelope)=vault_key_prototype::seal(pass,e[32..].try_into().unwrap(),&key) else{return 0;};
    let ok=seal(&key,&envelope,unsafe {&*(payload as *const[u8;PAYLOAD])},e[48..60].try_into().unwrap(),1,unsafe {&mut *(out as *mut[u8;BANK])});
    if ok {unsafe {let s=&mut *core::ptr::addr_of_mut!(SESSION);s.key.copy_from_slice(&*key);s.envelope=envelope;s.open=true;}}ok as u32
}
#[unsafe(no_mangle)] pub unsafe extern "C" fn mp_unlock(password:*const u8,len:usize,banks:*const u8,payload:*mut u8,generation_number:*mut u64,bank:*mut u32)->u32{
    unsafe {mp_lock();}
    if !(8..=128).contains(&len){return 0;}
    let b=unsafe {core::slice::from_raw_parts(banks,2*BANK)};let pass=unsafe {core::slice::from_raw_parts(password,len)};
    let a=generation(&b[..BANK]).unwrap_or(0);let z=generation(&b[BANK..]).unwrap_or(0);
    let first=if z>a{1}else{0};
    for i in [first,1-first]{
        let rec=&b[i*BANK..(i+1)*BANK];let mut clear=Zeroizing::new([0;PAYLOAD]);
        if let Some(key)=open(pass,rec,&mut clear){unsafe {
            let s=&mut *core::ptr::addr_of_mut!(SESSION);s.key.copy_from_slice(&*key);s.envelope.copy_from_slice(&rec[32..32+ENV]);s.open=true;
            core::ptr::copy_nonoverlapping(clear.as_ptr(),payload,PAYLOAD);*generation_number=generation(rec).unwrap();*bank=i as u32;
        }return 1;}
    }0
}
#[unsafe(no_mangle)] pub unsafe extern "C" fn mp_seal(payload:*const u8,nonce:*const u8,generation_number:u64,out:*mut u8)->u32{
    let s=unsafe {&*core::ptr::addr_of!(SESSION)};if !s.open{return 0;}
    seal_mode(&s.key,&s.envelope,unsafe{&*(payload as *const[u8;PAYLOAD])},unsafe{&*(nonce as *const[u8;12])},generation_number,unsafe{&mut *(out as *mut[u8;BANK])},if s.shared {SESSION_MAGIC}else{MAGIC}) as u32
}
/// Returns 2 for a wholly blank store, 1 for authenticated recovery, 0 for
/// damage/wrong key/legacy format. Never overwrites an existing legacy bank.
#[unsafe(no_mangle)] pub unsafe extern "C" fn mp_session_open(key:*const u8,banks:*const u8,payload:*mut u8,generation_number:*mut u64,bank:*mut u32)->u32{
    unsafe {mp_lock();}
    let key=unsafe {&*(key as *const[u8;32])};let b=unsafe {core::slice::from_raw_parts(banks,2*BANK)};
    if key.iter().all(|x|*x==0){return 0;}
    let mut clear=Zeroizing::new([0;PAYLOAD]);let mut found=None;
    let a=generation(&b[..BANK]).unwrap_or(0);let z=generation(&b[BANK..]).unwrap_or(0);let first=if z>a{1}else{0};
    for i in [first,1-first]{
        let rec=&b[i*BANK..(i+1)*BANK];if generation(rec).is_none()||&rec[..8]!=SESSION_MAGIC{continue;}
        clear.copy_from_slice(&rec[HEADER..HEADER+PAYLOAD]);
        if record_crypto::decrypt(key,rec[16..28].try_into().unwrap(),&rec[..HEADER],&mut *clear,rec[HEADER+PAYLOAD..HEADER+PAYLOAD+16].try_into().unwrap()){found=Some((generation(rec).unwrap(),i));break;}
    }
    let blank=b.iter().all(|x|*x==0);if found.is_none()&&!blank{return 0;}
    unsafe {let s=&mut *core::ptr::addr_of_mut!(SESSION);s.key.copy_from_slice(key);s.open=true;s.shared=true;
        let (g,i)=found.unwrap_or((0,1));*generation_number=g;*bank=i as u32;core::ptr::copy_nonoverlapping(clear.as_ptr(),payload,PAYLOAD);}
    if blank {2}else{1}
}
#[cfg(test)] mod tests{
 use super::*;
 #[test] fn encrypted_banks_authenticate_and_recover(){
  let password=b"disposable maps password";let key=[7;32];let env=vault_key_prototype::seal(password,&[9;28],&key).unwrap();
  let mut payload=[0;PAYLOAD];payload[..12].copy_from_slice(b"PrivateCafe!");let mut old=[0;BANK];assert!(seal(&key,&env,&payload,&[8;12],4,&mut old));
  assert!(!old.windows(12).any(|w|w==b"PrivateCafe!"));let mut clear=[0;PAYLOAD];assert!(open(password,&old,&mut clear).is_some());assert_eq!(clear,payload);
  assert!(open(b"incorrect password",&old,&mut clear).is_none());
  for offset in [0,8,16,32,128,HEADER+PAYLOAD]{let mut bad=old;bad[offset]^=1;assert!(open(password,&bad,&mut clear).is_none());}
  let mut new=[0;BANK];assert!(seal(&key,&env,&payload,&[4;12],5,&mut new));new[HEADER]^=1;assert!(open(password,&new,&mut clear).is_none());assert!(open(password,&old,&mut clear).is_some());
  unsafe {let mut banks=[0;2*BANK];banks[..BANK].copy_from_slice(&old);banks[BANK..].copy_from_slice(&new);let mut g=0;let mut b=9;
   assert_eq!(mp_unlock(password.as_ptr(),password.len(),banks.as_ptr(),clear.as_mut_ptr(),&mut g,&mut b),1);assert_eq!((g,b),(4,0));assert_eq!(mp_opened(),1);mp_lock();assert_eq!(mp_opened(),0);
   let s=&*core::ptr::addr_of!(SESSION);assert!(s.key.iter().all(|x|*x==0));assert!(s.envelope.iter().all(|x|*x==0));
   assert_eq!(mp_seal(payload.as_ptr(),[1;12].as_ptr(),5,new.as_mut_ptr()),0);
   let empty=[0;2*BANK];assert_eq!(mp_session_open(key.as_ptr(),empty.as_ptr(),clear.as_mut_ptr(),&mut g,&mut b),2);
   assert_eq!(mp_seal(payload.as_ptr(),[2;12].as_ptr(),1,old.as_mut_ptr()),1);assert_eq!(&old[..8],SESSION_MAGIC);
   let mut banks=[0;2*BANK];banks[..BANK].copy_from_slice(&old);assert_eq!(mp_seal(payload.as_ptr(),[3;12].as_ptr(),2,new.as_mut_ptr()),1);new[HEADER]^=1;banks[BANK..].copy_from_slice(&new);
   assert_eq!(mp_session_open(key.as_ptr(),banks.as_ptr(),clear.as_mut_ptr(),&mut g,&mut b),1);assert_eq!((g,b),(1,0));assert_eq!(clear,payload);
   assert_eq!(mp_session_open([99;32].as_ptr(),banks.as_ptr(),clear.as_mut_ptr(),&mut g,&mut b),0);assert_eq!(mp_cleared(),1);
   assert_eq!(mp_unlock(password.as_ptr(),password.len(),banks.as_ptr(),clear.as_mut_ptr(),&mut g,&mut b),0);
   mp_lock();assert_eq!(mp_cleared(),1);
  }
 }
}
