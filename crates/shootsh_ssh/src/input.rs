use shootsh_core::Action;
use termwiz::input::{InputEvent, InputParser, KeyCode, MouseButtons};

pub struct InputTransformer {
    parser: InputParser,
    last_mouse_buttons: MouseButtons,
}

impl InputTransformer {
    pub fn new() -> Self {
        Self {
            parser: InputParser::new(),
            last_mouse_buttons: MouseButtons::NONE,
        }
    }

    pub fn handle_input(&mut self, data: &[u8]) -> Vec<Action> {
        let mut actions = Vec::new();
        let mut last_buttons = self.last_mouse_buttons.clone();

        self.parser.parse(
            data,
            |event| {
                if let Some(action) = Self::static_map_event(event, &mut last_buttons) {
                    actions.push(action);
                }
            },
            false,
        );

        self.last_mouse_buttons = last_buttons;
        actions
    }

    fn static_map_event(event: InputEvent, last_buttons: &mut MouseButtons) -> Option<Action> {
        match event {
            InputEvent::Key(k) => {
                let is_ctrl = k.modifiers.contains(termwiz::input::Modifiers::CTRL);

                match k.key {
                    KeyCode::Char('c') if is_ctrl => Some(Action::Quit),
                    KeyCode::Char('d') if is_ctrl => Some(Action::Quit),
                    KeyCode::Char('k') if is_ctrl => Some(Action::RequestReset),

                    KeyCode::Char('y') => Some(Action::ConfirmReset),
                    KeyCode::Char('n') => Some(Action::CancelReset),

                    KeyCode::Char('r') => Some(Action::Restart),
                    KeyCode::Char('q') => Some(Action::Quit),

                    KeyCode::Enter => Some(Action::SubmitName),
                    KeyCode::Backspace => Some(Action::DeleteChar),
                    KeyCode::Escape => Some(Action::BackToMenu),
                    KeyCode::Char(c) => Some(Action::InputChar(c)),
                    _ => None,
                }
            }
            InputEvent::Mouse(m) => {
                let x = m.x.saturating_sub(1);
                let y = m.y.saturating_sub(1);

                let was_pressed = last_buttons.contains(MouseButtons::LEFT);
                let is_pressed = m.mouse_buttons.contains(MouseButtons::LEFT);

                *last_buttons = m.mouse_buttons;

                if is_pressed && !was_pressed {
                    return Some(Action::MouseClick(x, y));
                }
                Some(Action::MouseMove(x, y))
            }
            _ => None,
        }
    }
}
