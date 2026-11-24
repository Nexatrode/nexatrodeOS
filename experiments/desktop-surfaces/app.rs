#![no_std]
#![no_main]
extern crate alloc;
#[cfg(feature = "desktop-private-notes")]
#[path = "../desktop-notes/client.rs"]
mod private_notes;
#[cfg(feature = "desktop-private-notes")]
use core::cell::Cell;

use alloc::{format, rc::Rc};
use core::{
    ptr,
    sync::atomic::{AtomicU32, Ordering, fence},
};
use personal_os_slint_notes_spike::{NotesWindow, guest_platform};
#[cfg(not(feature = "desktop-dynamic-ui"))]
use sel4_microkit::memory_region_symbol;
use sel4_microkit::{Channel, ChannelSet, Handler, Infallible, debug_println, protection_domain};
use slint::{
    ComponentHandle, LogicalPosition, PhysicalSize, SharedString,
    platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType},
    platform::{Key, PointerEventButton, WindowEvent},
};

#[cfg(not(feature = "desktop-shell-app"))]
const WIDTH: usize = 480;
#[cfg(not(feature = "desktop-shell-app"))]
const HEIGHT: usize = 800;
#[cfg(feature = "desktop-shell-app")]
const WIDTH: usize = 560 * DESKTOP_SCALE;
#[cfg(feature = "desktop-shell-app")]
const HEIGHT: usize = 520 * DESKTOP_SCALE;
#[cfg(feature = "desktop-shell-app")]
const DESKTOP_SCALE: usize = if cfg!(feature = "desktop-shell-2x") { 2 } else { 1 };
const HEAP_BYTES: usize = 8 * 1024 * 1024;
#[used]
#[unsafe(link_section = ".app_identity")]
static APP_ID: [usize; 2] = [1, 0];
type HeapMutex = sel4_microkit::_private::heap::RawOneShotMutex;
#[global_allocator]
static HEAP: sel4_dlmalloc::DeferredStaticDlmalloc<HeapMutex> =
    sel4_dlmalloc::DeferredStaticDlmalloc::new();

