//! Experimental UTM-only software lock. Not a hardware trust boundary.
use alloc::rc::Rc;
use core::{cell::RefCell, ptr::NonNull};
use personal_os_rust_server::{device_lock::{Credential, LockController, LockSettings, UnlockResult}, vault_blob_transfer::BlobTransferPage};
use sel4_microkit::{Channel, MessageInfo, get_mr, set_mr};
use slint::ComponentHandle;
use zeroize::Zeroize;
use crate::NotesWindow;

const VAULT: Channel = Channel::new(15);
const ENTROPY: Channel = Channel::new(16);
const READ: usize = 0x450;
const WRITE: usize = 0x451;
const OK: usize = 0x300;
const NO_BLOB: usize = 0x30d;
const IDLE_MS: u64 = 60_000;

pub struct DeviceLock {
    page: NonNull<BlobTransferPage>,
    controller: Option<LockController>,
    pending: Option<Credential>,
    input: [u8; 128],
    length: usize,
    stage: u8,
}

impl Drop for DeviceLock {
    fn drop(&mut self) { self.input.zeroize(); }
}

fn now_ms() -> u64 {
    let t = personal_os_slint_notes_spike::guest_platform::counter_ticks();
    let f = personal_os_slint_notes_spike::guest_platform::counter_frequency();
    personal_os_slint_notes_spike::frame_clock::elapsed(t, f).as_millis().min(u64::MAX as u128) as u64
}

fn open_screen(ui: &NotesWindow, stage: u8, status: &str) {
    ui.set_page(14);
    ui.set_edit_target(0);
    ui.set_device_lock_stage(i32::from(stage));
    ui.set_device_lock_welcome(stage == 1);
    ui.set_device_lock_setup_feedback(if matches!(stage, 2 | 3) { "0 characters entered" } else { "" }.into());
    ui.set_device_lock_status(status.into());
    ui.set_device_lock_screen(true);
}

fn close_screen(ui: &NotesWindow) {
    ui.set_page(0);
    ui.set_edit_target(0);
    ui.set_device_lock_screen(false);
    ui.set_device_lock_setup_feedback("".into());
}

impl DeviceLock {
    fn clear(&mut self) { self.input.zeroize(); self.length = 0; }

    fn read(&mut self) -> Result<(u64, Option<[u8; personal_os_rust_server::device_lock::SETTINGS_SIZE]>), ()> {
        let reply = VAULT.pp_call(MessageInfo::new(READ as _, 0));
        if reply.count() != 1 { return Err(()); }
        let generation = get_mr(0);
        if reply.label() == NO_BLOB as _ { return Ok((generation, None)); }
        if reply.label() != OK as _ { return Err(()); }
        let page = unsafe { self.page.as_mut() };
        let mut settings = [0; personal_os_rust_server::device_lock::SETTINGS_SIZE];
        let payload = page.payload().ok_or(())?;
        if payload.len() != settings.len() { page.set_payload(&[]); return Err(()); }
        settings.copy_from_slice(payload);
        page.set_payload(&[]);
        Ok((generation, Some(settings)))
    }

    fn save(&mut self, settings: &[u8]) -> Result<(), ()> {
        // Both slots must carry the setting after success: otherwise loss of
        // the only new copy could resurrect a pre-enrollment unlocked scalar.
        for _ in 0..2 {
            let (generation, _) = self.read()?;
            if !unsafe { self.page.as_mut() }.set_payload(settings) { return Err(()); }
            set_mr(0, generation);
            let reply = VAULT.pp_call(MessageInfo::new(WRITE as _, 1));
            unsafe { self.page.as_mut() }.set_payload(&[]);
            if reply.label() != OK as _ || reply.count() != 1 { return Err(()); }
            let (generation_after, stored) = self.read()?;
            if generation_after <= generation || stored.as_ref().is_none_or(|saved| saved.as_slice() != settings) { return Err(()); }
        }
        Ok(())
    }

    fn salt() -> Result<[u8; 16], ()> {
        let reply = ENTROPY.pp_call(MessageInfo::new(0x401, 0));
        if reply.label() != 0x400 || reply.count() != 2 { return Err(()); }
        let mut salt = [0u8; 16];
        salt[..8].copy_from_slice(&get_mr(0).to_le_bytes());
        salt[8..].copy_from_slice(&get_mr(1).to_le_bytes());
        Ok(salt)
    }

    fn key(&mut self, key: &str) {
        if !matches!(self.stage, 1..=4) { return; }
        match key {
            "DELETE" | "\u{8}" => { if self.length > 0 { self.length -= 1; self.input[self.length] = 0; } }
            "CLEAR" => self.clear(),
            "SPACE" | " " if self.length < self.input.len() => { self.input[self.length] = b' '; self.length += 1; }
            _ if key.len() == 1 && key.as_bytes()[0].is_ascii_graphic() && self.length < self.input.len() => {
                self.input[self.length] = key.as_bytes()[0]; self.length += 1;
            }
            _ => {}
        }
    }

