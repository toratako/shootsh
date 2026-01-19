use std::time::{Duration, Instant};

pub const MAX_PLAYER_NAME_LEN: usize = 15;
pub const PLAYING_TIME_SEC: u16 = 15;
const BASE_HIT_VALUE: f64 = 100.0;
const COMBO_MULTIPLIER_STEP: f64 = 0.2;
const INITIAL_MULTIPLIER: f64 = 1.0;
const MAX_MULTIPLIER: f64 = 3.0;
const DECAY_RATE: f64 = 0.95;
const MAX_TARGET_LIFETIME_MS: u64 = 2000;

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

#[derive(Debug, Clone)]
pub struct CombatStats {
    score: f64,
    combo: u32,
    pub hit_count: u32,
    pub miss_count: u32,
}

impl CombatStats {
    pub fn new() -> Self {
        Self {
            score: 0.0,
            combo: 0,
            hit_count: 0,
            miss_count: 0,
        }
    }

    /// FinalScore = SUM(HitValue * ComboMultiplier)
    pub fn register_hit(&mut self) {
        self.hit_count += 1;
        self.combo += 1;

        let raw_multiplier = INITIAL_MULTIPLIER + (self.combo as f64 * COMBO_MULTIPLIER_STEP);
        let multiplier = raw_multiplier.min(MAX_MULTIPLIER);

        self.score += BASE_HIT_VALUE * multiplier;
    }

    /// Reset combo
    pub fn register_miss(&mut self) {
        self.combo = 0;
    }

    pub fn current_score(&self) -> u32 {
        self.score as u32
    }

    pub fn current_combo(&self) -> u32 {
        self.combo as u32
    }

    /// T_lifetime = T_max_life * (DecayRate)^Hits
    pub fn get_target_lifetime(&self) -> Duration {
        let decay = DECAY_RATE.powi(self.hit_count as i32);
        let millis = MAX_TARGET_LIFETIME_MS as f64 * decay;
        Duration::from_millis(millis as u64)
    }
}

#[derive(PartialEq, Clone, Debug)]
pub struct Target {
    pub pos: Point,
    pub visual_width: u16,
    pub visual_height: u16,
    pub hit_margin_x: u16,
    pub hit_margin_y: u16,
}

impl Target {
    const DEFAULT_VISUAL_WIDTH: u16 = 4;
    const DEFAULT_VISUAL_HEIGHT: u16 = 2;
    const DEFAULT_HIT_MARGIN_X: u16 = 2;
    const DEFAULT_HIT_MARGIN_Y: u16 = 1;
    const MIN_PADDING: u16 = 2;

    pub fn new_random(screen: Size) -> Self {
        use rand::Rng;
        let mut rng = rand::rng();

        let total_w = Self::DEFAULT_VISUAL_WIDTH;
        let total_h = Self::DEFAULT_VISUAL_HEIGHT;

        if screen.width <= total_w + Self::MIN_PADDING * 2
            || screen.height <= total_h + Self::MIN_PADDING * 2
        {
            return Self::fallback();
        }

        Self {
            pos: Point {
                x: rng.random_range(Self::MIN_PADDING..screen.width - total_w - Self::MIN_PADDING),
                y: rng.random_range(Self::MIN_PADDING..screen.height - total_h - Self::MIN_PADDING),
            },
            visual_width: Self::DEFAULT_VISUAL_WIDTH,
            visual_height: Self::DEFAULT_VISUAL_HEIGHT,
            hit_margin_x: Self::DEFAULT_HIT_MARGIN_X,
            hit_margin_y: Self::DEFAULT_HIT_MARGIN_Y,
        }
    }

    fn fallback() -> Self {
        Self {
            pos: Point { x: 0, y: 0 },
            visual_width: Self::DEFAULT_VISUAL_WIDTH,
            visual_height: Self::DEFAULT_VISUAL_HEIGHT,
            hit_margin_x: Self::DEFAULT_HIT_MARGIN_X,
            hit_margin_y: Self::DEFAULT_HIT_MARGIN_Y,
        }
    }

    pub fn is_hit(&self, x: u16, y: u16) -> bool {
        // Y: (pos.y - margin) to (pos.y + height + margin)
        let top_edge = self.pos.y.saturating_sub(self.hit_margin_y);
        let bottom_edge = self
            .pos
            .y
            .saturating_add(self.visual_height)
            .saturating_add(self.hit_margin_y);

        if y < top_edge || y >= bottom_edge {
            return false;
        }

        // X: (pos.x - margin) to (pos.x + width + margin)
        let left_edge = self.pos.x.saturating_sub(self.hit_margin_x);
        let right_edge = self
            .pos
            .x
            .saturating_add(self.visual_width)
            .saturating_add(self.hit_margin_x);

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
