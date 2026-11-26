//! Read-only view of the trusted desktop manager's bounded display mode.
use core::{ptr, sync::atomic::{AtomicU32,Ordering,fence}};
#[repr(C)] pub struct DisplayMode {pub revision:AtomicU32,pub serial:u32,pub width:u32,pub height:u32,pub scale:u32,pub app_height:u32}
#[derive(Clone,Copy)] pub struct Mode {pub width:u32,pub height:u32,pub app_height:u32}
pub fn read() -> Option<Mode> {
    let p=sel4_microkit::memory_region_symbol!(desktop_display_mode:*mut DisplayMode).as_ptr();
    let r=unsafe {&*p};let revision=r.revision.load(Ordering::Acquire);
    if revision==0 || revision&1!=0 {return None;}
    let mode=Mode{width:unsafe{ptr::read_volatile(&r.width)},height:unsafe{ptr::read_volatile(&r.height)},app_height:unsafe{ptr::read_volatile(&r.app_height)}};
    let scale=unsafe{ptr::read_volatile(&r.scale)};fence(Ordering::Acquire);
    if revision!=r.revision.load(Ordering::Acquire)||scale!=2||mode.width<1600||mode.width>3840||mode.height<1200||mode.height>2160||mode.width%2!=0||mode.height%2!=0||mode.app_height<360||mode.app_height>520 {return None;}
    Some(mode)
}
