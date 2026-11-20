//! Decide when a deferred chat tap becomes a vertical scroll gesture.
pub fn starts_chat_scroll(
    x: f32,
    y: f32,
    to_x: f32,
    to_y: f32,
    width: f32,
    height: f32,
    top_inset: f32,
    keyboard_travel: f32,
) -> bool {
    let dx = to_x - x;
    let dy = to_y - y;
    let in_thread = x > 36.0
        && x < width - 36.0
        && y >= top_inset + 240.0
        && y < height - 150.0 - keyboard_travel;
    let reserved_for_home = dy < 0.0 && y >= height * 0.75;
    in_thread && !reserved_for_home && dy.abs() >= 8.0 && dy.abs() > dx.abs()
}
