use crate::db::{DbRequest, ScoreEntry};
use crate::domain::MAX_PLAYER_NAME_LEN;
use crate::domain::{MouseTrace, Point, Size, Target, format_player_name};
use crate::validator::InteractionValidator;
use anyhow::Result;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

pub const PLAYING_TIME: u16 = 15;
pub const RANKING_LIMIT: u32 = 10;

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
    pub ranking_cache: Arc<Mutex<Vec<ScoreEntry>>>,
    pub db_tx: mpsc::Sender<DbRequest>,
    pub high_score: u32,
    pub current_score: u32,
    pub mouse_pos: Point,
    pub screen_size: Size,
    pub last_scene_change: Instant,
    pub should_quit: bool,
    pub mouse_history: VecDeque<MouseTrace>,
    pub last_target_spawn: Instant,
    validator: InteractionValidator,
    pub last_cheat_warning: Option<Instant>,
}

pub enum Action {
    InputChar(char),
    DeleteChar,
    SubmitName,
    MouseMove(u16, u16),
    MouseClick(u16, u16),
    Quit,
    BackToMenu,
    Tick,
}

impl App {
    pub fn new(db_tx: mpsc::Sender<DbRequest>, ranking_cache: Arc<Mutex<Vec<ScoreEntry>>>) -> Self {
        Self {
            scene: Scene::Naming,
            player_name: String::new(),
            ranking_cache,
            high_score: 0,
            current_score: 0,
            mouse_pos: Point { x: 0, y: 0 },
            screen_size: Size {
                width: 0,
                height: 0,
            },
            last_scene_change: Instant::now(),
            should_quit: false,
            mouse_history: VecDeque::with_capacity(51),
            last_target_spawn: Instant::now(),
            validator: InteractionValidator::new(Default::default()),
            last_cheat_warning: None,
            db_tx,
        }
    }

    pub fn update_state(&mut self, action: Action) -> Result<()> {
        match action {
            Action::Quit => self.should_quit = true,
            Action::Tick => self.handle_tick()?,
            Action::MouseMove(x, y) => self.handle_mouse_move(x, y),
            Action::MouseClick(x, y) => self.handle_hit(x, y)?,
            Action::InputChar(c) => self.handle_input_char(c),
            Action::DeleteChar => self.handle_delete_char(),
            Action::SubmitName => self.handle_submit_name(),
            Action::BackToMenu => self.change_scene(Scene::Menu),
        }
        Ok(())
    }

    fn prepare_for_next_target(&mut self) {
        self.last_target_spawn = Instant::now();
        self.mouse_history.clear();
        self.mouse_history
            .push_back(MouseTrace::new(self.mouse_pos.x, self.mouse_pos.y));
    }

    pub fn change_scene(&mut self, new_scene: Scene) {
        self.scene = new_scene;
        self.last_scene_change = Instant::now();
    }

    fn start_game(&mut self) {
        self.current_score = 0;
        let target = Target::new_random(self.screen_size.width, self.screen_size.height);
        self.change_scene(Scene::Playing { target });
        self.prepare_for_next_target();
    }

    fn end_game(&mut self) -> Result<()> {
        let name = format_player_name(&self.player_name);

        let _ = self.db_tx.try_send(DbRequest::SaveScore {
            name: name.clone(),
            score: self.current_score,
        });

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

    fn handle_tick(&mut self) -> Result<()> {
        if let Some(t) = self.last_cheat_warning {
            if t.elapsed() >= Duration::from_secs(2) {
                self.last_cheat_warning = None;
            }
        }

        if let Scene::Playing { .. } = self.scene {
            if self.last_scene_change.elapsed() >= Duration::from_secs(PLAYING_TIME.into()) {
                self.end_game()?;
            }
        }
        Ok(())
    }

    fn handle_mouse_move(&mut self, x: u16, y: u16) {
        self.mouse_pos = Point { x, y };

        if let Scene::Playing { .. } = self.scene {
            self.mouse_history.push_back(MouseTrace::new(x, y));
            if self.mouse_history.len() > 50 {
                self.mouse_history.pop_front();
            }
        } else {
            if !self.mouse_history.is_empty() {
                self.mouse_history.clear();
            }
        }
    }

    fn handle_hit(&mut self, x: u16, y: u16) -> Result<()> {
        match &mut self.scene {
            Scene::Menu => self.start_game(),
            Scene::Playing { target } => {
                if !target.is_hit(x, y) {
                    return Ok(());
                }

                let is_legit = self.validator.is_legit_interaction(
                    &self.mouse_history.make_contiguous(),
                    self.last_target_spawn,
                    Point { x, y },
                );

                if is_legit {
                    self.current_score += 1;
                    *target = Target::new_random(self.screen_size.width, self.screen_size.height);
                    self.prepare_for_next_target();
                } else {
                    self.last_cheat_warning = Some(Instant::now());
                    self.mouse_history.clear();
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
