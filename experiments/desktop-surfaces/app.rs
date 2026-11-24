#![no_std]
#![no_main]
extern crate alloc;

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

const WIDTH: usize = 480;
const HEIGHT: usize = 800;
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
        111 => Key::Delete.into(),
        103 => Key::UpArrow.into(),
        108 => Key::DownArrow.into(),
        105 => Key::LeftArrow.into(),
        106 => Key::RightArrow.into(),
        _ => return None,
    })
}

struct App {
    window: Rc<MinimalSoftwareWindow>,
    ui: NotesWindow,
    surface: *mut Header,
    id: usize,
    counter: usize,
    interactive: bool,
    held: [Option<SharedString>; 128],
    pointer: Option<LogicalPosition>,
    #[cfg(feature = "desktop-dynamic-ui")]
    generation: usize,
}

impl App {
    fn draw(&mut self) {
        #[cfg(not(feature = "desktop-dynamic-ui"))]
        self.ui
            .set_status(format!("PROCESS {} / UPDATE {}", self.id, self.counter).into());
        #[cfg(feature = "desktop-dynamic-ui")]
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
        #[cfg(not(feature = "desktop-dynamic-ui"))]
        let queue = memory_region_symbol!(app_input: *mut InputQueue).as_ptr();
        #[cfg(feature = "desktop-dynamic-ui")]
        let queue = 0x66000000 as *mut InputQueue;
        let queue = unsafe { &*queue };
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
            // The fixture uses disposable text only. This is not a shipping input trace.
            #[cfg(not(feature = "desktop-lock-app"))]
            debug_println!("INPUT|TEXT|app={}|{}", self.id, after);
        }
        self.counter += 1;
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
        interactive: unsafe { ptr::read_volatile(ptr::addr_of!(APP_ID[1])) != 0 },
        held: core::array::from_fn(|_| None),
        pointer: None,
        #[cfg(feature = "desktop-dynamic-ui")]
        generation: unsafe { ptr::read_volatile(0x68000000 as *const usize) },
    };
    app.ui.set_note(if app.interactive {
        SharedString::default()
    } else {
        format!("App {}\nIndependent counter {}", app.id, app.counter).into()
    });
    app.draw();
    app
}

impl Handler for App {
    type Error = Infallible;
    fn notified(&mut self, channels: ChannelSet) -> Result<(), Self::Error> {
        if self.interactive && channels.contains(Channel::new(2)) {
            self.input();
        } else if channels.contains(Channel::new(1)) {
            unsafe {
                ptr::read_volatile(0x61000000 as *const u8);
            }
            debug_println!("SURFACE|FAIL|peer surface readable");
        } else if channels.contains(Channel::new(0)) {
            self.counter += 1;
            self.ui
                .set_note(format!("App {}\nIndependent counter {}", self.id, self.counter).into());
            self.draw();
        }
        Ok(())
    }
}
