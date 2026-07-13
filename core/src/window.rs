#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WindowState {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    maximized: bool,
    fullscreen: bool,
}

impl WindowState {
    pub fn new(
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        maximized: bool,
        fullscreen: bool,
    ) -> Option<Self> {
        if width == 0 || height == 0 {
            return None;
        }

        Some(Self {
            x,
            y,
            width,
            height,
            maximized,
            fullscreen,
        })
    }

    pub fn x(&self) -> i32 {
        self.x
    }

    pub fn y(&self) -> i32 {
        self.y
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn is_maximized(&self) -> bool {
        self.maximized
    }

    pub fn is_fullscreen(&self) -> bool {
        self.fullscreen
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_window_dimensions() {
        assert!(WindowState::new(0, 0, 0, 800, false, false).is_none());
        assert!(WindowState::new(0, 0, 1280, 0, false, false).is_none());
    }
}
