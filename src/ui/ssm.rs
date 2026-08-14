use crate::aws::profile::default::detect_default_profile;
use crate::aws::profile::list::ProfileInfo;
use crate::aws::ssm::{InstanceInfo, SpawnedPortForward};
use crate::ui::profile::ProfileSelect;
use crate::ui::screen::Screen;
use crate::ui::ssm_instances::SSMInstancesScreen;
use crate::utils::constants::env;
use aws_config::BehaviorVersion;
use color_eyre::Result;
use crossterm::event::{self, Event, KeyEventKind};
use ratatui::prelude::Backend;
use ratatui::{widgets::ListState, Terminal};
use std::collections::{HashMap, HashSet};
use std::time::Duration;
use tokio::sync::mpsc;

/// Async results from background port-forward start attempts.
pub enum PortForwardEvent {
    Started(SpawnedPortForward),
    Failed {
        instance_id: String,
        detail: String,
    },
}

/// Lightweight view of an active forward for the UI (no process handle).
#[derive(Debug, Clone)]
pub struct PortForwardInfo {
    pub instance_id: String,
    pub remote_port: u16,
    pub local_port: u16,
    pub session_id: String,
}

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

    /// Active port-forwards keyed by instance id (one per instance).
    port_forwards: HashMap<String, SpawnedPortForward>,
    /// Instance ids currently starting a port-forward.
    pub port_forward_starting: HashSet<String>,
    pub port_forward_tx: mpsc::Sender<PortForwardEvent>,
    pub port_forward_rx: mpsc::Receiver<PortForwardEvent>,
    /// Console popup content (errors / unexpected exits), owned by app so it
    /// survives screen rebuilds.
    pub console_popup: Option<ConsolePopup>,

    pub logger: crate::ui::logger::AppLogger,
}

#[derive(Debug, Clone)]
pub struct ConsolePopup {
    pub title: String,
    pub body: String,
    pub scroll: u16,
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
        let (pf_tx, pf_rx) = mpsc::channel(8);

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
            port_forwards: HashMap::new(),
            port_forward_starting: HashSet::new(),
            port_forward_tx: pf_tx,
            port_forward_rx: pf_rx,
            console_popup: None,
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

        self.instances = crate::aws::ssm::get_instances(&self.client)
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

    pub fn port_forward_info(&self, instance_id: &str) -> Option<PortForwardInfo> {
        self.port_forwards.get(instance_id).map(|s| PortForwardInfo {
            instance_id: s.instance_id.clone(),
            remote_port: s.remote_port,
            local_port: s.local_port,
            session_id: s.session_id.clone(),
        })
    }

    pub fn has_port_forward(&self, instance_id: &str) -> bool {
        self.port_forwards.contains_key(instance_id)
    }

    pub fn is_port_forward_starting(&self, instance_id: &str) -> bool {
        self.port_forward_starting.contains(instance_id)
    }

    pub fn show_console(&mut self, title: impl Into<String>, body: impl Into<String>) {
        self.console_popup = Some(ConsolePopup {
            title: title.into(),
            body: body.into(),
            scroll: 0,
        });
    }

    pub fn close_console(&mut self) {
        self.console_popup = None;
    }

    /// Kick off a background port-forward start (TUI stays active).
    pub fn begin_port_forward(&mut self, instance_id: String, remote_port: u16, local_port: u16) {
        if self.port_forwards.contains_key(&instance_id)
            || self.port_forward_starting.contains(&instance_id)
        {
            return;
        }

        self.port_forward_starting.insert(instance_id.clone());

        let client = self.client.clone();
        let tx = self.port_forward_tx.clone();

        tokio::spawn(async move {
            let result =
                crate::aws::ssm::start_port_forwarding(&client, &instance_id, remote_port, local_port)
                    .await;

            let event = match result {
                Ok(spawned) => PortForwardEvent::Started(spawned),
                Err(e) => PortForwardEvent::Failed {
                    instance_id: e.instance_id,
                    detail: e.detail,
                },
            };

            let _ = tx.send(event).await;
        });
    }

