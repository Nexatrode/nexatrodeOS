#![cfg(feature = "full-suite-ui")]
use personal_os_slint_notes_spike::NotesWindow;
use slint::{ComponentHandle, Model, ModelRc, ModelTracker, PhysicalSize, SharedString, VecModel};
use slint::platform::{WindowAdapter, WindowEvent};
use std::{cell::RefCell, rc::Rc};
mod platform;

struct CountingRows { visited: Rc<RefCell<Vec<usize>>> }
impl Model for CountingRows {
    type Data = SharedString;
    fn row_count(&self) -> usize { 300 }
    fn row_data(&self, row: usize) -> Option<Self::Data> {
        if row >= 300 { return None; }
        self.visited.borrow_mut().push(row);
        Some(format!("Message {row}").into())
    }
    fn model_tracker(&self) -> &dyn ModelTracker { &() }
}

#[test]
fn opening_a_long_conversation_only_instantiates_nearby_rows() {
    let window = platform::install();
    let ui = NotesWindow::new().unwrap();
    let visited = Rc::new(RefCell::new(Vec::new()));
    ui.set_page(6);
    ui.set_message_service_ready(true);
    ui.set_message_view(true);
    ui.set_message_bubbles(ModelRc::new(CountingRows { visited: visited.clone() }));
    ui.set_message_bubble_side(ModelRc::new(VecModel::from(vec![0; 300])));
    ui.set_message_bubble_time(ModelRc::new(VecModel::from(vec![SharedString::default(); 300])));
    ui.set_message_bubble_photo_ids(ModelRc::new(VecModel::from(
        (0..300).map(|i| SharedString::from(format!("{}", i + 1))).collect::<Vec<_>>()
    )));
    ui.set_message_bubble_photo_pending(ModelRc::new(VecModel::from(vec![true; 300])));
    let visible_photos = Rc::new(RefCell::new(Vec::new()));
    let callbacks = visible_photos.clone();
    ui.on_set_chat_photo_visibility(move |row, visible| {
        if visible { callbacks.borrow_mut().push(row); }
    });
    ui.show().unwrap();
    window.set_size(PhysicalSize::new(414, 736));
    window.draw_if_needed(|r| { r.render(&mut vec![slint::Rgb8Pixel::default(); 414 * 736], 414); });
    let mut indices = visited.borrow().clone();
    indices.sort_unstable(); indices.dedup();
    assert!(indices.len() < 40, "created {} different rows for one viewport", indices.len());
    assert!(indices.iter().any(|&i| i >= 290), "the conversation must open at its newest messages: {indices:?}");
    assert!(visible_photos.borrow().iter().any(|&i| i >= 290), "newest visible photos must be requested");
    // The list may measure row zero before it seeks to the bottom.
    let first_visible = *visible_photos.borrow().iter().filter(|&&i| i >= 280).min().unwrap();
    visible_photos.borrow_mut().clear();
    window.window().dispatch_event(WindowEvent::PointerScrolled {
        position: slint::LogicalPosition::new(200., 450.), delta_x: 0., delta_y: 700.,
    });
    for _ in 0..20 {
        std::thread::sleep(std::time::Duration::from_millis(16));
        slint::platform::update_timers_and_animations();
        window.draw_if_needed(|r| { r.render(&mut vec![slint::Rgb8Pixel::default(); 414 * 736], 414); });
    }
    assert!(visible_photos.borrow().iter().any(|&i| i < first_visible), "scrolling must request older photos than {first_visible}: {:?}", visible_photos.borrow());
}
