#![cfg(feature = "full-suite-ui")]

use personal_os_slint_notes_spike::NotesWindow;
use slint::{ComponentHandle, LogicalPosition, ModelRc, PhysicalSize, SharedString, VecModel};
use slint::platform::{PointerEventButton, WindowAdapter, WindowEvent};
mod platform;

#[test]
fn home_card_and_chat_row_highlight_before_opening() {
    let window = platform::install();
    window.set_size(PhysicalSize::new(414, 897));
    let ui = NotesWindow::new().unwrap();
    ui.set_message_service_ready(true);
    ui.set_message_chats(ModelRc::new(VecModel::from(vec![SharedString::from("Example")])));
    ui.set_message_chat_index_map(ModelRc::new(VecModel::from(vec![0])));
    ui.on_open_chat(|_| true);
    ui.show().unwrap();

    let draw = |x: usize, y: usize| {
        window.window().request_redraw();
        let mut pixels = vec![slint::Rgb8Pixel::default(); 414 * 897];
        assert!(window.draw_if_needed(|renderer| { renderer.render(&mut pixels, 414); }));
        let p = pixels[y * 414 + x];
        (p.r, p.g, p.b)
    };
    let home_position = LogicalPosition::new(170.0, 175.0);
    let home_before = draw(170, 175);
    window.window().dispatch_event(WindowEvent::PointerMoved { position: home_position });
    window.window().dispatch_event(WindowEvent::PointerPressed { position: home_position, button: PointerEventButton::Left });
    assert_ne!(draw(170, 175), home_before, "pressed app card must visibly highlight before the click");
    assert_eq!(ui.get_page(), 0);
    window.window().dispatch_event(WindowEvent::PointerExited);
    window.window().dispatch_event(WindowEvent::PointerReleased { position: LogicalPosition::new(-1.0, -1.0), button: PointerEventButton::Left });
    assert_eq!(ui.get_page(), 0, "cancelling a gesture must not open the app");
    window.window().dispatch_event(WindowEvent::PointerMoved { position: home_position });
    window.window().dispatch_event(WindowEvent::PointerPressed { position: home_position, button: PointerEventButton::Left });
    window.window().dispatch_event(WindowEvent::PointerReleased { position: home_position, button: PointerEventButton::Left });
    assert_eq!(ui.get_page(), 6);

    let before = draw(350, 280);
    let position = LogicalPosition::new(350.0, 280.0);
    window.window().dispatch_event(WindowEvent::PointerMoved { position });
    window.window().dispatch_event(WindowEvent::PointerPressed { position, button: PointerEventButton::Left });
    assert_ne!(draw(350, 280), before, "pressed row must visibly highlight before the click");
    assert!(!ui.get_message_view());
    window.window().dispatch_event(WindowEvent::PointerReleased { position, button: PointerEventButton::Left });
    assert!(ui.get_message_view());
}
