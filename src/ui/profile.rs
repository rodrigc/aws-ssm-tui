use crate::ui::screen::Screen;
use crate::ui::ssm::SSMApp;
use crate::ui::ssm_instances::SSMInstancesScreen;
use crossterm::event::KeyCode;
use ratatui::Frame;
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};

use crate::aws::profile::list::ProfileInfo;

#[derive(Default, Debug, Clone, PartialEq)]
pub struct ProfileSelect {
    list_state: ListState,
    activate_profile: Option<String>,
}

impl ProfileSelect {
    pub fn new(profiles: &[ProfileInfo]) -> Self {
        let mut screen = Self {
            list_state: ListState::default(),
            activate_profile: std::env::var("AWS_PROFILE")
                .ok()
                .or_else(|| Some("".to_string())),
        };

        screen.jump_to_selected(profiles);

        if screen.list_state.selected().is_none() && !profiles.is_empty() {
            screen.list_state.select(Some(0));
        }

        screen
    }

    fn jump_to_selected(&mut self, profiles: &[ProfileInfo]) {
        if let Some(selected) = &self.activate_profile {
            if let Some(idx) = profiles.iter().position(|p| &p.name == selected) {
                self.list_state.select(Some(idx));
            }
        }
    }

    fn get_highlighted_profile<'a>(&self, profiles: &'a [ProfileInfo]) -> Option<&'a ProfileInfo> {
        self.list_state.selected().and_then(|i| profiles.get(i))
    }
}

impl Screen for ProfileSelect {
    fn name(&self) -> &'static str {
        "AWS Profile Select"
    }

    fn draw(&self, frame: &mut Frame, app: &SSMApp) {
        let outer = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Fill(1)])
            .split(frame.area());

        let inner = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(outer[1]);

        // Active Profile
        let active_text = match &self.activate_profile {
            Some(name) => format!("Active Profile: {}", name),
            None => "Active Profile: None".to_string(),
        };

        let active_para = Paragraph::new(active_text)
            .block(Block::default().borders(Borders::ALL))
            .style(
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            );

        frame.render_widget(active_para, outer[0]);

        // Profiles List
        let items: Vec<ListItem> = app
            .profiles
            .iter()
            .map(|p| {
                let is_active = self.activate_profile.as_ref().is_some_and(|n| n == &p.name);
                let prefix = if is_active { "* " } else { "  " };
                let style = if is_active {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };

                ListItem::new(Line::from(format!("{prefix}{}", p.name))).style(style)
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .title("AWS Profiles (↑↓ navigate, Enter select, q quit)")
                    .borders(Borders::ALL),
            )
            .highlight_style(
                Style::default()
                    .bg(Color::Blue)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol(">> ");

        frame.render_stateful_widget(list, inner[0], &mut self.list_state.clone());

        // Profile Info
        let info_text = if let Some(profile) = self.get_highlighted_profile(&app.profiles) {
            let mut lines = vec![format!("Profile: {}", profile.name)];
            if let Some(r) = &profile.region {
                lines.push(format!("Region: {}", r));
            }
            if let Some(u) = &profile.sso_start_url {
                lines.push(format!("SSO URL: {}", u));
            }
            if let Some(a) = &profile.sso_account_id {
                lines.push(format!("Account: {}", a));
            }
            if let Some(r) = &profile.sso_role_name {
                lines.push(format!("Role: {}", r));
            }
            lines.join("\n")
        } else {
            "No profile selected".to_string()
        };

        let info_para = Paragraph::new(info_text).block(
            Block::default()
                .title("Profile Information")
                .borders(Borders::ALL),
        );

        frame.render_widget(info_para, inner[1]);
    }

    fn handle_event(&mut self, app: &mut SSMApp, key: KeyCode) -> Option<Box<dyn Screen>> {
        match key {
            KeyCode::Down => {
                let i = match self.list_state.selected() {
                    Some(i) if i < app.profiles.len().saturating_sub(1) => i + 1,
                    _ => 0,
                };
                self.list_state.select(Some(i));
                None
            }

            KeyCode::Up => {
                let i = match self.list_state.selected() {
                    Some(0) => app.profiles.len().saturating_sub(1), // wrap around to last item
                    Some(i) => i - 1,
                    None => 0,
                };
                self.list_state.select(Some(i));
                None
            }

            KeyCode::Enter => {
                if let Some(i) = self.list_state.selected() {
                    if let Some(profile) = app.profiles.get(i) {
                        let name = profile.name.clone();
                        app.switch_profile(name.clone());

                        self.activate_profile = Some(name.clone());
                        app.current_profile = name;
                        self.jump_to_selected(&app.profiles);
                    }
                }
                None
            }

            KeyCode::Char('i') => Some(Box::new(SSMInstancesScreen::new(&app.instances))),

            KeyCode::Char('q') | KeyCode::Esc => {
                app.quit();
                None
            }

            KeyCode::Char('l') | KeyCode::Char('L') => {
                app.logger.toggle();
                None
            }

            _ => None,
        }
    }
}