    fn submit(&mut self, ui: &NotesWindow) {
        match self.stage {
            2 => {
                let candidate = Self::salt().and_then(|salt| Credential::create(&self.input[..self.length], salt).map_err(|_| ()));
                self.clear();
                match candidate {
                    Ok(credential) => { self.pending = Some(credential); self.stage = 3; open_screen(ui, 3, "Enter the same passphrase again."); }
                    Err(_) => open_screen(ui, 2, "Use at least 8 characters; entropy must be available."),
                }
            }
            3 => {
                let matches = self.pending.as_ref().is_some_and(|c| c.verify(&self.input[..self.length]));
                self.clear();
                if !matches { self.pending = None; self.stage = 2; open_screen(ui, 2, "Passphrases did not match. Start again."); return; }
                let Some(credential) = self.pending.take() else { return; };
                let Ok(settings) = LockSettings::enabled(credential, IDLE_MS) else { return; };
                let record = settings.encode();
                if self.save(&record).is_err() { self.stage = 0; open_screen(ui, 0, "Lock storage unavailable; restart required."); return; }
                match LockSettings::decode(&record).and_then(|s| s.into_controller(now_ms())) {
                    Ok(Some(mut controller)) => {
                        // Enrollment confirmation already verified the passphrase.
                        // Unlock via a second entry to avoid retaining its plaintext.
                        controller.lock(); self.controller = Some(controller); self.stage = 1;
                        ui.set_device_lock_enabled(true); open_screen(ui, 1, "Passphrase set. Enter it to unlock.");
                    }
                    _ => { self.stage = 0; open_screen(ui, 0, "Lock record unavailable."); }
                }
            }
            1 | 4 => {
                let input = &self.input[..self.length];
                let result = self.controller.as_mut().map(|c| {
                    let result = c.try_unlock(input, now_ms());
                    c.authentication_finished(now_ms(), result);
                    if c.is_locked() && result == UnlockResult::Unlocked { UnlockResult::Denied } else { result }
                });
                self.clear();
                if result != Some(UnlockResult::Unlocked) { ui.set_device_lock_status("Unable to unlock. Wait and try again.".into()); return; }
                if self.stage == 4 {
                    if self.save(&LockSettings::disabled().encode()).is_err() {
                        if let Some(c) = self.controller.as_mut() { c.lock(); }
                        ui.set_device_lock_status("Lock storage unavailable.".into()); return;
                    }
                    self.controller = None; ui.set_device_lock_enabled(false);
                }
                self.stage = 0; close_screen(ui);
            }
            _ => {}
        }
    }

    pub fn activity(&mut self, ui: &NotesWindow) {
        if let Some(controller) = self.controller.as_mut() {
            controller.activity(now_ms());
            if controller.is_locked() && !ui.get_device_lock_screen() {
                self.clear(); self.stage = 1; open_screen(ui, 1, "Enter your passphrase.");
            }
        }
    }

    pub fn tick(&mut self, ui: &NotesWindow) {
        if let Some(controller) = self.controller.as_mut() {
            controller.tick(now_ms());
            if controller.is_locked() && !ui.get_device_lock_screen() {
                self.clear(); self.stage = 1; open_screen(ui, 1, "Enter your passphrase.");
            }
        }
    }

    pub fn enabled(&self) -> bool { self.controller.is_some() }
}

pub fn initialize(ui: &NotesWindow, page: NonNull<BlobTransferPage>) -> Rc<RefCell<DeviceLock>> {
    open_screen(ui, 0, "Checking device lock…");
    let state = Rc::new(RefCell::new(DeviceLock { page, controller: None, pending: None, input: [0; 128], length: 0, stage: 0 }));
    {
        let mut lock = state.borrow_mut();
        match lock.read().and_then(|(_, settings)| match settings {
            None => Ok(None),
            Some(bytes) => LockSettings::decode(&bytes).map_err(|_| ())?.into_controller(now_ms()).map_err(|_| ()),
        }) {
            Ok(Some(controller)) => { lock.controller = Some(controller); lock.stage = 1; ui.set_device_lock_enabled(true); open_screen(ui, 1, "Enter your passphrase."); }
            Ok(None) => { ui.set_device_lock_enabled(false); close_screen(ui); }
            Err(_) => { lock.stage = 0; open_screen(ui, 0, "Lock storage unavailable; restart required."); }
        }
    }
    ui.set_device_lock_available(true);
    let shared = state.clone(); let weak = ui.as_weak(); ui.on_device_lock_key(move |key| {
        let mut lock = shared.borrow_mut();
        lock.key(key.as_str());
        if matches!(lock.stage, 2 | 3) {
            if let Some(ui) = weak.upgrade() {
                ui.set_device_lock_setup_feedback(alloc::format!("{} characters entered", lock.length).into());
            }
        }
    });
    let shared = state.clone(); let weak = ui.as_weak(); ui.on_device_lock_submit(move || {
        if let Some(ui) = weak.upgrade() { shared.borrow_mut().submit(&ui); }
    });
    let shared = state.clone(); let weak = ui.as_weak(); ui.on_device_lock_action(move |action| {
        if let Some(ui) = weak.upgrade() {
            let mut lock = shared.borrow_mut(); lock.clear();
            if action == 2 {
                lock.pending = None;
                if lock.controller.is_some() { lock.stage = 1; open_screen(&ui, 1, "Enter your passphrase."); }
                else { lock.stage = 0; close_screen(&ui); }
            } else if action == 1 {
                if let Some(controller) = lock.controller.as_mut() { controller.lock(); lock.stage = 4; open_screen(&ui, 4, "Enter your passphrase to disable the lock."); }
            } else if let Some(controller) = lock.controller.as_mut() {
                controller.lock(); lock.stage = 1; open_screen(&ui, 1, "Enter your passphrase.");
            } else { lock.stage = 2; open_screen(&ui, 2, "Choose a passphrase of at least 8 characters."); }
        }
    });
    state
}
