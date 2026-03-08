use std::io;

use ratatui::{backend::CrosstermBackend, Terminal};

use crossterm::{
    event::{self, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};

mod persistence;
mod screens;
mod system;
mod ui;

use screens::menu::MenuScreen;
use ui::screen::{Screen, ScreenAction};

fn main() -> Result<(), io::Error> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut current_screen: Box<dyn Screen> = Box::new(MenuScreen::new());

    loop {
        terminal.draw(|f| {
            f.render_widget(ratatui::widgets::Clear, f.area());
            current_screen.render(f, f.area());
        })?;

        if event::poll(std::time::Duration::from_millis(16))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == event::KeyEventKind::Press {
                    match current_screen.handle_input(key) {
                        Some(ScreenAction::Exit) => break,
                        Some(ScreenAction::UpdateAndExit) => {
                            // Restore terminal before running update
                            disable_raw_mode()?;
                            execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
                            
                            println!("Starting update process...");
                            if let Err(e) = crate::system::update::perform_update() {
                                eprintln!("Update failed: {}", e);
                            }
                            return Ok(());
                        }
                        Some(ScreenAction::Switch(next_screen)) => {
                            current_screen = next_screen;
                        }
                        None => {}
                    }
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}
