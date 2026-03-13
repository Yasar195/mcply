use crate::screens::system::SystemScreen;
use crate::system::update::{self, UpdateInfo};
use crate::ui::screen::{Screen, ScreenAction};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use std::sync::{Arc, Mutex};
use std::thread;

pub struct UpdateScreen {
    pub update_info: Arc<Mutex<Option<Result<UpdateInfo, String>>>>,
    pub is_checking: Arc<Mutex<bool>>,
}

impl UpdateScreen {
    pub fn new() -> Self {
        let update_info = Arc::new(Mutex::new(None));
        let is_checking = Arc::new(Mutex::new(true));

        let info_clone = Arc::clone(&update_info);
        let checking_clone = Arc::clone(&is_checking);

        thread::spawn(move || {
            let result = update::check_for_updates().map_err(|e| e.to_string());
            *info_clone.lock().unwrap() = Some(result);
            *checking_clone.lock().unwrap() = false;
        });

        UpdateScreen {
            update_info,
            is_checking,
        }
    }
}

impl Screen for UpdateScreen {
    fn handle_input(&mut self, key: KeyEvent) -> Option<ScreenAction> {
        match key.code {
            KeyCode::Char('u') | KeyCode::Enter => {
                let info = self.update_info.lock().unwrap();
                if let Some(Ok(ref update)) = *info {
                    if update.update_available {
                        return Some(ScreenAction::UpdateAndExit);
                    }
                }
                None
            }
            KeyCode::Esc | KeyCode::Char('b') => Some(ScreenAction::Switch(Box::new(SystemScreen::new()))),
            _ => None,
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        let checking = *self.is_checking.lock().unwrap();
        let info_lock = self.update_info.lock().unwrap();

        let mut status_text = Vec::new();
        let mut update_available = false;

        if checking {
            status_text.push("Checking for updates...".to_string());
        } else if let Some(ref result) = *info_lock {
            match result {
                Ok(info) => {
                    status_text.push(format!("Current Version: {}", info.current_version));
                    status_text.push(format!("Latest Version:  {}", info.latest_version));
                    status_text.push(format!("Release Date:    {}", info.release_date));
                    status_text.push("".to_string());

                    if info.update_available {
                        status_text.push("A new version is available!".to_string());
                        status_text.push("Press 'u' or Enter to update and restart.".to_string());
                        update_available = true;
                    } else {
                        status_text.push("You are up to date.".to_string());
                    }
                }
                Err(e) => {
                    status_text.push(format!("Error checking for updates: {}", e));
                }
            }
        }

        status_text.push("".to_string());
        status_text.push("Press 'b' or Esc to go back.".to_string());

        let content = status_text.join("\n");
        let paragraph = Paragraph::new(content)
            .block(
                Block::default()
                    .title(" Software Update ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(if update_available { Color::Green } else { Color::Cyan })),
            )
            .alignment(Alignment::Center);

        frame.render_widget(paragraph, area);
    }
}
