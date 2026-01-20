use crate::db::{DbCache, DbRequest, UserContext};
use crate::domain::{
    CombatStats, MAX_PLAYER_NAME_LEN, MouseTrace, PLAYING_TIME_SEC, Point, Size, Target,
};
use crate::validator::InteractionValidator;
use anyhow::Result;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

pub const RANKING_LIMIT: u32 = 10;

#[derive(Clone)]
pub struct PlayingState {
    pub target: Target,
    pub combat_stats: CombatStats,
    pub mouse_history: VecDeque<MouseTrace>,
    pub last_target_spawn: Instant,
    pub scene_start: Instant,
}

#[derive(Clone, PartialEq)]
pub enum Scene {
    Naming(String),
    Menu,
    Playing(Box<PlayingState>),
    GameOver {
        final_score: u32,
        is_new_record: bool,
    },
}

impl PartialEq for PlayingState {
    fn eq(&self, other: &Self) -> bool {
        self.target == other.target
            && self.combat_stats.current_score() == other.combat_stats.current_score()
    }
}

pub struct App {
    pub user: UserContext,
    pub scene: Scene,
    pub db_cache: Arc<DbCache>,
    pub db_tx: mpsc::Sender<DbRequest>,
    pub mouse_pos: Point,
    pub screen_size: Size,
    pub last_scene_change: Instant,
    pub should_quit: bool,
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
    pub fn new(user: UserContext, db_tx: mpsc::Sender<DbRequest>, db_cache: Arc<DbCache>) -> Self {
        let initial_scene = if user.name.is_empty() || user.name == "NewPlayer" {
            Scene::Naming(String::new())
        } else {
            Scene::Menu
        };

        Self {
            user,
            scene: initial_scene,
            db_cache,
            mouse_pos: Point { x: 0, y: 0 },
            screen_size: Size::default(),
            last_scene_change: Instant::now(),
            should_quit: false,
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

    pub fn change_scene(&mut self, new_scene: Scene) {
        self.scene = new_scene;
        self.last_scene_change = Instant::now();
    }

    fn start_game(&mut self) {
        let state = PlayingState {
            target: Target::new_random(self.screen_size),
            combat_stats: CombatStats::new(),
            mouse_history: VecDeque::from([MouseTrace::new(self.mouse_pos.x, self.mouse_pos.y)]),
            last_target_spawn: Instant::now(),
            scene_start: Instant::now(),
        };
        self.change_scene(Scene::Playing(Box::new(state)));
    }

    fn end_game(&mut self, stats: CombatStats) -> Result<()> {
        let final_score = stats.current_score();

        let _ = self.db_tx.try_send(DbRequest::SaveGame {
            user_id: self.user.id,
            score: final_score,
            hits: stats.hit_count,
            misses: stats.miss_count,
        });

        let is_new_record = final_score > self.user.high_score;
        if is_new_record {
            self.user.high_score = final_score;
        }

        self.change_scene(Scene::GameOver {
            final_score,
            is_new_record,
        });

        Ok(())
    }

    fn handle_tick(&mut self) -> Result<()> {
        if self
            .last_cheat_warning
            .map_or(false, |t| t.elapsed() >= Duration::from_secs(2))
        {
            self.last_cheat_warning = None;
        }

        if let Scene::Playing(state) = &mut self.scene {
            if state.scene_start.elapsed() >= Duration::from_secs(PLAYING_TIME_SEC.into()) {
                let stats = state.combat_stats.clone();
                return self.end_game(stats);
            }

            if state
                .target
                .is_expired(state.last_target_spawn.elapsed(), &state.combat_stats)
            {
                state.combat_stats.register_miss();
                state.target = Target::new_random(self.screen_size);
                state.last_target_spawn = Instant::now();
                state.mouse_history.clear();
            }
        }
        Ok(())
    }

    fn handle_mouse_move(&mut self, x: u16, y: u16) {
        self.mouse_pos = Point { x, y };

        if let Scene::Playing(state) = &mut self.scene {
            state.mouse_history.push_back(MouseTrace::new(x, y));
            if state.mouse_history.len() > 50 {
                state.mouse_history.pop_front();
            }
        }
    }

    fn handle_hit(&mut self, x: u16, y: u16) -> Result<()> {
        match &mut self.scene {
            Scene::Menu => self.start_game(),
            Scene::Playing(state) => {
                state.mouse_history.push_back(MouseTrace::new(x, y));

                if !state.target.is_hit(x, y) {
                    state.combat_stats.register_miss();
                    return Ok(());
                }

                let is_legit = self.validator.is_legit_interaction(
                    &state.mouse_history,
                    state.last_target_spawn,
                    Point { x, y },
                );

                if is_legit {
                    state.combat_stats.register_hit();
                    state.target = Target::new_random(self.screen_size);
                    state.last_target_spawn = Instant::now();
                    state.mouse_history.clear();
                } else {
                    state.combat_stats.register_miss();
                    self.last_cheat_warning = Some(Instant::now());
                    state.mouse_history.clear();
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
        match &mut self.scene {
            Scene::Naming(name) => {
                if name.chars().count() < MAX_PLAYER_NAME_LEN {
                    name.push(c);
                }
            }
            Scene::Playing(_) if c == 'r' => self.start_game(),
            _ if c == 'q' => self.should_quit = true,
            _ => {}
        }
    }

    fn handle_delete_char(&mut self) {
        if let Scene::Naming(name) = &mut self.scene {
            name.pop();
        }
    }

    fn handle_submit_name(&mut self) {
        if let Scene::Naming(name) = &mut self.scene {
            let trimmed = name.trim().to_string();
            if !trimmed.is_empty() {
                let _ = self.db_tx.try_send(DbRequest::UpdateUsername {
                    user_id: self.user.id,
                    new_name: trimmed,
                });

                self.change_scene(Scene::Menu);
            }
        }
    }
}
