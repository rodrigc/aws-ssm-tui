mod aws;
mod ui;
mod utils;

use ui::ssm::{run_ssm_app, SSMApp};

use color_eyre::Result;
use ratatui::{
    backend::CrosstermBackend,
    crossterm::terminal::{
        disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
    },
    Terminal,
};
use std::io;

#[tokio::main]
async fn main() -> Result<()> {
    // Install color-eyre hook for nice error reports
    color_eyre::install()?;

    // Initialize tui-logger
    tui_logger::init_logger(tui_logger::LevelFilter::Debug)?;
    tui_logger::set_default_level(tui_logger::LevelFilter::Debug);

    // Setup terminal.
    // Intentionally do NOT enable mouse capture: this app is keyboard-only, and
    // mouse capture turns selection/drags into escape sequences (the "weird
    // characters" you see when moving the mouse in Alacritty, etc.).
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    crossterm::execute!(stdout, EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let app_result: Result<()> = {
        // Run the app
        let mut app = SSMApp::new().await?;
        run_ssm_app(&mut terminal, &mut app).await
    };

    // Restore terminal
    let _ = disable_raw_mode();
    let _ = crossterm::execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let _ = terminal.show_cursor();

    app_result
}
