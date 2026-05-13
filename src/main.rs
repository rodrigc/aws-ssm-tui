mod aws;
mod ui;

use ui::ssm::{run_ssm_app, SSMApp};

use color_eyre::Result;
use ratatui::{
    backend::CrosstermBackend,
    crossterm::{
        event::{DisableMouseCapture, EnableMouseCapture},
        terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
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

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    crossterm::execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let app_result: Result<()> = {
        // Run the app
        let mut app = SSMApp::new().await?;
        run_ssm_app(&mut terminal, &mut app).await
    };

    // Restore terminal
    let _ = disable_raw_mode();
    let _ = crossterm::execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    );
    let _ = terminal.show_cursor();

    app_result
}