#[repr(C)]
struct Header {
    revision: AtomicU32,
    width: u32,
    height: u32,
    stride: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct InputEvent {
    kind: u32,
    code: u32,
    value: u32,
    x: i32,
    y: i32,
}
#[repr(C)]
struct InputQueue {
    head: AtomicU32,
    tail: AtomicU32,
    reserved: [u32; 2],
    events: [InputEvent; 64],
}
const _: () = assert!(core::mem::size_of::<InputEvent>() == 20);

fn input_address() -> *mut InputQueue {
    #[cfg(not(feature = "desktop-dynamic-ui"))]
    { memory_region_symbol!(app_input: *mut InputQueue).as_ptr() }
    #[cfg(feature = "desktop-dynamic-ui")]
    { 0x66000000 as *mut InputQueue }
}

fn key_text(code: usize) -> Option<SharedString> {
    let character = match code {
        2..=10 => Some((b'1' + (code - 2) as u8) as char),
        11 => Some('0'),
        16..=25 => Some(b"qwertyuiop"[code - 16] as char),
        30..=38 => Some(b"asdfghjkl"[code - 30] as char),
        44..=50 => Some(b"zxcvbnm"[code - 44] as char),
        57 => Some(' '),
        _ => None,
    };
    if let Some(character) = character {
        return Some(format!("{character}").into());
    }
    Some(match code {
        14 => Key::Backspace.into(),
        28 => Key::Return.into(),
        42 | 54 => Key::Shift.into(),
        29 | 97 => Key::Control.into(),
        111 => Key::Delete.into(),
        103 => Key::UpArrow.into(),
        108 => Key::DownArrow.into(),
        105 => Key::LeftArrow.into(),
        106 => Key::RightArrow.into(),
        _ => return None,
    })
}

#[cfg(feature = "desktop-shell-app")]
fn calculator_key(ui: &NotesWindow, state: &core::cell::RefCell<personal_os_slint_notes_spike::calculator::Calculator>, key: &str) {
    use personal_os_slint_notes_spike::calculator::{Command, Operation};
    let command = match key {
        "C" => Command::Clear, "=" | "\n" => Command::Equals,
        "+" => Command::Operation(Operation::Add), "-" => Command::Operation(Operation::Subtract),
        "*" => Command::Operation(Operation::Multiply), "/" => Command::Operation(Operation::Divide),
        _ if key.len() == 1 && key.as_bytes()[0].is_ascii_digit() => Command::Digit(key.as_bytes()[0] - b'0'),
        _ => return,
    };
    let result = state.borrow_mut().press(command);
    ui.set_result(result.as_str().into());
    debug_println!("DESKTOP_CALC|DISPLAY|{}", result);
}
struct App {
    window: Rc<MinimalSoftwareWindow>,
    ui: NotesWindow,
    surface: *mut Header,
    id: usize,
    counter: usize,
    #[cfg(feature = "desktop-shell-app")]
    calculator: Rc<core::cell::RefCell<personal_os_slint_notes_spike::calculator::Calculator>>,
    interactive: bool,
    held: [Option<SharedString>; 128],
    pointer: Option<LogicalPosition>,
    #[cfg(feature = "desktop-dynamic-ui")]
    generation: usize,
    #[cfg(feature = "desktop-private-notes")]
    store: private_notes::Store,
    #[cfg(feature = "desktop-private-notes")]
    save_requested: Rc<Cell<bool>>,
}

impl App {
    fn draw(&mut self) {
        #[cfg(all(not(feature = "desktop-dynamic-ui"), not(feature = "desktop-shell-app")))]
        self.ui
            .set_status(format!("PROCESS {} / UPDATE {}", self.id, self.counter).into());
        #[cfg(all(feature = "desktop-dynamic-ui", not(feature = "desktop-private-notes"), not(feature = "desktop-shell-app")))]
        self.ui
            .set_status(format!("INSTANCE {} / UPDATE {}", self.generation, self.counter).into());
        slint::platform::update_timers_and_animations();
        let header = unsafe { &mut *self.surface };
        let before = header.revision.load(Ordering::Relaxed);
        header
            .revision
            .store(before.wrapping_add(1), Ordering::Release);
        fence(Ordering::SeqCst);
        header.width = WIDTH as u32;
        header.height = HEIGHT as u32;
        header.stride = (WIDTH * 3) as u32;
        let pixels = unsafe {
            core::slice::from_raw_parts_mut(
                self.surface.add(1).cast::<slint::Rgb8Pixel>(),
                WIDTH * HEIGHT,
            )
        };
        self.window.draw_if_needed(|renderer| {
            renderer.render(pixels, WIDTH);
        });
        header
            .revision
            .store(before.wrapping_add(2), Ordering::Release);
        #[cfg(not(feature = "desktop-dynamic-ui"))]
        Channel::new(0).notify();
        #[cfg(feature = "desktop-dynamic-ui")]
        {
            if self.counter == 0 {
                #[cfg(feature = "desktop-lock-app")]
                {
                    sel4_microkit::set_mr(0, 1);
                    let reply = Channel::new(0).pp_call(sel4_microkit::MessageInfo::new(0x791, 1));
                    assert_eq!(reply.label(), 1, "app unlocked device");
                    debug_println!("LOCK_APP|UNLOCK_DENIED");
                }
                let malformed = Channel::new(0).pp_call(sel4_microkit::MessageInfo::new(0x780, 0));
                assert_eq!(malformed.label(), 1, "malformed publication accepted");
                sel4_microkit::set_mr(0, self.generation.wrapping_add(1) as u64);
                sel4_microkit::set_mr(1, 0);
                let forged = Channel::new(0).pp_call(sel4_microkit::MessageInfo::new(0x780, 2));
                assert_eq!(forged.label(), 1, "forged instance accepted");
                sel4_microkit::set_mr(0, self.generation as u64);
                let stride = header.stride;
                header.stride = 0;
                let invalid_frame =
                    Channel::new(0).pp_call(sel4_microkit::MessageInfo::new(0x780, 2));
                header.stride = stride;
                assert_eq!(invalid_frame.label(), 1, "invalid frame accepted");
                debug_println!("DYNAMIC_APP|RPC_DENIALS|generation={}", self.generation);
            }
            sel4_microkit::set_mr(0, self.generation as u64);
            sel4_microkit::set_mr(1, self.counter as u64);
            let reply = Channel::new(0).pp_call(sel4_microkit::MessageInfo::new(0x780, 2));
            assert_eq!(reply.label(), 0, "surface publication rejected");
        }
    }

    fn reset_input(&mut self) {
        for text in self.held.iter_mut().filter_map(Option::take) {
            self.window
                .dispatch_event(WindowEvent::KeyReleased { text });
        }
        if let Some(position) = self.pointer.take() {
            self.window.dispatch_event(WindowEvent::PointerReleased {
                position,
                button: PointerEventButton::Left,
            });
        }
    }

