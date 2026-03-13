use crate::screens::menu::MenuScreen;
use crate::screens::update::UpdateScreen;
use crate::ui::navigation::NavigatableList;
use crate::ui::screen::{Screen, ScreenAction};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, List, ListItem};
use ratatui::Frame;

pub struct SystemScreen {
    pub title: String,
    pub list: NavigatableList,
}

impl SystemScreen {
    pub fn new() -> Self {
        let mut list = NavigatableList {
            state: ratatui::widgets::ListState::default(),
            options: vec![
                "Update".to_string(),
                "Back".to_string(),
            ],
        };
        list.state.select(Some(0));

        SystemScreen {
            title: "System Menu".to_string(),
            list,
        }
    }
}

impl Screen for SystemScreen {
    fn handle_input(&mut self, key: KeyEvent) -> Option<ScreenAction> {
        match key.code {
            KeyCode::Down => {
                self.list.next();
                None
            }
            KeyCode::Up => {
                self.list.previous();
                None
            }
            KeyCode::Enter => {
                let selected = self.list.state.selected().unwrap_or(0);
                match self.list.options[selected].as_str() {
                    "Update" => Some(ScreenAction::Switch(Box::new(UpdateScreen::new()))),
                    "Back" => Some(ScreenAction::Switch(Box::new(MenuScreen::new()))),
                    _ => None,
                }
            }
            KeyCode::Esc | KeyCode::Char('b') => Some(ScreenAction::Switch(Box::new(MenuScreen::new()))),
            _ => None,
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self
            .list
            .options
            .iter()
            .map(|label| ListItem::new(format!("  {}", label)))
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .title(format!(" {} ", self.title))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Yellow))
                    .style(Style::default().bg(Color::Black)),
            )
            .highlight_style(
                Style::default()
                    .bg(Color::DarkGray)
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol(">>");

        frame.render_stateful_widget(list, area, &mut self.list.state);
    }
}
