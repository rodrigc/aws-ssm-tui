use crate::aws::profile::default::detect_default_profile;
use crate::aws::profile::list::ProfileInfo;
use crate::aws::ssm::InstanceInfo;
use anyhow::Result;
use aws_config::BehaviorVersion;
use crossterm::event::{self, Event, KeyCode};
use log::info;
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Terminal,
};

use crate::ui::profile::AppAction;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Screen {
    SSMInstances,
    ProfileSelect,
}

pub struct SSMApp {
    screen: Screen,
    pub instances: Vec<InstanceInfo>,
    instances_state: ListState,
    pub profiles: Vec<ProfileInfo>,
    profiles_state: ListState,
    current_profile: String,
    loading: bool,
    pub client: aws_sdk_ssm::Client,
}

impl SSMApp {
    pub async fn new() -> Result<Self> {
        //let current_profile = String::from("CURRENT"); //aws::profile::get_current_profile();
        let current_profile = detect_default_profile()
            .await
            .unwrap_or_else(|| "default".to_string());

        let profiles = crate::aws::profile::list::load_profiles().await?;

        // Build the config using the detected/current profile
        let config = aws_config::defaults(BehaviorVersion::latest())
            .profile_name(&current_profile)
            .load()
            .await;

        let client = aws_sdk_ssm::Client::new(&config);
        let instances = crate::aws::ssm::get_instances().await?;

        let mut instances_state = ListState::default();
        if !instances.is_empty() {
            instances_state.select(Some(0));
        }

        let mut profiles_state = ListState::default();
        // Highlight the current profile in the list
        if let Some(idx) = profiles.iter().position(|p| p.name == current_profile) {
            profiles_state.select(Some(idx));
        } else if !profiles.is_empty() {
            profiles_state.select(Some(0));
        }

        Ok(Self {
            screen: Screen::SSMInstances,
            instances,
            instances_state,
            profiles,
            profiles_state,
            current_profile,
            loading: false,
            client,
        })
    }

    fn next_instance(&mut self) {
        if self.instances.is_empty() {
            return;
        }
        let i = match self.instances_state.selected() {
            Some(i) => {
                if i >= self.instances.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.instances_state.select(Some(i));
    }

    fn previous_instance(&mut self) {
        if self.instances.is_empty() {
            return;
        }
        let i = match self.instances_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.instances.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.instances_state.select(Some(i));
    }

    fn next_profile(&mut self) {
        if self.profiles.is_empty() {
            return;
        }
        let i = match self.profiles_state.selected() {
            Some(i) => {
                if i >= self.profiles.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.profiles_state.select(Some(i));
    }

    fn previous_profile(&mut self) {
        if self.profiles.is_empty() {
            return;
        }
        let i = match self.profiles_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.profiles.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.profiles_state.select(Some(i));
    }

    async fn select_profile(&mut self) -> Result<()> {
        if let Some(i) = self.profiles_state.selected() {
            self.current_profile = self.profiles[i].name.clone();
            self.loading = true;

            // In a real app, you'd set AWS_PROFILE here or use the profile in config
            // For now, we'll just reload instances with the current environment
            // Note: To actually use a different profile, you'd need to either:
            // 1. Set std::env::set_var("AWS_PROFILE", &self.current_profile) before loading config
            // 2. Pass profile_name to the config loader

            self.instances = crate::aws::ssm::get_instances().await?;
            self.instances_state.select(if self.instances.is_empty() {
                None
            } else {
                Some(0)
            });
            self.loading = false;
        }
        Ok(())
    }

    fn toggle_screen(&mut self) {
        self.screen = match self.screen {
            Screen::SSMInstances => Screen::ProfileSelect,
            Screen::ProfileSelect => Screen::SSMInstances,
        };
    }
}

pub fn ui_instances(f: &mut ratatui::Frame, app: &mut SSMApp) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Current profile display
            Constraint::Min(5),    // Instances list
        ])
        .split(f.area());

    // Current profile display
    let profile_text = format!("Active AWS Profile: {}", app.current_profile);
    let profile_paragraph = Paragraph::new(profile_text)
        .block(Block::default().borders(Borders::ALL))
        .style(
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        );
    f.render_widget(profile_paragraph, chunks[0]);

    // Instances list
    let items: Vec<ListItem> = app
        .instances
        .iter()
        .map(|instance| {
            let content = Line::from(vec![
                Span::raw(format!("{:20} ", instance.instance_id)),
                Span::raw(format!("{:15} ", instance.platform_type)),
                Span::raw(format!("{:15} ", instance.platform_name)),
                Span::raw(&instance.resource_type),
            ]);
            ListItem::new(content)
        })
        .collect();

    let title = if app.loading {
        "SSM Instances (Loading...)"
    } else {
        "SSM Instances (↑/↓ to navigate, 's' to start session, 'p' to switch profile, 'q' to quit)"
    };

    let list = List::new(items)
        .block(Block::default().title(title).borders(Borders::ALL))
        .highlight_style(
            Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");

    f.render_stateful_widget(list, chunks[1], &mut app.instances_state);
}

pub fn ui_profile_select(f: &mut ratatui::Frame, app: &mut SSMApp) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Current profile display
            Constraint::Min(5),    // Profile list
        ])
        .split(f.area());

    // Current profile display
    let profile_text = format!("Current Profile: {}", app.current_profile);
    let profile_paragraph = Paragraph::new(profile_text)
        .block(Block::default().borders(Borders::ALL))
        .style(
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        );
    f.render_widget(profile_paragraph, chunks[0]);

    // Profile list
    let items: Vec<ListItem> = app
        .profiles
        .iter()
        .map(|profile| {
            let mut content = vec![Span::raw(&profile.name)];

            if let Some(region) = &profile.region {
                content.push(Span::styled(
                    format!(" ({})", region),
                    Style::default().fg(Color::Gray),
                ));
            }

            ListItem::new(Line::from(content))
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .title(
                    "AWS Profiles (↑/↓ to navigate, Enter to select, 'p' to go back, 'q' to quit)",
                )
                .borders(Borders::ALL),
        )
        .highlight_style(
            Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");

    f.render_stateful_widget(list, chunks[1], &mut app.profiles_state);
}

pub fn ui(f: &mut ratatui::Frame, app: &mut SSMApp) {
    match app.screen {
        Screen::SSMInstances => ui_instances(f, app),
        Screen::ProfileSelect => ui_profile_select(f, app),
    }
}

pub async fn run_ssm_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    mut app: SSMApp,
) -> Result<AppAction> {
    loop {
        terminal.draw(|f| ui(f, &mut app))?;

        if let Event::Key(key) = event::read()? {
            match app.screen {
                Screen::SSMInstances => match key.code {
                    KeyCode::Char('q') => return Ok(AppAction::Quit),
                    KeyCode::Char('p') => return Ok(AppAction::SwitchToProfileSelect),
                    KeyCode::Char('s') => {
                        info!("START SESSION")
                    }
                    KeyCode::Down => app.next_instance(),
                    KeyCode::Up => app.previous_instance(),
                    _ => {}
                },
                Screen::ProfileSelect => match key.code {
                    KeyCode::Char('q') | KeyCode::Char('p') => {
                        return Ok(AppAction::SwitchToSSMInstances)
                    }
                    KeyCode::Char('p') => app.toggle_screen(),
                    KeyCode::Down => app.next_profile(),
                    KeyCode::Up => app.previous_profile(),
                    KeyCode::Enter => {
                        app.select_profile().await?;
                        app.toggle_screen();
                    }
                    _ => {}
                },
            }
        }
    }
}
