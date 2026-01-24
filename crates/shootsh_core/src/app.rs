use crate::anticheat::BehaviorAnalyzer;
use crate::db::{DbCache, DbRequest, UserContext};
use crate::domain::{
    CombatStats, MAX_PLAYER_NAME_LEN, MouseTrace, PLAYING_TIME_SEC, Point, Size, Target,
};
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
pub struct NamingState {
    pub input: String,
    pub error: Option<String>,
    pub is_loading: bool,
}

#[derive(Clone, PartialEq)]
pub enum Scene {
    Naming(NamingState),
    Menu,
    Playing(Box<PlayingState>),
    GameOver {
        final_score: u32,
        is_new_record: bool,
    },
    ResetConfirmation,
}

impl PartialEq for PlayingState {
    fn eq(&self, other: &Self) -> bool {
        self.target == other.target
            && self.combat_stats.current_score() == other.combat_stats.current_score()
    }
}

pub type ActionResult = (
    Result<()>,
    Option<tokio::sync::oneshot::Receiver<Result<(), anyhow::Error>>>,
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaderboardTab {
    Daily,
    Weekly,
    AllTime,
}

impl Default for LeaderboardTab {
    fn default() -> Self {
        Self::AllTime
    }
}

impl LeaderboardTab {
    pub fn next(&self) -> Self {
        match self {
            Self::Daily => Self::Weekly,
            Self::Weekly => Self::AllTime,
            Self::AllTime => Self::Daily,
        }
    }

    pub fn prev(&self) -> Self {
        match self {
            Self::Daily => Self::AllTime,
            Self::Weekly => Self::Daily,
            Self::AllTime => Self::Weekly,
        }
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
    behavior_analyzer: BehaviorAnalyzer,
    pub last_cheat_warning: Option<Instant>,
    pub leaderboard_tab: LeaderboardTab,
}

pub enum Action {
    AppendCharacter(char),
    DeleteCharacter,
    SubmitInput,
    MouseMove(u16, u16),
    MouseClick(u16, u16),
    Quit,
    BackToMenu,
    Tick,
    RequestReset,
    ConfirmReset,
    CancelReset,
    Restart,
    NavigateLeft,
    NavigateRight,
}

impl App {
    pub fn new(user: UserContext, db_tx: mpsc::Sender<DbRequest>, db_cache: Arc<DbCache>) -> Self {
        let initial_scene = if user.name.is_none() {
            Scene::Naming(NamingState {
                input: String::new(),
                error: None,
                is_loading: false,
            })
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
            behavior_analyzer: BehaviorAnalyzer::new(Default::default()),
            last_cheat_warning: None,
            db_tx,
            leaderboard_tab: LeaderboardTab::default(),
        }
    }

    pub fn input_captured(&self) -> bool {
        matches!(self.scene, Scene::Naming(_))
    }

    pub fn update_state(&mut self, action: Action) -> ActionResult {
        match action {
            Action::Restart => {
                if matches!(self.scene, Scene::Playing(_) | Scene::GameOver { .. }) {
                    self.start_game();
                }
                (Ok(()), None)
            }
            Action::Quit => {
                self.should_quit = true;
                (Ok(()), None)
            }
            Action::RequestReset => {
                if matches!(self.scene, Scene::Menu) {
                    self.change_scene(Scene::ResetConfirmation);
                }
                (Ok(()), None)
            }
            Action::ConfirmReset => (Ok(()), self.handle_confirm_reset()),
            Action::CancelReset => {
                if matches!(self.scene, Scene::ResetConfirmation) {
                    self.change_scene(Scene::Menu);
                }
                (Ok(()), None)
            }
            Action::Tick => (self.handle_tick(), None),
            Action::MouseMove(x, y) => {
                self.handle_mouse_move(x, y);
                (Ok(()), None)
            }
            Action::MouseClick(x, y) => (self.handle_click(x, y), None),
            Action::NavigateLeft => {
                self.handle_navigate_left();
                (Ok(()), None)
            }
            Action::NavigateRight => {
                self.handle_navigate_right();
                (Ok(()), None)
            }
            Action::AppendCharacter(c) => (self.handle_append_char(c), None),
            Action::DeleteCharacter => (self.handle_delete_char(), None),
            Action::SubmitInput => (Ok(()), self.handle_submit_name()),
            Action::BackToMenu => {
                self.change_scene(Scene::Menu);
                (Ok(()), None)
            }
        }
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

        // honestly, should wait db response and react.
        // update high score
        let is_new_record = final_score > self.user.high_score;
        if is_new_record {
            self.user.high_score = final_score;
        }

        // update stats
        self.user.total_hits += stats.hit_count;
        self.user.total_misses += stats.miss_count;
        self.user.sessions += 1;

        // update activity
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        if let Some(day) = self.user.user_activity.iter_mut().find(|d| d.date == today) {
            day.count += 1;
        } else {
            self.user.user_activity.insert(
                0,
                crate::db::ActivityDay {
                    date: today,
                    count: 1,
                },
            );
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
            // end game
            if state.scene_start.elapsed() >= Duration::from_secs(PLAYING_TIME_SEC.into()) {
                let stats = state.combat_stats.clone();
                return self.end_game(stats);
            }

            // respawn target
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

    fn handle_click(&mut self, x: u16, y: u16) -> Result<()> {
        match &mut self.scene {
            Scene::Menu => self.start_game(),
            Scene::Playing(state) => {
                state.mouse_history.push_back(MouseTrace::new(x, y));

                if !state.target.is_hit(x, y) {
                    state.combat_stats.register_miss();
                    return Ok(());
                }

                let is_legit = self.behavior_analyzer.is_legit_interaction(
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

    fn handle_append_char(&mut self, c: char) -> Result<()> {
        if let Scene::Naming(state) = &mut self.scene {
            if !state.is_loading
                && c.is_ascii_alphanumeric()
                && state.input.chars().count() < MAX_PLAYER_NAME_LEN
            {
                state.input.push(c);
            }
        }
        Ok(())
    }

    fn handle_delete_char(&mut self) -> Result<()> {
        if let Scene::Naming(state) = &mut self.scene {
            if !state.is_loading {
                state.input.pop();
            }
        }
        Ok(())
    }

    pub fn handle_submit_name(
        &mut self,
    ) -> Option<tokio::sync::oneshot::Receiver<Result<(), anyhow::Error>>> {
        if let Scene::Naming(state) = &mut self.scene {
            if state.is_loading {
                return None;
            }

            let trimmed = state.input.trim().to_string();
            if !trimmed.is_empty() {
                let (tx, rx) = tokio::sync::oneshot::channel();

                state.is_loading = true;
                state.error = None;

                let _ = self.db_tx.try_send(DbRequest::UpdateUsername {
                    user_id: self.user.id,
                    new_name: trimmed,
                    reply_tx: tx,
                });

                return Some(rx);
            }
        }
        None
    }

    fn handle_confirm_reset(
        &mut self,
    ) -> Option<tokio::sync::oneshot::Receiver<Result<(), anyhow::Error>>> {
        let (tx, rx) = tokio::sync::oneshot::channel();

        let send_result = self.db_tx.try_send(DbRequest::DeleteUser {
            user_id: self.user.id,
            reply_tx: tx,
        });

        if send_result.is_err() {
            return None;
        }

        Some(rx)
    }

    fn handle_navigate_left(&mut self) {
        match &self.scene {
            Scene::Menu | Scene::GameOver { .. } => {
                self.leaderboard_tab = self.leaderboard_tab.prev();
            }
            _ => {}
        }
    }

    fn handle_navigate_right(&mut self) {
        match &self.scene {
            Scene::Menu | Scene::GameOver { .. } => {
                self.leaderboard_tab = self.leaderboard_tab.next();
            }
            _ => {}
        }
    }
}
