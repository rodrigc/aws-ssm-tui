use crate::aws::profile::default::detect_default_profile;
use crate::aws::profile::list::ProfileInfo;
use crate::aws::ssm::InstanceInfo;
use crate::ui::profile::ProfileSelect;
use crate::ui::screen::Screen;
use crate::ui::ssm_instances::SSMInstancesScreen;
use crate::utils::constants::env;
use aws_config::BehaviorVersion;
use color_eyre::Result;
use crossterm::event::{self, Event, KeyEventKind};
use ratatui::prelude::Backend;
use ratatui::{widgets::ListState, Terminal};
use std::time::Duration;
use tokio::sync::mpsc;

pub struct SSMApp {
    pub current_screen: Box<dyn Screen>,

    // Global / Shared data
    pub instances: Vec<InstanceInfo>,
    pub profiles: Vec<ProfileInfo>,
    pub current_profile: String,
    pub loading: bool,
    pub client: aws_sdk_ssm::Client,
    refresh_instances_requested: bool,
    pending_profile_switch: Option<String>,

    should_quit: bool,
    session_active: bool,
    pub session_end_tx: mpsc::Sender<()>,
    pub session_end_rx: mpsc::Receiver<()>,

    pub logger: crate::ui::logger::AppLogger,
}

impl SSMApp {
    pub async fn new() -> Result<Self> {
        let current_profile = detect_default_profile()
            .await
            .unwrap_or_else(|| "".to_string());

        let profiles = crate::aws::profile::list::load_profiles()
            .await
            .map_err(|e| color_eyre::eyre::eyre!(e))?;

        // Build the config using the detected/current profile
        let config = aws_config::defaults(BehaviorVersion::latest())
            .profile_name(&current_profile)
            .load()
            .await;

        let client = aws_sdk_ssm::Client::new(&config);

        let mut profiles_state = ListState::default();
        // Highlight the current profile in the list
        if let Some(idx) = profiles.iter().position(|p| p.name == current_profile) {
            profiles_state.select(Some(idx));
        } else if !profiles.is_empty() {
            profiles_state.select(Some(0));
        }

        let (tx, rx) = mpsc::channel(1);

        Ok(Self {
            current_screen: Box::new(ProfileSelect::new(&profiles)),
            instances: vec![],
            profiles,
            current_profile,
            loading: false,
            client,
            refresh_instances_requested: false,
            pending_profile_switch: None,
            should_quit: false,
            session_active: false,
            session_end_tx: tx,
            session_end_rx: rx,
            logger: crate::ui::logger::AppLogger::default(),
        })
    }

    pub fn switch_profile(&mut self, profile_name: String) {
        self.pending_profile_switch = Some(profile_name);
    }

    pub async fn apply_pending_profile_switch(&mut self) -> Result<()> {
        if let Some(profile_name) = self.pending_profile_switch.take() {
            tracing::info!("Rebuilding client for profile: {}", profile_name);

            let config = aws_config::defaults(BehaviorVersion::latest())
                .profile_name(&profile_name)
                .load()
                .await;

            self.client = aws_sdk_ssm::Client::new(&config);
            self.current_profile = profile_name;
            std::env::set_var(env::AWS_PROFILE_ENV_VAR, &self.current_profile);

            // Optionally auto-refresh instances
            self.refresh_instances_requested = true;
        }
        Ok(())
    }

    pub async fn load_instances(&mut self) -> Result<()> {
        self.loading = true;

        self.instances = crate::aws::ssm::get_instances()
            .await
            .map_err(|e| color_eyre::eyre::eyre!(e))?;

        self.loading = false;

        // Refresh the current screen if it's the instances screen
        if self.current_screen.name() == "SSM Instances" {
            self.current_screen = Box::new(SSMInstancesScreen::new(&self.instances));
        }

        Ok(())
    }

    pub fn request_instance_refresh(&mut self) {
        self.refresh_instances_requested = true;
    }

    pub fn current_screen(&self) -> &dyn Screen {
        &*self.current_screen
    }

    /*
    pub fn switch_screen(&mut self, new_screen: Box<dyn Screen>) {
        self.current_screen = new_screen;
    }
    */

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub fn quit(&mut self) {
        self.should_quit = true
    }

    pub fn start_session(&mut self) {
        self.session_active = true;
    }

    pub fn end_session(&mut self) {
        self.session_active = false;
    }
}

pub async fn run_ssm_app<B: Backend>(terminal: &mut Terminal<B>, app: &mut SSMApp) -> Result<()> {
    loop {
        // Only draw if no session is active
        if !app.session_active {
            if let Err(e) = terminal.draw(|frame| {
                app.current_screen.draw(frame, app);

                // Draw logger if visible (bottom panel)
                if app.logger.visible {
                    let areas = ratatui::layout::Layout::default()
                        .direction(ratatui::layout::Direction::Vertical)
                        .constraints([
                            ratatui::layout::Constraint::Percentage(70),
                            ratatui::layout::Constraint::Percentage(30),
                        ])
                        .split(frame.area());

                    app.logger.draw(frame, areas[1]);
                }
            }) {
                tracing::error!("Draw error: {}", e);
            }
        }

        if !app.session_active {
            if event::poll(Duration::from_millis(50))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        let mut screen = std::mem::replace(
                            &mut app.current_screen,
                            Box::new(ProfileSelect::default()),
                        );

                        let next_screen = screen.handle_event(app, key.code);

                        if let Some(new_screen) = next_screen {
                            app.current_screen = new_screen;
                        } else {
                            // Put the old screen back if no switch happened
                            app.current_screen = screen;
                        }
                    }
                }
            }
        } else {
            // Session is active - sleep, do not read keys
            tokio::time::sleep(Duration::from_millis(100)).await
        }

        // Process pending profile switch
        if app.pending_profile_switch.is_some() {
            if let Err(e) = app.apply_pending_profile_switch().await {
                tracing::error!("Failed to switch profile: {}", e);
            }
        }

        // Check if background session has ended
        if app.session_end_rx.try_recv().is_ok() {
            app.end_session();

            // Force clean redraw
            let _ = terminal.clear();
            let _ = terminal.draw(|frame| {
                app.current_screen.draw(frame, app);
            });
        }

        // Process refresh request
        if app.refresh_instances_requested {
            if let Err(e) = app.load_instances().await {
                tracing::error!("Failed to load intances: {}", e);
            }

            if app.current_screen.name() == "SSM Instances" {
                app.current_screen = Box::new(SSMInstancesScreen::new(&app.instances));
            }
            app.refresh_instances_requested = false;
        }

        if app.should_quit() {
            break;
        }

        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    Ok(())
}
