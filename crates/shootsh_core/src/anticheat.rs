use crate::domain::{MouseTrace, Point};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

pub struct AntiCheatConfig {
    pub min_reaction_time: Duration,
}

impl Default for AntiCheatConfig {
    fn default() -> Self {
        Self {
            min_reaction_time: Duration::from_millis(100),
        }
    }
}

pub struct BehaviorAnalyzer {
    config: AntiCheatConfig,
}

impl BehaviorAnalyzer {
    pub fn new(config: AntiCheatConfig) -> Self {
        Self { config }
    }

    pub fn is_legit_interaction(
        &self,
        history: &VecDeque<MouseTrace>,
        spawn_time: Instant,
        _click_pos: Point,
    ) -> bool {
        let last_trace = match history.back() {
            Some(t) => t,
            None => return false,
        };

        // check reaction speed
        if last_trace.time.duration_since(spawn_time) < self.config.min_reaction_time {
            return false;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_human_reaction_speed() {
        let v = BehaviorAnalyzer::new(AntiCheatConfig::default());
        let spawn = Instant::now();
        let history = VecDeque::from(vec![MouseTrace {
            pos: Point { x: 1, y: 1 },
            time: spawn + Duration::from_millis(200),
        }]);
        assert!(v.is_legit_interaction(&history, spawn, Point { x: 1, y: 1 }));
    }

    #[test]
    fn test_bot_reaction_speed() {
        let v = BehaviorAnalyzer::new(AntiCheatConfig::default());
        let spawn = Instant::now();
        let history = VecDeque::from(vec![MouseTrace {
            pos: Point { x: 1, y: 1 },
            time: spawn + Duration::from_millis(50),
        }]);
        assert!(!v.is_legit_interaction(&history, spawn, Point { x: 1, y: 1 }));
    }
}
