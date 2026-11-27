//! Dedicated trusted authenticator for the disposable dynamic-process experiment.
#![no_std]
#![no_main]
extern crate alloc;
use alloc::{format, rc::Rc};
use core::{cell::RefCell, ptr, sync::atomic::{AtomicU32, Ordering, fence}};
use personal_os_slint_notes_spike::{LockWindow, guest_platform};
use personal_os_rust_server::{device_lock::{Credential, LockSettings, LockController, UnlockResult}, vault_blob_transfer::BlobTransferPage};
use sel4_microkit::{Channel, ChannelSet, Handler, Infallible, MessageInfo, debug_println, get_mr, set_mr, protection_domain};
use slint::{ComponentHandle, LogicalPosition, PhysicalSize, platform::{PointerEventButton, WindowEvent, software_renderer::{MinimalSoftwareWindow, RepaintBufferType}}};
use zeroize::Zeroize;
#[cfg(feature = "desktop-wrapped-lock-ui")]
use personal_os_rust_server::{vault_key_prototype as wrapping, wrapped_vault_bootstrap as bootstrap};
#[cfg(feature = "desktop-wrapped-lock-ui")]
use zeroize::Zeroizing;
#[cfg(feature = "desktop-wrapped-lock-ui")]
const RECORD_SIZE: usize = bootstrap::BOOTSTRAP_SIZE;
#[cfg(not(feature = "desktop-wrapped-lock-ui"))]
const RECORD_SIZE: usize = personal_os_rust_server::device_lock::SETTINGS_SIZE;
const ROOT: Channel = Channel::new(0);
const VAULT: Channel = Channel::new(4);
const ENTROPY: Channel = Channel::new(3);
const WIDTH: usize = if cfg!(feature="desktop-trio-lock-auth") {2560} else {1080};
const HEIGHT: usize = if cfg!(feature="desktop-trio-lock-auth") {1600} else {2340};
const SCALE:f32=if cfg!(feature="desktop-trio-lock-auth") {2.} else {3.};
type HeapMutex = sel4_microkit::_private::heap::RawOneShotMutex;
#[global_allocator]
static HEAP: sel4_dlmalloc::DeferredStaticDlmalloc<HeapMutex> = sel4_dlmalloc::DeferredStaticDlmalloc::new();
#[repr(C)] struct Header { revision: AtomicU32, width: u32, height: u32, stride: u32, epoch: u64, locked: u32, reserved: u32 }
#[repr(C)] #[derive(Clone, Copy)] struct Event { kind: u32, code: u32, value: u32, x: i32, y: i32 }
#[repr(C)] struct Queue { head: AtomicU32, tail: AtomicU32, reserved: [u32; 2], events: [Event; 64] }
const _: () = assert!(core::mem::size_of::<Header>() == 32);
struct Form { input: [u8; 128], length: usize, submit: bool, reveal: bool, lock: bool }
impl Form {
    fn new() -> Self { Self { input: [0; 128], length: 0, submit: false, reveal: false, lock: false } }
    fn clear(&mut self) { self.input.zeroize(); self.length = 0; }
    fn key(&mut self, key: &str) {
        match key {
            "CLEAR" => self.clear(),
            "DELETE" => { if self.length > 0 { self.length -= 1; self.input[self.length] = 0; } }
            "SPACE" if self.length < 128 => { self.input[self.length] = b' '; self.length += 1; }
            _ if key.len() == 1 && key.as_bytes()[0].is_ascii_graphic() && self.length < 128 => {
                self.input[self.length] = key.as_bytes()[0]; self.length += 1;
            }
            _ => ()
        }
    }
}
impl Drop for Form { fn drop(&mut self) { self.clear(); } }
fn now() -> u64 {
    let t = guest_platform::counter_ticks(); let f = u64::from(guest_platform::counter_frequency());
    t / f * 1000 + t % f * 1000 / f
}
fn page() -> &'static mut BlobTransferPage { unsafe { &mut *(0x68000000 as *mut BlobTransferPage) } }
fn read() -> Result<(u64, Option<[u8; RECORD_SIZE]>), ()> {
    let reply = VAULT.pp_call(MessageInfo::new(0x450, 0));
    if reply.count() != 1 { return Err(()); }
    let generation = get_mr(0);
    if reply.label() == 0x30d { return Ok((generation, None)); }
    if reply.label() != 0x300 { return Err(()); }
    let payload = page().payload().ok_or(())?;
    let result = payload.try_into().map_err(|_| ());
    page().set_payload(&[]);
    result.map(|data| (generation, Some(data)))
}
fn save(data: &[u8; RECORD_SIZE]) -> Result<(), ()> {
    for _ in 0..2 {
        let (generation, _) = read()?;
        if !page().set_payload(data) { return Err(()); }
        set_mr(0, generation);
        let reply = VAULT.pp_call(MessageInfo::new(0x451, 1));
        page().set_payload(&[]);
        if reply.label() != 0x300 || reply.count() != 1 { return Err(()); }
        let (after, contents) = read()?;
        if after <= generation || contents.as_ref() != Some(data) { return Err(()); }
    }
    Ok(())
}
fn salt() -> Result<[u8; 16], ()> {
    let reply = ENTROPY.pp_call(MessageInfo::new(0x401, 0));
    if reply.label() != 0x400 || reply.count() != 2 { return Err(()); }
    let mut result = [0; 16];
    result[..8].copy_from_slice(&get_mr(0).to_le_bytes()); result[8..].copy_from_slice(&get_mr(1).to_le_bytes());
    Ok(result)
}
#[cfg(feature = "desktop-wrapped-lock-ui")]
fn wrap_new_key(password: &[u8]) -> Option<([u8; wrapping::ENVELOPE_SIZE], [u8; 32])> {
    let mut key = Zeroizing::new([0u8; 32]);
    key[..16].copy_from_slice(&salt().ok()?); key[16..].copy_from_slice(&salt().ok()?);
    let mut entropy = [0; 28];
    entropy[..16].copy_from_slice(&salt().ok()?); entropy[16..].copy_from_slice(&salt().ok()?[..12]);
    if key.iter().all(|b| *b == 0) { return None; }
    let envelope = wrapping::seal(password, &entropy, &key)?;
    Some((envelope, wrapping::key_hash(&key)))
}
struct Auth {
    window: Rc<MinimalSoftwareWindow>, ui: LockWindow, form: Rc<RefCell<Form>>,
    controller: Option<LockController>, pending: Option<Credential>, epoch: u64, idle: u64,
    stage: u8, locked: bool, welcome: bool, shift: bool, pointer: Option<LogicalPosition>,
    #[cfg(feature = "desktop-wrapped-lock-ui")]
    pending_wrapped: Option<([u8; wrapping::ENVELOPE_SIZE], [u8; 32])>,
    #[cfg(feature = "desktop-wrapped-lock-ui")]
    wrapped: Option<[u8; wrapping::ENVELOPE_SIZE]>,
    #[cfg(feature = "desktop-wrapped-lock-ui")]
    abandon_next: bool,
}
impl Auth {
    fn sync(&mut self) {
        let reply = ROOT.pp_call(MessageInfo::new(0x792, 0));
        assert_eq!(reply.label(), 0);
        assert_eq!(reply.count(), if cfg!(feature = "desktop-wrapped-lock-ui") { 4 } else { 3 });
        let locked = get_mr(0) != 0; let epoch = get_mr(1); self.idle = get_mr(2);
        if epoch != self.epoch {
            self.window.request_redraw();
            self.form.borrow_mut().clear(); self.shift = false;
            if let Some(position) = self.pointer.take() {
                self.window.dispatch_event(WindowEvent::PointerReleased { position, button: PointerEventButton::Left });
            }
            self.form.borrow_mut().submit = false;
            if locked && self.controller.is_some() {
                self.controller.as_mut().unwrap().lock(); self.stage = 1; self.welcome = true;
                self.ui.set_status("".into());
            }
        }
        self.epoch = epoch; self.locked = locked;
        if !locked { self.stage = 0; self.welcome = false; }
        #[cfg(feature = "desktop-wrapped-lock-ui")]
        if get_mr(3) != 0 {
            self.form.borrow_mut().clear(); self.stage = 4; self.welcome = false;
            self.ui.set_status("Vault lock failed. Restart required.".into());
        }
    }
    fn draw(&mut self) {
        self.ui.set_stage(i32::from(self.stage)); self.ui.set_welcome(self.welcome);
        self.ui.set_feedback(if self.stage == 2 || self.stage == 3 {
            format!("{} characters entered", self.form.borrow().length).into()
        } else { "".into() });
        slint::platform::update_timers_and_animations();
        let h = unsafe { &mut *(0x60000000 as *mut Header) };
        let before = h.revision.load(Ordering::Relaxed);
        // Publish only when Slint changed. The epoch is always attached to that frame.
        let mut drew = false;
        self.window.draw_if_needed(|renderer| {
            h.revision.store(before.wrapping_add(1), Ordering::Release); fence(Ordering::SeqCst);
            let pixels = unsafe { core::slice::from_raw_parts_mut((h as *mut Header).add(1).cast::<slint::Rgb8Pixel>(), WIDTH * HEIGHT) };
            renderer.render(pixels, WIDTH); drew = true;
        });
        if drew {
            h.width = WIDTH as u32; h.height = HEIGHT as u32; h.stride = (WIDTH * 3) as u32;
            h.epoch = self.epoch; h.locked = u32::from(self.locked); h.reserved = 0;
            h.revision.store(before.wrapping_add(2), Ordering::Release);
        }
        Channel::new(1).notify();
    }
    fn submit(&mut self) {
        if self.welcome { self.welcome = false; return; }
        if self.stage == 2 {
            let f = self.form.borrow();
            self.pending = salt().ok().and_then(|salt| Credential::create(&f.input[..f.length], salt).ok());
            #[cfg(feature = "desktop-wrapped-lock-ui")]
            {
                self.pending_wrapped = if self.pending.is_some() { wrap_new_key(&f.input[..f.length]) } else { None };
                if self.pending_wrapped.is_none() { self.pending = None; }
            }
            drop(f);
            if self.pending.is_some() { self.stage = 3; self.ui.set_status("Enter the same passphrase again.".into()); }
            else { self.ui.set_status("Use at least 8 characters.".into()); }
        } else if self.stage == 3 {
            let f = self.form.borrow();
            let valid = self.pending.as_ref().is_some_and(|c| c.verify(&f.input[..f.length])); drop(f);
            if valid {
                let settings = LockSettings::enabled(self.pending.take().unwrap(), self.idle).unwrap();
                #[cfg(not(feature = "desktop-wrapped-lock-ui"))]
                let record = settings.encode();
                #[cfg(feature = "desktop-wrapped-lock-ui")]
                let record = {
                    let (envelope, hash) = self.pending_wrapped.take().unwrap();
                    self.wrapped = Some(envelope);
                    bootstrap::encode(&settings.encode(), &envelope, &hash).unwrap()
                };
                if save(&record).is_ok() {
                    self.controller = settings.into_controller(now()).unwrap(); self.stage = 1; self.welcome = true;
                    self.ui.set_status("Passphrase saved. Unlock to start an app.".into());
                    debug_println!("LOCK_AUTH|ENROLLED");
                } else { self.stage = 4; self.ui.set_status("Storage failed. Device stays locked.".into()); }
            } else { self.ui.set_status("Confirmation did not match.".into()); }
        } else if self.stage == 1 {
            debug_println!("LOCK_AUTH|VERIFY_BEGIN");
            let f = self.form.borrow();
            let result = self.controller.as_mut().unwrap().try_unlock(&f.input[..f.length], now()); drop(f);
            self.controller.as_mut().unwrap().authentication_finished(now(), result);
            match result {
                UnlockResult::Unlocked => {
                    if self.unlock_root() {
                        self.stage = 0; self.locked = false; debug_println!("LOCK_AUTH|UNLOCKED");
                    } else {
                        self.controller.as_mut().unwrap().lock(); debug_println!("LOCK_AUTH|STALE_UNLOCK_DENIED");
                    }
                }
                UnlockResult::Denied => { self.ui.set_status("Unable to unlock. Try again shortly.".into()); debug_println!("LOCK_AUTH|DENIED"); }
                UnlockResult::Wait => { self.ui.set_status("Please wait before trying again.".into()); debug_println!("LOCK_AUTH|WAIT"); }
            }
        }
        self.form.borrow_mut().clear();
        debug_println!("LOCK_AUTH|INPUT_CLEARED");
    }
    fn unlock_root(&mut self) -> bool {
        #[cfg(feature = "desktop-wrapped-lock-ui")]
        {
            let key = {
                let f = self.form.borrow();
                self.wrapped.as_ref().and_then(|envelope| wrapping::open(&f.input[..f.length], envelope))
            };
            let Some(key) = key else { return false; };
            // Derivation happens before the bounded lease starts. Only the
            // manager can arm this epoch or commit the prepared key.
            set_mr(0, self.epoch);
            if ROOT.pp_call(MessageInfo::new(0x793, 1)).label() != 0 { return false; }
            page().set_payload(&*key);
            set_mr(0, self.epoch);
            let prepared = VAULT.pp_call(MessageInfo::new(0x452, 1));
            page().set_payload(&[]);
            drop(key);
            if prepared.label() != 0x300 {
                set_mr(0, self.epoch); ROOT.pp_call(MessageInfo::new(0x794, 1)); return false;
            }
            // Fixture-only fault injection: abandon a preparation with F8.
            // The manager must independently expire and erase it.
            if core::mem::take(&mut self.abandon_next) {
                debug_println!("LOCK_AUTH|PREPARE_ABANDONED");
                return false;
            }
        }
        set_mr(0, self.epoch);
        let success = ROOT.pp_call(MessageInfo::new(0x791, 1)).label() == 0;
        #[cfg(feature = "desktop-wrapped-lock-ui")]
        if !success { set_mr(0, self.epoch); ROOT.pp_call(MessageInfo::new(0x794, 1)); }
        success
    }
    fn actions(&mut self) {
        let (submit, reveal, lock) = { let mut f = self.form.borrow_mut();
            let a = (f.submit, f.reveal, f.lock); f.submit = false; f.reveal = false; f.lock = false; a };
        if reveal { self.welcome = false; }
        if lock { ROOT.pp_call(MessageInfo::new(0x790, 0)); self.sync(); }
        if submit { self.submit(); }
    }
    fn input(&mut self) {
        let queue = unsafe { &mut *(0x66000000 as *mut Queue) };
        let head = queue.head.load(Ordering::Acquire); let mut tail = queue.tail.load(Ordering::Acquire);
        if head.wrapping_sub(tail) > 64 { self.form.borrow_mut().clear(); tail = head; }
        while tail != head {
            let slot = &mut queue.events[tail as usize % 64];
            let event = unsafe { ptr::read_volatile(slot) };
            unsafe { ptr::write_volatile(slot, Event { kind: 0, code: 0, value: 0, x: 0, y: 0 }) };
            tail = tail.wrapping_add(1);
            match event.kind {
                1 if event.value <= 2 => {
                    #[cfg(feature = "desktop-wrapped-lock-ui")]
                    if event.code == 66 && event.value == 1 && self.locked && !cfg!(feature="desktop-trio-lock-auth") { self.abandon_next = true; }
                    if event.code == 42 || event.code == 54 { self.shift = event.value != 0; }
                    else if event.value != 0 && self.locked {
                        if event.code == 28 { self.form.borrow_mut().submit = true; }
                        else if !self.welcome {
                            let letter = match event.code {
                                2..=10 => Some(b'1' + (event.code - 2) as u8), 11 => Some(b'0'),
                                16..=25 => Some(b"qwertyuiop"[(event.code - 16) as usize]),
                                30..=38 => Some(b"asdfghjkl"[(event.code - 30) as usize]),
                                44..=50 => Some(b"zxcvbnm"[(event.code - 44) as usize]), _ => None };
                            if let Some(mut letter) = letter {
                                if self.shift { letter = letter.to_ascii_uppercase(); }
                                self.form.borrow_mut().key(core::str::from_utf8(&[letter]).unwrap());
                            } else if event.code == 14 { self.form.borrow_mut().key("DELETE"); }
                            else if event.code == 57 { self.form.borrow_mut().key("SPACE"); }
                            else if event.code == 1 { self.form.borrow_mut().clear(); self.welcome = true; }
                        }
                    }
                }
                2 if event.value <= 2 => {
                    let position = LogicalPosition::new(event.x as f32 / SCALE, event.y as f32 / SCALE);
                    self.window.dispatch_event(match event.value {
                        0 => { self.pointer = None; WindowEvent::PointerReleased { position, button: PointerEventButton::Left } }
                        1 => { self.pointer = Some(position); WindowEvent::PointerPressed { position, button: PointerEventButton::Left } }
                        _ => WindowEvent::PointerMoved { position }
                    });
                }
                4 => { self.shift = false; self.form.borrow_mut().clear(); }
                _ => ()
            }
            self.actions();
        }
        queue.tail.store(tail, Ordering::Release);
    }
}
#[protection_domain]
fn init() -> Auth {
    HEAP.set_bounds(sel4_dlmalloc::StaticHeapBounds::new(0x64000000 as *mut u8, 0x2000000)).unwrap();
    let window = MinimalSoftwareWindow::new(RepaintBufferType::ReusedBuffer);
    guest_platform::install(&window);
    let ui = LockWindow::new().unwrap(); ui.show().unwrap();
    window.dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor: SCALE });
    window.set_size(PhysicalSize::new(WIDTH as u32, HEIGHT as u32));
    let form = Rc::new(RefCell::new(Form::new()));
    let f = form.clone(); ui.on_key(move |key| f.borrow_mut().key(key.as_str()));
    let f = form.clone(); ui.on_submit(move || f.borrow_mut().submit = true);
    let f = form.clone(); ui.on_reveal(move || f.borrow_mut().reveal = true);
    let f = form.clone(); ui.on_lock_now(move || f.borrow_mut().lock = true);
    let mut app = Auth { window, ui, form, controller: None, pending: None, epoch: 0, idle: 60000, stage: 4, locked: true, welcome: false, shift: false, pointer: None,
        #[cfg(feature = "desktop-wrapped-lock-ui")] pending_wrapped: None,
        #[cfg(feature = "desktop-wrapped-lock-ui")] wrapped: None,
        #[cfg(feature = "desktop-wrapped-lock-ui")] abandon_next: false,
    };
    app.sync();
    match read() {
        Ok((_, None)) => { app.stage = 2; app.ui.set_status("Choose at least 8 characters. Disposable test only.".into()); debug_println!("LOCK_AUTH|SETUP"); }
        Ok((_, Some(data))) => {
            #[cfg(not(feature = "desktop-wrapped-lock-ui"))]
            let settings = Some(data);
            #[cfg(feature = "desktop-wrapped-lock-ui")]
            let settings = bootstrap::decode(&data).map(|(settings, envelope, _)| { app.wrapped = Some(envelope); settings });
            app.controller = settings.and_then(|data| LockSettings::decode(&data).ok()).and_then(|s| s.into_controller(now()).ok().flatten());
            if app.controller.is_some() { app.stage = 1; app.welcome = true; debug_println!("LOCK_AUTH|RESTORED_LOCKED"); }
            else { app.ui.set_status("Invalid lock record. Device stays locked.".into()); }
        }
        Err(()) => app.ui.set_status("Storage unavailable. Device stays locked.".into()),
    }
    app.draw(); app
}
impl Handler for Auth {
    type Error = Infallible;
    fn notified(&mut self, channels: ChannelSet) -> Result<(), Self::Error> {
        self.sync();
        if channels.contains(Channel::new(2)) { self.input(); }
        self.actions(); self.draw(); Ok(())
    }
}
