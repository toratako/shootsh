use crate::db::Leaderboard;
use crate::db::ScoreEntry;
use crate::domain::MAX_PLAYER_NAME_LEN;
use crate::domain::{Target, format_player_name};
use anyhow::Result;
use std::time::{Duration, Instant};

pub const PLAYING_TIME: u16 = 15;

#[derive(PartialEq, Clone)]
pub enum Scene {
    Naming,
    Menu,
    Playing {
        target: Target,
    },
    GameOver {
        final_score: u32,
        is_new_record: bool,
    },
}

pub struct App {
    pub scene: Scene,
    pub player_name: String,
    pub leaderboard: Leaderboard,
    pub ranking_cache: Vec<ScoreEntry>,
    pub high_score: u32,
    pub current_score: u32,
    pub mouse_pos: (u16, u16),
    pub screen_size: (u16, u16),
    pub last_scene_change: Instant,
    pub should_quit: bool,
}

pub enum Action {
    InputChar(char),
    DeleteChar,
    SubmitName,
    MouseClick(u16, u16),
    Quit,
    BackToMenu,
    Tick,
}

impl App {
    pub fn new(leaderboard: Leaderboard) -> Self {
        let ranking_cache = leaderboard.get_top_10().unwrap_or_default();
        Self {
            scene: Scene::Naming,
            player_name: String::new(),
            leaderboard,
            ranking_cache,
            high_score: 0,
            current_score: 0,
            mouse_pos: (0, 0),
            screen_size: (0, 0),
            last_scene_change: Instant::now(),
            should_quit: false,
        }
    }

    pub fn change_scene(&mut self, new_scene: Scene) {
        self.scene = new_scene;
        self.last_scene_change = Instant::now();
    }
    fn start_game(&mut self) {
        self.current_score = 0;
        let target = self.generate_new_target();
        self.change_scene(Scene::Playing { target });
    }

    fn generate_new_target(&self) -> Target {
        Target::new_random(self.screen_size.0, self.screen_size.1)
    }

    fn end_game(&mut self) -> Result<()> {
        let name = format_player_name(&self.player_name);
        self.leaderboard.save(&name, self.current_score)?;
        self.update_ranking_cache();

        let is_new_record = self.current_score > self.high_score;
        if is_new_record {
            self.high_score = self.current_score;
        }

        self.change_scene(Scene::GameOver {
            final_score: self.current_score,
            is_new_record,
        });
        Ok(())
    }

    pub fn update_ranking_cache(&mut self) {
        if let Ok(scores) = self.leaderboard.get_top_10() {
            self.ranking_cache = scores;
        }
    }

    pub fn update_state(&mut self, action: Action) -> Result<()> {
        match action {
            Action::Quit => self.should_quit = true,
            Action::Tick => self.handle_tick()?,
            Action::MouseClick(x, y) => self.handle_hit(x, y)?,
            Action::InputChar(c) => self.handle_input_char(c),
            Action::DeleteChar => self.handle_delete_char(),
            Action::SubmitName => self.handle_submit_name(),
            Action::BackToMenu => self.change_scene(Scene::Menu),
        }
        Ok(())
    }

    fn handle_tick(&mut self) -> Result<()> {
        if let Scene::Playing { .. } = self.scene {
            if self.last_scene_change.elapsed() >= Duration::from_secs(PLAYING_TIME.into()) {
                self.end_game()?;
            }
        }
        Ok(())
    }

    fn handle_hit(&mut self, x: u16, y: u16) -> Result<()> {
        match &mut self.scene {
            Scene::Menu => {
                self.start_game();
            }
            Scene::Playing { target, .. } => {
                if target.is_hit(x, y) {
                    self.current_score += 1;
                    *target = Target::new_random(self.screen_size.0, self.screen_size.1);
                }
            }
            Scene::GameOver { .. } => {
                if self.last_scene_change.elapsed() >= Duration::from_millis(500) {
                    self.change_scene(Scene::Menu);
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_input_char(&mut self, c: char) {
        match self.scene {
            Scene::Naming => {
                if self.player_name.chars().count() < MAX_PLAYER_NAME_LEN {
                    self.player_name.push(c);
                }
            }
            Scene::Playing { .. } if c == 'r' => {
                self.start_game();
            }
            _ if c == 'q' => {
                self.should_quit = true;
            }
            _ => {}
        }
    }

    fn handle_delete_char(&mut self) {
        if let Scene::Naming = self.scene {
            self.player_name.pop();
        }
    }

    fn handle_submit_name(&mut self) {
        if let Scene::Naming = self.scene {
            if !self.player_name.trim().is_empty() {
                self.change_scene(Scene::Menu);
            }
        }
    }
}
