use crate::domain::{MouseTrace, Point};
use std::time::{Duration, Instant};

pub struct AntiCheatConfig {
    pub min_reaction_time: Duration,
    pub max_pixels_per_ms: f64,
}

impl Default for AntiCheatConfig {
    fn default() -> Self {
        Self {
            min_reaction_time: Duration::from_millis(100),
            max_pixels_per_ms: 6.0,
        }
    }
}

pub struct InteractionValidator {
    config: AntiCheatConfig,
}

impl InteractionValidator {
    pub fn new(config: AntiCheatConfig) -> Self {
        Self { config }
    }

    pub fn is_legit_interaction(
        &self,
        history: &[MouseTrace],
        spawn_time: Instant,
        click_pos: Point,
    ) -> bool {
        let last_trace = match history.last() {
            Some(t) => t,
            None => return false,
        };

        // check warp
        if last_trace.pos != click_pos {
            return false;
        }

        // check reaction speed
        if last_trace.time.duration_since(spawn_time) < self.config.min_reaction_time {
            return false;
        }

        // check speed
        if !self.has_plausible_speed(history) {
            return false;
        }

        true
    }

    fn has_plausible_speed(&self, history: &[MouseTrace]) -> bool {
        if history.len() < 2 {
            return true;
        }
        let max_px_per_ms_sq = self.config.max_pixels_per_ms.powi(2);

        history.windows(2).all(|w| {
            let dx = w[1].pos.x as f64 - w[0].pos.x as f64;
            let dy = w[1].pos.y as f64 - w[0].pos.y as f64;
            let dist_sq = dx.powi(2) + dy.powi(2);

            let dt = w[1].time.duration_since(w[0].time).as_millis() as f64;

            dt <= 0.0 || dist_sq <= max_px_per_ms_sq * dt.powi(2)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_human_reaction_speed() {
        let v = InteractionValidator::new(AntiCheatConfig::default());
        let spawn = Instant::now();
        let history = vec![MouseTrace {
            pos: Point { x: 1, y: 1 },
            time: spawn + Duration::from_millis(200),
        }];
        assert!(v.is_legit_interaction(&history, spawn, Point { x: 1, y: 1 }));
    }

    #[test]
    fn test_bot_reaction_speed() {
        let v = InteractionValidator::new(AntiCheatConfig::default());
        let spawn = Instant::now();
        let history = vec![MouseTrace {
            pos: Point { x: 1, y: 1 },
            time: spawn + Duration::from_millis(50),
        }];
        assert!(!v.is_legit_interaction(&history, spawn, Point { x: 1, y: 1 }));
    }

    #[test]
    fn test_warp_detection_should_fail() {
        let v = InteractionValidator::new(AntiCheatConfig::default());
        let spawn = Instant::now();

        let history = vec![MouseTrace {
            pos: Point { x: 10, y: 10 },
            time: spawn + Duration::from_millis(200),
        }];

        // warp from (10, 10) to (50, 50)
        let click_pos = Point { x: 50, y: 50 };

        assert!(!v.is_legit_interaction(&history, spawn, click_pos),);
    }

    #[test]
    fn test_empty_history_should_fail() {
        let v = InteractionValidator::new(AntiCheatConfig::default());
        let spawn = Instant::now();
        let history = vec![];
        let click_pos = Point { x: 10, y: 10 };

        assert!(!v.is_legit_interaction(&history, spawn, click_pos),);
    }
}
