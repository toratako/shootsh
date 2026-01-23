use shootsh_core::Action;
use termwiz::input::{InputEvent, InputParser, KeyCode, Modifiers, MouseButtons};

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

    pub fn handle_input(&mut self, data: &[u8]) -> Vec<(InputEvent, MouseButtons)> {
        let mut results = Vec::new();
        self.parser.parse(
            data,
            |event| {
                results.push((event.clone(), self.last_mouse_buttons.clone()));

                if let InputEvent::Mouse(m) = &event {
                    self.last_mouse_buttons = m.mouse_buttons.clone();
                }
            },
            false,
        );
        results
    }
}

pub fn map_input_to_action(
    event: InputEvent,
    captured: bool,
    last_mouse_buttons: &MouseButtons,
) -> Option<Action> {
    match event {
        InputEvent::Key(k) => {
            let is_ctrl = k.modifiers.contains(Modifiers::CTRL);
            if is_ctrl {
                return match k.key {
                    KeyCode::Char('c') | KeyCode::Char('d') => Some(Action::Quit),
                    KeyCode::Char('k') => Some(Action::RequestReset),
                    _ => None,
                };
            }

            if captured {
                match k.key {
                    KeyCode::Enter => Some(Action::SubmitInput),
                    KeyCode::Backspace => Some(Action::DeleteCharacter),
                    KeyCode::Escape => Some(Action::BackToMenu),
                    KeyCode::Char(c) => Some(Action::AppendCharacter(c)),
                    _ => None,
                }
            } else {
                match k.key {
                    KeyCode::Char('q') => Some(Action::Quit),
                    KeyCode::Char('r') => Some(Action::Restart),
                    KeyCode::Char('y') => Some(Action::ConfirmReset),
                    KeyCode::Char('n') => Some(Action::CancelReset),
                    KeyCode::Enter => Some(Action::SubmitInput),
                    KeyCode::Backspace => Some(Action::DeleteCharacter),
                    KeyCode::Escape => Some(Action::BackToMenu),
                    KeyCode::Char(c) => Some(Action::AppendCharacter(c)),
                    _ => None,
                }
            }
        }
        InputEvent::Mouse(m) => {
            // 1-index to 0-index
            let x = m.x.saturating_sub(1);
            let y = m.y.saturating_sub(1);
            let was_pressed = last_mouse_buttons.contains(MouseButtons::LEFT);
            let is_pressed = m.mouse_buttons.contains(MouseButtons::LEFT);

            if is_pressed && !was_pressed {
                Some(Action::MouseClick(x, y))
            } else {
                Some(Action::MouseMove(x, y))
            }
        }
        _ => None,
    }
}
