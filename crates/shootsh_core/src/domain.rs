use std::time::Instant;

pub const MAX_PLAYER_NAME_LEN: usize = 15;
pub const PLAYING_TIME_SEC: u16 = 15;

#[derive(PartialEq, Clone, Copy, Debug, Default)]
pub struct Size {
    pub width: u16,
    pub height: u16,
}

#[derive(PartialEq, Clone, Copy, Debug, Default)]
pub struct Point {
    pub x: u16,
    pub y: u16,
}

#[derive(Clone, Copy, Debug)]
pub struct MouseTrace {
    pub pos: Point,
    pub time: Instant,
}

impl MouseTrace {
    pub fn new(x: u16, y: u16) -> Self {
        Self {
            pos: Point { x, y },
            time: Instant::now(),
        }
    }
}

#[derive(PartialEq, Clone)]
pub struct Target {
    pub pos: Point,
    pub visual_width: u16,
    pub hit_margin: u16,
}

impl Target {
    const DEFAULT_VISUAL_WIDTH: u16 = 2;
    const DEFAULT_HIT_MARGIN: u16 = 1;
    const MIN_X_PADDING: u16 = 2;
    const MIN_Y_PADDING: u16 = 2;

    pub fn new_random(width: u16, height: u16) -> Self {
        use rand::Rng;
        let mut rng = rand::rng();

        let total_width = Self::DEFAULT_VISUAL_WIDTH + (Self::DEFAULT_HIT_MARGIN * 2);

        if width <= total_width + Self::MIN_X_PADDING || height <= Self::MIN_Y_PADDING * 2 {
            return Self::fallback();
        }

        Self {
            pos: Point {
                x: rng.random_range(Self::MIN_X_PADDING..width - total_width),
                y: rng.random_range(Self::MIN_Y_PADDING..height - Self::MIN_Y_PADDING),
            },
            visual_width: Self::DEFAULT_VISUAL_WIDTH,
            hit_margin: Self::DEFAULT_HIT_MARGIN,
        }
    }

    fn fallback() -> Self {
        Self {
            pos: Point { x: 0, y: 0 },
            visual_width: Self::DEFAULT_VISUAL_WIDTH,
            hit_margin: Self::DEFAULT_HIT_MARGIN,
        }
    }

    pub fn is_hit(&self, x: u16, y: u16) -> bool {
        if y != self.pos.y {
            return false;
        }

        let left_edge = self.pos.x.saturating_sub(self.hit_margin);
        let right_edge = self
            .pos
            .x
            .saturating_add(self.visual_width)
            .saturating_add(self.hit_margin);

        x >= left_edge && x < right_edge
    }
}

pub fn format_player_name(name: &str) -> String {
    let cleaned = name
        .trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_PLAYER_NAME_LEN)
        .collect::<String>();

    if cleaned.is_empty() {
        "Anonymous".to_string()
    } else {
        cleaned
    }
}
