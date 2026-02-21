mod aws;
mod ui;

use anyhow::Result;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use ui::profile::{run_profile_app, ProfileApp};
use ui::ssm::{run_ssm_app, SSMApp};

#[tokio::main]
async fn main() -> Result<()> {
    println!("Loading AWS configuration...");
    
    // Load profiles once at startup
    let profiles = aws::profile::list::load_profiles().await?;
    println!("Found {} profiles", profiles.len());

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Start with SSM app
    let mut current_app = CurrentApp::SSM;
    
    let result = loop {
        match current_app {
            CurrentApp::SSM => {
                let app = SSMApp::new().await?;
                println!("Found {} instances", app.instances.len());
                
                match run_ssm_app(&mut terminal, app).await? {
                    ui::ssm::AppAction::Quit => break Ok(()),
                    ui::ssm::AppAction::SwitchToProfileSelect => {
                        current_app = CurrentApp::Profile;
                    }
                }
            }
            CurrentApp::Profile => {
                let app = ProfileApp::new(profiles.clone());
                
                // ProfileApp only returns SwitchToSSM now
                run_profile_app(&mut terminal, app).await?;
                current_app = CurrentApp::SSM;
            }
        }
    };

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}

enum CurrentApp {
    SSM,
    Profile,
}
