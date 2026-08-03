use crate::aws::ssm::InstanceInfo;
use crate::ui::profile::ProfileSelect;
use crate::ui::screen::Screen;
use crossterm::event::KeyCode;
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState},
    Frame,
};

#[derive(Default, Debug)]
pub struct SSMInstancesScreen {
    pub instances_state: TableState,
}

impl SSMInstancesScreen {
    pub fn new(instances: &[InstanceInfo]) -> Self {
        let mut state = TableState::default();
        if !instances.is_empty() {
            state.select(Some(0));
        }
        Self {
            instances_state: state,
        }
    }
}

impl Screen for SSMInstancesScreen {
    fn name(&self) -> &'static str {
        "SSM Instances"
    }

    fn draw(&self, frame: &mut Frame, app: &crate::ui::ssm::SSMApp) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Current profile
                Constraint::Min(5),    // Instances list
            ])
            .split(frame.area());

        // Current Profile Header
        let profile_text = format!("Active AWS Profile: {}", app.current_profile);
        let profile_para = Paragraph::new(profile_text)
            .block(Block::default().borders(Borders::ALL))
            .style(
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            );

        frame.render_widget(profile_para, chunks[0]);

        // Instances Table Header & Rows
        let header = Row::new(vec![
            Cell::from("ComputerName").style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Cell::from("InstanceId").style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Cell::from("LastPingDateTime").style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Cell::from("PingStatus").style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Cell::from("PlatformName").style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        ]);

        let rows: Vec<Row> = app
            .instances
            .iter()
            .map(|instance| {
                Row::new(vec![
                    Cell::from(instance.computer_name.as_str()),
                    Cell::from(instance.instance_id.as_str()),
                    Cell::from(instance.last_ping_date_time.as_str()),
                    Cell::from(instance.ping_status.as_str()),
                    Cell::from(instance.platform_name.as_str()),
                ])
            })
            .collect();

        let title = if app.loading {
            "SSM Instances (Loading...)"
        } else {
            "SSM Instances (↑↓ navigate, s=start session, i=instances, p=profiles, r=refresh, q=quit)"
        };

        let table = Table::new(
            rows,
            [
                Constraint::Min(25),     // ComputerName
                Constraint::Length(20),  // InstanceId
                Constraint::Length(25),  // LastPingDateTime
                Constraint::Length(15),  // PingStatus
                Constraint::Min(20),     // PlatformName
            ],
        )
        .header(header)
        .block(Block::default().title(title).borders(Borders::ALL))
        .row_highlight_style(
            Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");

        frame.render_stateful_widget(table, chunks[1], &mut self.instances_state.clone());
    }

    fn handle_event(
        &mut self,
        app: &mut crate::ui::ssm::SSMApp,
        key: KeyCode,
    ) -> Option<Box<dyn Screen>> {
        match key {
            KeyCode::Down => {
                let i = match self.instances_state.selected() {
                    Some(i) if i < app.instances.len().saturating_sub(1) => i + 1,
                    _ => 0,
                };
                self.instances_state.select(Some(i));
                None
            }

            KeyCode::Up => {
                let i = match self.instances_state.selected() {
                    Some(0) => app.instances.len().saturating_sub(1), // wrap around to last item
                    Some(i) => i - 1,
                    None => 0,
                };
                self.instances_state.select(Some(i));
                None
            }

            KeyCode::Char('r') => {
                app.request_instance_refresh();
                None
            }

            KeyCode::Char('s') => {
                if let Some(i) = self.instances_state.selected() {
                    if let Some(instance) = app.instances.get(i) {
                        let instance_id = instance.instance_id.clone();
                        let client = app.client.clone();
                        let tx = app.session_end_tx.clone(); // Send signal when done
                                                             //
                                                             // Pause TUI drawing and input
                        app.start_session();

                        // Suspend TUI completely
                        let _ = crossterm::terminal::disable_raw_mode();
                        let _ = crossterm::execute!(
                            std::io::stdout(),
                            crossterm::terminal::LeaveAlternateScreen,
                            crossterm::cursor::Show
                        );

                        println!("\n=== SSM Session Started on {} ===\n", instance_id);
                        println!("Full terminal mode active.");
                        println!("You can now run vim, htop, etc.\n");
                        tokio::spawn(async move {
                            let result =
                                crate::aws::ssm::start_interactive_ssm(&client, &instance_id).await;

                            // Restore TUI after session ends
                            let _ = crossterm::terminal::enable_raw_mode();
                            let _ = crossterm::execute!(
                                std::io::stdout(),
                                crossterm::terminal::EnterAlternateScreen,
                                crossterm::cursor::Hide
                            );

                            if let Err(e) = result {
                                tracing::error!("\nSession ended with error: {}", e);
                            } else {
                                tracing::info!("\nSession ended successfully.");
                            }

                            // Signal back to main loop that session has ended, and we can resume TUI
                            let _ = tx.send(()).await;
                        });
                    }
                }
                None
            }
            KeyCode::Char('p') => Some(Box::new(ProfileSelect::new(&app.profiles))),
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
