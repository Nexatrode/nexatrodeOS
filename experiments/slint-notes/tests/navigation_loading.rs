#![cfg(feature = "full-suite-ui")]
use personal_os_slint_notes_spike::NotesWindow;
mod platform;
use slint::{ComponentHandle, PhysicalSize, LogicalPosition};
use slint::platform::{PointerEventButton, WindowAdapter, WindowEvent};
use std::{cell::Cell, rc::Rc};

#[test]
fn loading_conversation_does_not_send_or_open_attachment_picker() {
    let window = platform::install();
    let ui = NotesWindow::new().unwrap();
    ui.set_page(6);
    ui.set_message_service_ready(true);
    ui.set_message_view(true);
    ui.set_message_target_ready(false);
    ui.set_message_body("draft".into());
    ui.show().unwrap();
    window.set_size(PhysicalSize::new(414, 736));
    window.draw_if_needed(|renderer| { renderer.render(&mut vec![slint::Rgb8Pixel::default(); 414 * 736], 414); });
    let sends = Rc::new(Cell::new(0));
    let called = sends.clone();
    ui.on_send_message(move |_| called.set(called.get() + 1));
    for position in [LogicalPosition::new(360., 670.), LogicalPosition::new(40., 670.)] {
        window.window().dispatch_event(WindowEvent::PointerPressed { position, button: PointerEventButton::Left });
        window.window().dispatch_event(WindowEvent::PointerReleased { position, button: PointerEventButton::Left });
    }
    assert_eq!(sends.get(), 0);
    assert!(!ui.get_message_attach_menu());
    assert_eq!(ui.get_message_body(), "draft");
    ui.set_message_target_ready(true);
    window.draw_if_needed(|renderer| { renderer.render(&mut vec![slint::Rgb8Pixel::default(); 414 * 736], 414); });
    let position = LogicalPosition::new(360., 670.);
    window.window().dispatch_event(WindowEvent::PointerPressed { position, button: PointerEventButton::Left });
    window.window().dispatch_event(WindowEvent::PointerReleased { position, button: PointerEventButton::Left });
    assert_eq!(sends.get(), 1, "positive control must reach the send button");
}
