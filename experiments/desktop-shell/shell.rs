//! Static desktop shell. Its management endpoint is not delegated to app processes.
#![no_std]
#![no_main]
extern crate alloc;
use alloc::{rc::Rc, vec::Vec};
use core::{ptr, sync::atomic::{AtomicU32, Ordering, fence}};
use personal_os_slint_notes_spike::{DesktopShell, DesktopWindow, guest_platform, wall_clock};
use sel4_microkit::{Channel, ChannelSet, Handler, Infallible, MessageInfo, memory_region_symbol, protection_domain, set_mr, debug_println};
use slint::{ComponentHandle, LogicalPosition, PhysicalSize, Model, ModelRc, VecModel, platform::{PointerEventButton, WindowEvent, software_renderer::{MinimalSoftwareWindow, RepaintBufferType}}};
const APP_COUNT: usize = if cfg!(feature = "desktop-browser-shell") { 3 } else { 2 };
const SCALE: usize = if cfg!(feature = "desktop-shell-2x") { 2 } else { 1 };
const WIDTH: usize = 1280 * SCALE;
const HEIGHT: usize = 800 * SCALE;
const MAX_WIDTH: usize=if cfg!(feature="desktop-resize-shell") {3840} else {WIDTH};
const MAX_HEIGHT: usize=if cfg!(feature="desktop-resize-shell") {2160} else {HEIGHT};
type HeapMutex = sel4_microkit::_private::heap::RawOneShotMutex;
#[global_allocator]
static HEAP: sel4_dlmalloc::DeferredStaticDlmalloc<HeapMutex> = sel4_dlmalloc::DeferredStaticDlmalloc::new();
#[repr(C)] struct Header { revision: AtomicU32, width: u32, height: u32, stride: u32 }
#[repr(C)] #[derive(Clone, Copy, Default)] struct Window { x: i32, y: i32, open: u32, visible: u32 }
#[repr(C)] struct State { revision: AtomicU32, top: u32, focused: u32, reserved: u32, windows: [Window; APP_COUNT] }
#[repr(C)] #[derive(Clone, Copy)] struct Event { kind: u32, code: u32, value: u32, x: i32, y: i32 }
#[repr(C)] struct Queue { head: AtomicU32, tail: AtomicU32, reserved: [u32; 2], events: [Event; 64] }
#[repr(C)] struct Clock { revision: AtomicU32, length: u32, text: [u8; 128] }
const _: () = assert!(core::mem::size_of::<State>() == 16 + 16 * APP_COUNT);
struct Shell { window: Rc<MinimalSoftwareWindow>, ui: DesktopShell, revision: u32, pointer: Option<LogicalPosition>, model: Rc<VecModel<DesktopWindow>>, focused: u32,
    width:usize,height:usize,app_height:usize,force_full:bool,
    #[cfg(feature="desktop-virgl-shell")] decor_focus: Option<u32>,
    #[cfg(feature="desktop-virgl-shell")] scratch: Vec<slint::Rgb8Pixel>,
}
impl Shell {
    fn refresh(&mut self) {
        #[cfg(feature="desktop-resize-shell")]
        if let Some(mode)=personal_os_slint_notes_spike::desktop_resize::read() {
            if mode.app_height as usize!=self.app_height {self.app_height=mode.app_height as usize;self.decor_focus=None;self.ui.set_app_height(mode.app_height as i32);}
            if (self.width,self.height)!=(mode.width as usize,mode.height as usize) {
                self.width=mode.width as usize;self.height=mode.height as usize;
                self.window.set_size(PhysicalSize::new(mode.width,mode.height));self.window.request_redraw();
                self.force_full=true;
            }
        }
        let state = unsafe { &*memory_region_symbol!(shell_state: *mut State).as_ptr() };
        let first = state.revision.load(Ordering::Acquire);
        if first != self.revision && first != 0 && first & 1 == 0 {
            let top = unsafe { ptr::read_volatile(&state.top) };
            let focused = unsafe { ptr::read_volatile(&state.focused) };
            let wins = unsafe { ptr::read_volatile(&state.windows) };
            let packed = unsafe { ptr::read_volatile(&state.reserved) };
            fence(Ordering::Acquire);
            if first == state.revision.load(Ordering::Acquire) && (1..=APP_COUNT as u32).contains(&top) {
                let order: [usize; APP_COUNT] = core::array::from_fn(|i| {
                    if APP_COUNT==3 { (((packed >> (4*i)) & 15) as usize).wrapping_sub(1) }
                    else if i==0 { if top==1 {1} else {0} } else {top as usize-1}
                });
                if order.iter().any(|i|*i>=APP_COUNT) || order.iter().enumerate().any(|(i,v)|order[..i].contains(v)) { return; }
                let windows: Vec<_> = order.into_iter().map(|i| DesktopWindow { id: (i + 1) as i32, left: if cfg!(feature="desktop-virgl-shell") {0} else {wins[i].x}, top: if cfg!(feature="desktop-virgl-shell") {43} else {wins[i].y}, shown: wins[i].visible != 0, active: focused == (i + 1) as u32 }).collect();
                for (i, data) in windows.into_iter().enumerate() {
                    if let Some(old)=self.model.row_data(i) {
                        if old.id!=data.id || old.left!=data.left || old.top!=data.top || old.shown!=data.shown || old.active!=data.active {self.model.set_row_data(i,data);}
                    } else { self.model.push(data); }
                }
                self.ui.set_notes_running(wins[0].open != 0);
                self.ui.set_calculator_running(wins[1].open != 0);
                self.ui.set_browser_running(wins.get(2).is_some_and(|w|w.open!=0));
                self.revision = first;
                self.focused = focused;
            }
        }
        let clock = unsafe { &*memory_region_symbol!(clock_view: *mut Clock).as_ptr() };
        let revision = clock.revision.load(Ordering::Acquire);
        if revision & 1 == 0 && unsafe { ptr::read_volatile(&clock.length) } == 19 {
            let mut text = [0; 19];
            for (i, byte) in text.iter_mut().enumerate() { *byte = unsafe { clock.text.as_ptr().add(i).read_volatile() }; }
            fence(Ordering::Acquire);
            if revision == clock.revision.load(Ordering::Acquire) {
                if let Some(time) = wall_clock::display_time(&text, include_bytes!(concat!(env!("OUT_DIR"), "/timezone.bin"))) {
                    self.ui.set_clock(core::str::from_utf8(&time).unwrap().into());
                }
            }
        }
    }
    fn draw(&mut self) {
        self.refresh(); slint::platform::update_timers_and_animations();
        #[cfg(feature="desktop-virgl-shell")]
        if self.revision!=0 && self.decor_focus!=Some(self.focused) {
            const DW: usize=562*SCALE;
            let dh=(self.app_height+44)*SCALE;
            const _: ()=assert!(16+DW*564*SCALE*3<=0x400000);
            let base=memory_region_symbol!(shell_decorations:*mut u8).as_ptr();
            for id in 1..=APP_COUNT {
                self.ui.set_layer_window(id as i32);
                let scratch=&mut self.scratch;
                self.window.draw_if_needed(|renderer| {renderer.render(scratch,self.width);});
                let surface=unsafe {base.add((id-1)*0x400000).cast::<Header>()};
                let header=unsafe {&*surface};let before=header.revision.load(Ordering::Relaxed);
                header.revision.store(before.wrapping_add(1),Ordering::Release);fence(Ordering::SeqCst);
                unsafe {
                    ptr::addr_of_mut!((*surface).width).write(DW as u32);ptr::addr_of_mut!((*surface).height).write(dh as u32);ptr::addr_of_mut!((*surface).stride).write((DW*3) as u32);
                    let dest=surface.add(1).cast::<slint::Rgb8Pixel>();
                    for row in 0..dh {ptr::copy_nonoverlapping(self.scratch.as_ptr().add(row*self.width),dest.add(row*DW),DW);}
                }
                header.revision.store(before.wrapping_add(2),Ordering::Release);
            }
            self.ui.set_layer_window(0);self.decor_focus=Some(self.focused);
        }
        let surface = memory_region_symbol!(shell_surface: *mut Header).as_ptr();
        let header = unsafe { &*surface };
        let before = header.revision.load(Ordering::Relaxed);
        let mut changed = false;
        let force_full=self.force_full;
        self.window.draw_if_needed(|renderer| {
            header.revision.store(before.wrapping_add(1), Ordering::Release); fence(Ordering::SeqCst);
            unsafe { ptr::addr_of_mut!((*surface).width).write(self.width as u32); ptr::addr_of_mut!((*surface).height).write(self.height as u32); ptr::addr_of_mut!((*surface).stride).write((self.width * 3) as u32); }
            let pixels = unsafe { core::slice::from_raw_parts_mut(surface.add(1).cast::<slint::Rgb8Pixel>(), self.width * self.height) };
            if force_full {renderer.set_repaint_buffer_type(RepaintBufferType::NewBuffer);}
            renderer.render(pixels, self.width);
            if force_full {renderer.set_repaint_buffer_type(RepaintBufferType::ReusedBuffer);}
            header.revision.store(before.wrapping_add(2), Ordering::Release);
            changed = true;
        });
        if changed {self.force_full=false;Channel::new(1).notify();}
    }
    fn input(&mut self) {
        let q = unsafe { &*memory_region_symbol!(shell_input: *mut Queue).as_ptr() };
        let head = q.head.load(Ordering::Acquire); let mut tail = q.tail.load(Ordering::Acquire);
        if head.wrapping_sub(tail) > 64 { tail = head; self.release(); }
        while tail != head {
            let e = unsafe { q.events.as_ptr().add(tail as usize % 64).read_volatile() }; tail = tail.wrapping_add(1);
            if e.kind == 4 { self.release(); }
            if e.kind != 2 || e.value > 2 { continue; }
            let position = LogicalPosition::new(e.x as f32, e.y as f32);
            self.window.dispatch_event(match e.value {
                0 => { self.pointer = None; WindowEvent::PointerReleased { position, button: PointerEventButton::Left } },
                1 => { self.pointer = Some(position); WindowEvent::PointerPressed { position, button: PointerEventButton::Left } },
                _ => WindowEvent::PointerMoved { position },
            });
        }
        q.tail.store(tail, Ordering::Release);
    }
    fn release(&mut self) {
        if let Some(position) = self.pointer.take() { self.window.dispatch_event(WindowEvent::PointerReleased { position, button: PointerEventButton::Left }); }
    }
}
#[protection_domain]
fn init() -> Shell {
    HEAP.set_bounds(sel4_dlmalloc::StaticHeapBounds::new(memory_region_symbol!(shell_heap: *mut u8).as_ptr(), (if cfg!(feature="desktop-resize-shell") {128} else if cfg!(feature="desktop-virgl-shell") {64} else {32}) * 1024 * 1024)).unwrap();
    let window = MinimalSoftwareWindow::new(RepaintBufferType::ReusedBuffer); guest_platform::install(&window);
    window.dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor: SCALE as f32 });
    let ui = DesktopShell::new().unwrap();
    ui.set_browser_enabled(APP_COUNT==3);
    ui.set_real_apps(cfg!(feature = "desktop-apps-shell"));
    ui.set_compositor_layers(cfg!(feature="desktop-virgl-shell"));
    ui.on_command(|command, id, x, y| {
        // Three-app input focus and buttons are committed by the trusted
        // manager in event order, independently of shell repaint latency.
        if cfg!(feature="desktop-virgl-shell") || (APP_COUNT == 3 && command != 4) { return; }
        for (i, value) in [command, id, x, y].into_iter().enumerate() { set_mr(i, value as u64); }
        let reply = Channel::new(0).pp_call(MessageInfo::new(0x7b0, 4));
        if reply.label() != 0 { debug_println!("DESKTOP_SHELL|COMMAND_REJECTED|{}", command); }
    });
    ui.show().unwrap(); window.set_size(PhysicalSize::new(WIDTH as u32, HEIGHT as u32));
    let model = Rc::new(VecModel::from(Vec::<DesktopWindow>::new())); ui.set_windows(ModelRc::from(model.clone()));
    let mut shell = Shell { window, ui, revision: 0, pointer: None, model, focused:0,width:WIDTH,height:HEIGHT,app_height:520,force_full:true,
        #[cfg(feature="desktop-virgl-shell")] decor_focus:None,
        #[cfg(feature="desktop-virgl-shell")] scratch:alloc::vec![slint::Rgb8Pixel::new(0,0,0);MAX_WIDTH*MAX_HEIGHT],
    }; shell.draw();
    debug_println!("DESKTOP_SHELL|READY|{}x{}|scale={}", WIDTH, HEIGHT, SCALE); shell
}
impl Handler for Shell {
    type Error = Infallible;
    fn notified(&mut self, channels: ChannelSet) -> Result<(), Self::Error> {
        self.refresh();
        if channels.contains(Channel::new(2)) { self.input(); }
        self.draw(); Ok(())
    }
}
