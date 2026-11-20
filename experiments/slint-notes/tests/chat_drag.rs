#[path = "../src/chat_drag.rs"]
mod chat_drag;
use chat_drag::starts_chat_scroll;

#[test]
fn vertical_drag_in_chat_starts_scrolling() {
    assert!(starts_chat_scroll(
        353.0, 273.0, 353.0, 320.0, 414.0, 897.0, 0.0, 0.0
    ));
    assert!(starts_chat_scroll(
        200.0, 500.0, 201.0, 470.0, 414.0, 897.0, 0.0, 0.0
    ));
}

#[test]
fn taps_horizontal_navigation_and_edge_gestures_stay_deferred() {
    assert!(!starts_chat_scroll(
        200.0, 300.0, 201.0, 304.0, 414.0, 897.0, 0.0, 0.0
    ));
    assert!(!starts_chat_scroll(
        200.0, 300.0, 250.0, 303.0, 414.0, 897.0, 0.0, 0.0
    ));
    assert!(!starts_chat_scroll(
        10.0, 300.0, 10.0, 350.0, 414.0, 897.0, 0.0, 0.0
    ));
    assert!(!starts_chat_scroll(
        200.0, 700.0, 200.0, 660.0, 414.0, 897.0, 0.0, 0.0
    ));
    assert!(!starts_chat_scroll(
        200.0, 120.0, 200.0, 180.0, 414.0, 897.0, 0.0, 0.0
    ));
}

#[test]
fn keyboard_area_does_not_start_a_chat_scroll() {
    assert!(!starts_chat_scroll(
        200.0, 580.0, 200.0, 620.0, 414.0, 897.0, 0.0, 278.0
    ));
    assert!(starts_chat_scroll(
        200.0, 300.0, 200.0, 340.0, 414.0, 897.0, 0.0, 278.0
    ));
}
