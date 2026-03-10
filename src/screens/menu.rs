// use crate::screens::settings::SettingsScreen;
// use crate::screens::tasks::TasksScreen;
use crate::screens::models::ModelsScreen;
use crate::screens::mcp_servers::McpServersScreen;
use crate::screens::chat::ChatScreen;
use crate::ui::navigation::NavigatableList;
use crate::ui::screen::{Screen, ScreenAction};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, List, ListItem};
use ratatui::Frame;

pub struct MenuScreen {
    pub title: String,
    pub list: NavigatableList,
}

impl MenuScreen {
    pub fn new() -> Self {
        let mut list = NavigatableList {
            state: ratatui::widgets::ListState::default(),
            options: vec![
                "Chat".to_string(),
                "Models".to_string(),
                "MCP Servers".to_string(),
                "System".to_string(),
                "Exit".to_string(),
            ],
        };
        list.state.select(Some(0));

        MenuScreen {
            title: "Main Menu - MCPLY".to_string(),
            list,
        }
    }
}

impl Screen for MenuScreen {
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
                    "Chat" => Some(ScreenAction::Switch(Box::new(ChatScreen::new()))),
                    "Models" => Some(ScreenAction::Switch(Box::new(ModelsScreen::new()))),
                    "MCP Servers" => Some(ScreenAction::Switch(Box::new(McpServersScreen::new()))),
                    "Exit" => Some(ScreenAction::Exit),
                    _ => None,
                }
            }
            KeyCode::Char('q') => Some(ScreenAction::Exit),
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
                    .border_style(Style::default().fg(Color::Cyan))
                    .style(Style::default().bg(Color::Black)),
            )
            .highlight_style(
                Style::default()
                    .bg(Color::DarkGray)
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol(">>");

        frame.render_stateful_widget(list, area, &mut self.list.state);
    }
}