mod aws;
mod ui;

use anyhow::Result;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use scopeguard::defer;
use std::io::{self, Write};
use tui_logger::LevelFilter;
use ui::profile::{run_profile_app, ProfileApp};
use ui::ssm::{run_ssm_app, SSMApp};

#[tokio::main]
async fn main() -> Result<()> {
    println!("Loading AWS configuration...");

    // Load profiles once at startup
    let profiles = aws::profile::list::load_profiles().await?;
    println!("Found {} profiles", profiles.len());

    // Try to detect usable default
    let initial_profile = aws::profile::default::detect_default_profile().await;

    tui_logger::init_logger(LevelFilter::Trace).unwrap();
    tui_logger::set_default_level(LevelFilter::Trace);

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    defer! {
        let _ = disable_raw_mode();
        let _ = disable_raw_mode();
        let _ = execute!(
                 //terminal.backend_mut(),
                 io::stdout(),
                 LeaveAlternateScreen,
                 DisableMouseCapture
              );
        let mut stdout = io::stdout();
        let _ = stdout.flush();
    //cleanup_terminal.show_cursor();
    }

    // If we do not have a usable profile, start with the Profile app.
    // If we have a usable profile, start with the SSM app
    let mut current_app = if initial_profile.is_some() {
        CurrentApp::SSM
    } else {
        CurrentApp::Profile
    };

    let result = loop {
        match current_app {
            CurrentApp::SSM => {
                let app = SSMApp::new().await?;
                tracing::info!("Found {} instances", app.instances.len());

                match run_ssm_app(&mut terminal, app).await? {
                    ui::profile::AppAction::Quit => break Ok(()),
                    ui::profile::AppAction::SwitchToProfileSelect => {
                        current_app = CurrentApp::Profile;
                    }
                    _ => {}
                }
            }
            CurrentApp::Profile => {
                let app = ProfileApp::new(profiles.clone());

                // ProfileApp only returns SwitchToSSMInstances now
                run_profile_app(&mut terminal, app).await?;
                current_app = CurrentApp::SSM;
            }
        }
    };

    /*
        // Restore terminal
        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        terminal.show_cursor()?;
    */

    result
}

enum CurrentApp {
    SSM,
    Profile,
}
