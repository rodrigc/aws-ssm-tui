use anyhow::Result;
use crossterm::event;
use crossterm::event::{Event, KeyCode};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Terminal,
};

use crate::aws::profile::list::ProfileInfo;

pub struct ProfileApp {
    profiles: Vec<ProfileInfo>,
    list_state: ListState,
    activate_profile: Option<String>,
}

impl ProfileApp {
    pub fn new(profiles: Vec<ProfileInfo>) -> Self {
        let list_state = ListState::default();

        // Get the current selected profile from environment or default
        let activate_profile = std::env::var("AWS_PROFILE")
            .ok()
            .or_else(|| Some("default".to_string()));

        let mut app = Self {
            profiles,
            list_state,
            activate_profile,
        };

        // Jump to the selected profile in the list (or select first if not found)
        app.jump_to_selected();

        // If still nothing selected and we have profiles, select the first one
        if app.list_state.selected().is_none() && !app.profiles.is_empty() {
            app.list_state.select(Some(0));
        }

        app
    }

    pub fn next(&mut self) {
        let i = match self.list_state.selected() {
            Some(i) => {
                if i >= self.profiles.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    pub fn previous(&mut self) {
        let i = match self.list_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.profiles.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    pub fn select_current(&mut self) {
        if let Some(i) = self.list_state.selected() {
            self.activate_profile = Some(self.profiles[i].name.clone());
        }
    }

    pub fn jump_to_selected(&mut self) {
        if let Some(selected_name) = &self.activate_profile {
            if let Some(index) = self.profiles.iter().position(|p| &p.name == selected_name) {
                self.list_state.select(Some(index));
            }
        }
    }

    pub fn get_highlighted_profile(&self) -> Option<&ProfileInfo> {
        self.list_state.selected().map(|i| &self.profiles[i])
    }

    /*
    fn get_activate_profile_info(&self) -> Option<&ProfileInfo> {
        self.activate_profile
            .as_ref()
            .and_then(|name| self.profiles.iter().find(|p| &p.name == name))
    }
    */
}

pub fn ui(f: &mut ratatui::Frame, app: &mut ProfileApp) {
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Active profile
            Constraint::Fill(1),   // bottom: grows / fills remaining space
        ])
        .split(f.area());

    let inner = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(5), // Profile list
            Constraint::Min(8), // Profile info
        ])
        .split(outer[1]);

    // Active profile
    let selected_text = if let Some(profile_name) = &app.activate_profile {
        format!("Active Profile: {}", profile_name)
    } else {
        "Active Profile: None".to_string()
    };

    let selected_paragraph = Paragraph::new(selected_text)
        .block(
            Block::default()
                //.title("Current Selection")
                .borders(Borders::ALL),
        )
        .style(
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        );

    f.render_widget(selected_paragraph, outer[0]);
    // Profile list
    let items: Vec<ListItem> = app
        .profiles
        .iter()
        .map(|profile| {
            let is_active = app
                .activate_profile
                .as_ref()
                .is_some_and(|name| name == &profile.name);

            // Prefix only the active one with *
            let prefix = if is_active { "* " } else { "  " }; // two spaces to keep alignment

            let text = format!("{prefix}{}", profile.name);
            let span = Span::raw(text);

            // Optional: give active profiles a different style (even when not highlighted)
            let style = if is_active {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            ListItem::new(Line::from(span)).style(style)
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .title("AWS Profiles (↑/↓ to navigate, Enter to select, q to quit)")
                .borders(Borders::ALL),
        )
        .highlight_style(
            Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");

    f.render_stateful_widget(list, inner[0], &mut app.list_state);

    // Profile info for the highlighted profile
    let info_text = if let Some(profile) = app.get_highlighted_profile() {
        let mut lines = vec![format!("Profile: {}", profile.name)];

        if let Some(region) = &profile.region {
            lines.push(format!("Region: {}", region));
        }

        if let Some(sso_url) = &profile.sso_start_url {
            lines.push(format!("SSO Start URL: {}", sso_url));
        }

        if let Some(account_id) = &profile.sso_account_id {
            lines.push(format!("SSO Account ID: {}", account_id));
        }

        if let Some(role_name) = &profile.sso_role_name {
            lines.push(format!("SSO Role Name: {}", role_name));
        }

        lines.join("\n")
    } else {
        "No profile selected".to_string()
    };

    let info_paragraph = Paragraph::new(info_text).block(
        Block::default()
            .title("Profile Information (highlighted)")
            .borders(Borders::ALL),
    );

    f.render_widget(info_paragraph, inner[1]);
}

pub enum AppAction {
    Quit,
    SwitchToSSM,
}

pub async fn run_profile_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    mut app: ProfileApp,
) -> Result<AppAction> {
    loop {
        terminal.draw(|f| ui(f, &mut app))?;

        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Char('q') => return Ok(AppAction::Quit),
                KeyCode::Char('p') => return Ok(AppAction::SwitchToSSM),
                KeyCode::Down => app.next(),
                KeyCode::Up => app.previous(),
                KeyCode::Enter => {
                    app.select_current();
                    // Optionally, you could break here to exit after selection
                }
                _ => {}
            }
        }
    }
}
