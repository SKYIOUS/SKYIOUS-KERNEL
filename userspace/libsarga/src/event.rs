#[derive(Clone, Copy, Debug)]
pub enum Event {
    KeyDown(u16),
    KeyUp(u16),
    MouseMove(i32, i32),
    MouseDown(i32, i32, u16),
    MouseUp(i32, i32, u16),
    Tick,
}

#[derive(Clone, Copy, Debug)]
pub struct MouseState {
    pub x: i32,
    pub y: i32,
    pub left: bool,
    pub right: bool,
    pub middle: bool,
}

impl MouseState {
    pub fn new() -> Self {
        MouseState { x: 0, y: 0, left: false, right: false, middle: false }
    }
}