    fn input(&mut self) {
        let queue = unsafe { &*input_address() };
        let head = queue.head.load(Ordering::Acquire);
        let mut tail = queue.tail.load(Ordering::Acquire);
        if head.wrapping_sub(tail) > 64 {
            self.reset_input();
            queue.tail.store(head, Ordering::Release);
            return;
        }
        let before = self.ui.get_note();
        while tail != head {
            let event = unsafe { ptr::read_volatile(&queue.events[tail as usize % 64]) };
            tail = tail.wrapping_add(1);
            match event.kind {
                1 if event.code < 128 && event.value <= 2 => {
                    let code = event.code as usize;
                    #[cfg(feature = "desktop-private-notes")]
                    if code == 60 || code == 61 {
                        if event.value == 1 && code == 60 { self.save_requested.set(true); }
                        if event.value == 1 && code == 61 {
                            if self.store.snapshot(self.ui.get_note().as_str()).is_ok() {
                                debug_println!("PRIVATE_NOTES_APP|SNAPSHOT|generation={}", self.generation);
                            }
                        }
                        continue;
                    }
                    #[cfg(feature = "desktop-shell-app")]
                    if self.id == 2 {
                        if event.value == 1 {
                            let key = match code { 13 | 28 => Some("=".into()), 12 => Some("-".into()), 53 => Some("/".into()), 1 => Some("C".into()), _ => key_text(code) };
                            if let Some(key) = key { calculator_key(&self.ui, &self.calculator, &key); }
                        }
                        continue;
                    }
                    if event.value == 1 && self.held[code].is_none() {
                        self.held[code] = key_text(code);
                    }
                    if let Some(text) = self.held[code].clone() {
                        self.window.dispatch_event(match event.value {
                            0 => WindowEvent::KeyReleased { text },
                            1 => WindowEvent::KeyPressed { text },
                            _ => WindowEvent::KeyPressRepeated { text },
                        });
                        #[cfg(not(feature = "desktop-lock-app"))]
                        debug_println!(
                            "INPUT|KEY|app={}|code={}|value={}",
                            self.id,
                            code,
                            event.value
                        );
                    }
                    if event.value == 0 {
                        self.held[code] = None;
                    }
                }
                2 if event.value <= 2 => {
                    let position = LogicalPosition::new(event.x as f32, event.y as f32);
                    self.window.dispatch_event(match event.value {
                        0 => {
                            self.pointer = None;
                            WindowEvent::PointerReleased {
                                position,
                                button: PointerEventButton::Left,
                            }
                        }
                        1 => {
                            self.pointer = Some(position);
                            WindowEvent::PointerPressed {
                                position,
                                button: PointerEventButton::Left,
                            }
                        }
                        _ => {
                            if self.pointer.is_some() {
                                self.pointer = Some(position);
                            }
                            WindowEvent::PointerMoved { position }
                        }
                    });
                }
                3 if event.value <= 1 => {
                    self.window
                        .dispatch_event(WindowEvent::WindowActiveChanged(event.value == 1));
                }
                4 => self.reset_input(),
                _ => (),
            }
        }
        queue.tail.store(tail, Ordering::Release);
        let after = self.ui.get_note();
        if before != after {
            #[cfg(feature = "desktop-private-notes")]
            self.ui.set_status("Unsaved changes".into());
            // The fixture uses disposable text only. This is not a shipping input trace.
            #[cfg(not(feature = "desktop-lock-app"))]
            debug_println!("INPUT|TEXT|app={}|{}", self.id, after);
        }
        self.counter += 1;
        #[cfg(feature = "desktop-private-notes")]
        if self.save_requested.replace(false) {
            self.ui.set_status("Saving…".into());
            self.draw();
            let note = self.ui.get_note();
            match self.store.save(note.as_str()) {
                Ok(()) => {
                    self.ui.set_status("Saved encrypted".into());
                    debug_println!("PRIVATE_NOTES_APP|SAVED|revision={}", self.store.revision());
                }
                Err(_) => { self.ui.set_status("Unable to save".into()); debug_println!("PRIVATE_NOTES_APP|SAVE_FAILED"); }
            }
        }
        self.draw();
    }
}

#[protection_domain]
fn init() -> App {
    #[cfg(not(feature = "desktop-dynamic-ui"))]
    let heap = memory_region_symbol!(app_heap: *mut u8);
    #[cfg(feature = "desktop-dynamic-ui")]
    let heap = core::ptr::NonNull::new(0x64000000 as *mut u8).unwrap();
    HEAP.set_bounds(sel4_dlmalloc::StaticHeapBounds::new(
        heap.as_ptr(),
        HEAP_BYTES,
    ))
    .unwrap();
    let window = MinimalSoftwareWindow::new(RepaintBufferType::ReusedBuffer);
    guest_platform::install(&window);
    #[cfg(feature = "desktop-shell-app")]
    window.dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor: DESKTOP_SCALE as f32 });
    let ui = NotesWindow::new().unwrap();
    ui.show().unwrap();
    window.set_size(PhysicalSize::new(WIDTH as u32, HEIGHT as u32));
    let mut app = App {
        window,
        ui,
        surface: {
            #[cfg(not(feature = "desktop-dynamic-ui"))]
            {
                memory_region_symbol!(app_surface: *mut Header).as_ptr()
            }
            #[cfg(feature = "desktop-dynamic-ui")]
            {
                0x60000000 as *mut Header
            }
        },
        id: unsafe { ptr::read_volatile(ptr::addr_of!(APP_ID[0])) },
        counter: 0,
        #[cfg(feature = "desktop-shell-app")]
        calculator: Rc::new(core::cell::RefCell::new(Default::default())),
        interactive: unsafe { ptr::read_volatile(ptr::addr_of!(APP_ID[1])) != 0 },
        held: core::array::from_fn(|_| None),
        pointer: None,
        #[cfg(feature = "desktop-dynamic-ui")]
        generation: unsafe { ptr::read_volatile(0x68000000 as *const usize) },
        #[cfg(feature = "desktop-private-notes")]
        store: private_notes::Store::new(unsafe { ptr::read_volatile(0x68000000 as *const u64) }),
        #[cfg(feature = "desktop-private-notes")]
        save_requested: Rc::new(Cell::new(false)),
    };
    #[cfg(not(feature = "desktop-private-notes"))]
    app.ui.set_note(if app.interactive {
        SharedString::default()
    } else {
        format!("App {}\nIndependent counter {}", app.id, app.counter).into()
    });
    #[cfg(feature = "desktop-private-notes")]
    {
        let save = app.save_requested.clone();
        app.ui.on_save_note(move |_| save.set(true));
        app.store.check_isolation();
        match app.store.load() {
            Ok(note) => {
                app.ui.set_note(note); app.ui.set_status("Private encrypted note".into());
                debug_println!("PRIVATE_NOTES_APP|LOADED|revision={}", app.store.revision());
            }
            Err(_) => { app.ui.set_note("".into()); app.ui.set_status("Unable to load".into()); debug_println!("PRIVATE_NOTES_APP|LOAD_FAILED"); }
        }
    }
    #[cfg(feature = "desktop-shell-app")]
    {
        app.ui.set_kind(app.id as i32);
        let weak = app.ui.as_weak();
        let calculator = app.calculator.clone();
        app.ui.on_calculator_key(move |key| {
            if let Some(ui) = weak.upgrade() { calculator_key(&ui, &calculator, &key); }
        });
    }
    app.draw();
    app
}