    pub fn handle_port_forward_event(&mut self, event: PortForwardEvent) {
        match event {
            PortForwardEvent::Started(spawned) => {
                self.port_forward_starting.remove(&spawned.instance_id);
                tracing::info!(
                    "Port forward active: {} localhost:{} → remote:{}",
                    spawned.instance_id,
                    spawned.local_port,
                    spawned.remote_port
                );
                self.port_forwards
                    .insert(spawned.instance_id.clone(), spawned);
            }
            PortForwardEvent::Failed {
                instance_id,
                detail,
            } => {
                self.port_forward_starting.remove(&instance_id);
                tracing::error!("Port forward failed for {instance_id}: {detail}");
                self.show_console(
                    format!("Port Forward Failed — {instance_id}"),
                    detail,
                );
            }
        }
    }

    /// Stop an active port-forward for the instance (kills plugin + terminates session).
    pub fn stop_port_forward(&mut self, instance_id: &str) {
        let Some(session) = self.port_forwards.remove(instance_id) else {
            return;
        };

        let client = self.client.clone();
        let id = instance_id.to_string();

        tokio::spawn(async move {
            let _detail = crate::aws::ssm::stop_port_forwarding(&client, session).await;
            tracing::info!("Port forward stopped for {id}");
        });
    }

    /// Poll running plugin processes; remove any that exited and surface output.
    pub fn poll_port_forwards(&mut self) {
        let mut exited = Vec::new();

        for (id, session) in self.port_forwards.iter_mut() {
            match session.child.try_wait() {
                Ok(Some(status)) => {
                    let plugin_out = session
                        .output
                        .lock()
                        .map(|g| g.clone())
                        .unwrap_or_default();
                    let mut detail = format!(
                        "Port forward process exited for {}\n\
                         Forward was: localhost:{} → remote:{}\n\
                         SessionId: {}\n\
                         Status: {status}\n",
                        session.instance_id, session.local_port, session.remote_port, session.session_id
                    );
                    if !plugin_out.trim().is_empty() {
                        detail.push_str("\n--- plugin output ---\n");
                        detail.push_str(&plugin_out);
                    }
                    exited.push((id.clone(), detail, session.session_id.clone()));
                }
                Ok(None) => {}
                Err(e) => {
                    exited.push((
                        id.clone(),
                        format!("Failed to poll port-forward process for {id}: {e}"),
                        session.session_id.clone(),
                    ));
                }
            }
        }

        for (id, detail, session_id) in exited {
            self.port_forwards.remove(&id);
            let client = self.client.clone();
            tokio::spawn(async move {
                let _ = client
                    .terminate_session()
                    .session_id(session_id)
                    .send()
                    .await;
            });
            self.show_console(format!("Port Forward Ended — {id}"), detail);
        }
    }
}

pub async fn run_ssm_app<B: Backend>(terminal: &mut Terminal<B>, app: &mut SSMApp) -> Result<()> {
    loop {
        // Only draw if no interactive shell session is active
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
                        // Global console popup captures keys first
                        if app.console_popup.is_some() {
                            handle_console_key(app, key.code);
                        } else {
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
            }
        } else {
            // Interactive shell session is active - sleep, do not read keys
            tokio::time::sleep(Duration::from_millis(100)).await
        }

        // Process pending profile switch
        if app.pending_profile_switch.is_some() {
            if let Err(e) = app.apply_pending_profile_switch().await {
                tracing::error!("Failed to switch profile: {}", e);
            }
        }

        // Check if background interactive session has ended
        if app.session_end_rx.try_recv().is_ok() {
            app.end_session();

            // Force clean redraw
            let _ = terminal.clear();
            let _ = terminal.draw(|frame| {
                app.current_screen.draw(frame, app);
            });
        }

        // Port-forward start results
        while let Ok(event) = app.port_forward_rx.try_recv() {
            app.handle_port_forward_event(event);
        }

        // Detect unexpected plugin exits
        app.poll_port_forwards();

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
            // Best-effort cleanup of port-forwards on quit
            let ids: Vec<String> = app.port_forwards.keys().cloned().collect();
            for id in ids {
                app.stop_port_forward(&id);
            }
            break;
        }

        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    Ok(())
}

fn handle_console_key(app: &mut SSMApp, key: crossterm::event::KeyCode) {
    use crossterm::event::KeyCode;
    match key {
        KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => {
            app.close_console();
        }
        KeyCode::Up | KeyCode::PageUp => {
            if let Some(p) = app.console_popup.as_mut() {
                p.scroll = p.scroll.saturating_sub(1);
            }
        }
        KeyCode::Down | KeyCode::PageDown => {
            if let Some(p) = app.console_popup.as_mut() {
                p.scroll = p.scroll.saturating_add(1);
            }
        }
        _ => {}
    }
}
