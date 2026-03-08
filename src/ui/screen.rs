use crossterm::event::{KeyEvent};
use ratatui::Frame;
use ratatui::layout::Rect;

pub trait Screen {
    fn render(&mut self, frame: &mut Frame, area: Rect);
    fn handle_input(&mut self, key: KeyEvent) -> Option<ScreenAction>;
}

pub enum ScreenAction {
    Switch(Box<dyn Screen>),
    UpdateAndExit,
    Exit,
}