impl Handler for App {
    type Error = Infallible;
    fn notified(&mut self, channels: ChannelSet) -> Result<(), Self::Error> {
        #[cfg(feature = "desktop-shell-app")]
        if channels.contains(Channel::new(1)) {
            // Reset takes precedence over a coalesced input notification. Drop
            // unread events so a later reopen cannot replay old button presses.
            let queue = unsafe { &*input_address() };
            queue.tail.store(queue.head.load(Ordering::Acquire), Ordering::Release);
            self.reset_input();
            *self.calculator.borrow_mut() = Default::default();
            self.ui.set_result("0".into()); self.draw();
            debug_println!("DESKTOP_CALC|RESET");
            return Ok(());
        }
        if self.interactive && channels.contains(Channel::new(2)) {
            self.input();
        } else if channels.contains(Channel::new(1)) {
            #[cfg(not(feature = "desktop-shell-app"))]
            unsafe {
                ptr::read_volatile(0x61000000 as *const u8);
            }
            #[cfg(not(feature = "desktop-shell-app"))]
            debug_println!("SURFACE|FAIL|peer surface readable");
        } else if channels.contains(Channel::new(0)) {
            #[cfg(not(feature = "desktop-shell-app"))]
            {
            self.counter += 1;
            self.ui
                .set_note(format!("App {}\nIndependent counter {}", self.id, self.counter).into());
            }
            self.draw();
        }
        Ok(())
    }
}
