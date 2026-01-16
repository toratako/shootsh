use crate::domain::{MouseTrace, Point};
use std::time::{Duration, Instant};

pub struct AntiCheatConfig {
    pub min_reaction_time: Duration,
    pub max_pixels_per_ms: f64,
    pub min_variance: f64,
}

impl Default for AntiCheatConfig {
    fn default() -> Self {
        Self {
            min_reaction_time: Duration::from_millis(100),
            max_pixels_per_ms: 6.0,
            min_variance: 0.001,
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

        // check reaction speed
        if last_trace.time.duration_since(spawn_time) < self.config.min_reaction_time {
            return false;
        }

        // check warp
        if last_trace.pos != click_pos {
            return false;
        }

        // check speed
        if 2 <= history.len() && !self.has_plausible_speed(history) {
            return false;
        }

        // check jitter
        if 4 <= history.len() && !self.has_human_jitter(history) {
            return false;
        }

        true
    }

    fn has_plausible_speed(&self, history: &[MouseTrace]) -> bool {
        history.windows(2).all(|w| {
            let (p1, p2) = (w[0], w[1]);
            let dist = self.calculate_distance(p1.pos, p2.pos);
            let duration = p2.time.duration_since(p1.time).as_millis() as f64;

            duration <= 0.0 || (dist / duration) <= self.config.max_pixels_per_ms
        })
    }

    fn has_human_jitter(&self, history: &[MouseTrace]) -> bool {
        if history.len() < 4 {
            return true;
        }

        let mut speeds = Vec::new();
        for window in history.windows(2) {
            let dist = self.calculate_distance(window[0].pos, window[1].pos);
            let duration = window[1].time.duration_since(window[0].time).as_secs_f64();
            if duration > 0.0 {
                speeds.push(dist / duration);
            }
        }

        if speeds.len() < 2 {
            return true;
        }

        let accelerations: Vec<f64> = speeds.windows(2).map(|w| (w[1] - w[0]).abs()).collect();
        let count = accelerations.len() as f64;
        let sum: f64 = accelerations.iter().sum();
        let mean = sum / count;

        let variance: f64 = accelerations
            .iter()
            .map(|a| (a - mean).powi(2))
            .sum::<f64>()
            / count;

        variance > self.config.min_variance
    }

    fn calculate_distance(&self, p1: Point, p2: Point) -> f64 {
        let dx = (p2.x as f64 - p1.x as f64).abs();
        let dy = (p2.y as f64 - p1.y as f64).abs();
        (dx.powi(2) + dy.powi(2)).sqrt()
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

    #[test]
    fn test_perfectly_linear_movement_should_fail() {
        let v = InteractionValidator::new(AntiCheatConfig::default());
        let spawn = Instant::now();

        let mut history = Vec::new();
        for i in 0..10 {
            history.push(MouseTrace {
                pos: Point { x: i * 10, y: 10 },
                time: spawn + Duration::from_millis((200 + i * 10).into()),
            });
        }

        assert!(!v.is_legit_interaction(&history, spawn, Point { x: 90, y: 10 }),);
    }

    #[test]
    fn test_perfect_mechanical_zigzag_should_fail() {
        let v = InteractionValidator::new(AntiCheatConfig::default());
        let spawn = Instant::now();
        let mut history = Vec::new();

        for i in 0..10 {
            let y = if i % 2 == 0 { 10 } else { 15 };
            history.push(MouseTrace {
                pos: Point { x: i * 10, y },
                time: spawn + Duration::from_millis((200 + i * 10).into()),
            });
        }

        let last_pos = history.last().unwrap().pos;

        assert!(!v.is_legit_interaction(&history, spawn, last_pos),);
    }

    #[test]
    fn test_human_like_zigzag_should_pass() {
        let v = InteractionValidator::new(AntiCheatConfig::default());
        let spawn = Instant::now();
        let mut history = Vec::new();

        let offsets = [(0, 0), (10, 2), (21, 0), (29, 3), (41, 1)];
        let times = [200, 212, 225, 233, 250];

        for (i, (x, y)) in offsets.iter().enumerate() {
            history.push(MouseTrace {
                pos: Point { x: *x, y: *y },
                time: spawn + Duration::from_millis(times[i]),
            });
        }

        let last_pos = history.last().unwrap().pos;

        assert!(v.is_legit_interaction(&history, spawn, last_pos),);
    }
}
