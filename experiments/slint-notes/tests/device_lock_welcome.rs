#![cfg(feature = "full-suite-ui")]

mod platform;

use std::{cell::Cell, rc::Rc};
use personal_os_slint_notes_spike::NotesWindow;
use slint::{ComponentHandle, LogicalPosition, PhysicalSize};
use slint::platform::{PointerEventButton, WindowAdapter, WindowEvent};

#[test]
fn lock_welcome_requires_an_upward_swipe_before_entering_a_passphrase() {
    let window = platform::install();
    window.set_size(PhysicalSize::new(414, 897));
    let ui = NotesWindow::new().unwrap();
    ui.set_device_lock_screen(true);
    ui.set_device_lock_stage(1);
    let keys = Rc::new(Cell::new(0));
    let observed = keys.clone();
    ui.on_device_lock_key(move |_| observed.set(observed.get() + 1));
    ui.show().unwrap();

    let tap = |x, y| {
        let position = LogicalPosition::new(x, y);
        window.window().dispatch_event(WindowEvent::PointerMoved { position });
        window.window().dispatch_event(WindowEvent::PointerPressed { position, button: PointerEventButton::Left });
        window.window().dispatch_event(WindowEvent::PointerReleased { position, button: PointerEventButton::Left });
    };

    tap(50.0, 630.0);
    assert_eq!(keys.get(), 0, "welcome must not accept hidden passphrase input");
    assert!(ui.get_device_lock_screen());
    tap(207.0, 827.0);
    tap(50.0, 630.0);
    assert_eq!(keys.get(), 0, "a tap must not reveal the keyboard");
    let short_start = LogicalPosition::new(207.0, 827.0);
    let short_end = LogicalPosition::new(207.0, 800.0);
    window.window().dispatch_event(WindowEvent::PointerPressed { position: short_start, button: PointerEventButton::Left });
    window.window().dispatch_event(WindowEvent::PointerMoved { position: short_end });
    window.window().dispatch_event(WindowEvent::PointerReleased { position: short_end, button: PointerEventButton::Left });
    tap(50.0, 630.0);
    assert_eq!(keys.get(), 0, "a short movement must not reveal the keyboard");
    let start = LogicalPosition::new(207.0, 827.0);
    let end = LogicalPosition::new(207.0, 680.0);
    window.window().dispatch_event(WindowEvent::PointerMoved { position: start });
    window.window().dispatch_event(WindowEvent::PointerPressed { position: start, button: PointerEventButton::Left });
    window.window().dispatch_event(WindowEvent::PointerMoved { position: end });
    window.window().dispatch_event(WindowEvent::PointerReleased { position: end, button: PointerEventButton::Left });
    assert!(ui.get_device_lock_screen(), "opening the keyboard must not unlock");
    tap(50.0, 630.0);
    assert_eq!(keys.get(), 1, "the keyboard should accept input after the upward swipe");
}
