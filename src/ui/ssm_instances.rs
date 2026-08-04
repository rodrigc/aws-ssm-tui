use crate::aws::ssm::InstanceInfo;
use crate::ui::profile::ProfileSelect;
use crate::ui::screen::Screen;
use crossterm::event::KeyCode;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, TableState, Wrap},
    Frame,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PortForwardFocus {
    RemotePort,
    LocalPort,
    Ok,
    Cancel,
}

impl PortForwardFocus {
    fn next(self) -> Self {
        match self {
            Self::RemotePort => Self::LocalPort,
            Self::LocalPort => Self::Ok,
            Self::Ok => Self::Cancel,
            Self::Cancel => Self::RemotePort,
        }
    }

    fn prev(self) -> Self {
        match self {
            Self::RemotePort => Self::Cancel,
            Self::LocalPort => Self::RemotePort,
            Self::Ok => Self::LocalPort,
            Self::Cancel => Self::Ok,
        }
    }
}

#[derive(Debug, Clone)]
struct PortForwardDialog {
    instance_id: String,
    remote_port: String,
    local_port: String,
    focus: PortForwardFocus,
    error: Option<String>,
}

impl PortForwardDialog {
    fn new(instance_id: String) -> Self {
        Self {
            instance_id,
            remote_port: "22".to_string(),
            local_port: "2222".to_string(),
            focus: PortForwardFocus::RemotePort,
            error: None,
        }
    }

    fn focused_value_mut(&mut self) -> Option<&mut String> {
        match self.focus {
            PortForwardFocus::RemotePort => Some(&mut self.remote_port),
            PortForwardFocus::LocalPort => Some(&mut self.local_port),
            PortForwardFocus::Ok | PortForwardFocus::Cancel => None,
        }
    }

    fn parse_ports(&self) -> Result<(u16, u16), String> {
        let remote = self
            .remote_port
            .trim()
            .parse::<u16>()
            .map_err(|_| "Remote port must be a number between 1 and 65535".to_string())?;
        let local = self
            .local_port
            .trim()
            .parse::<u16>()
            .map_err(|_| "Local port must be a number between 1 and 65535".to_string())?;
        if remote == 0 {
            return Err("Remote port must be between 1 and 65535".to_string());
        }
        if local == 0 {
            return Err("Local port must be between 1 and 65535".to_string());
        }
        Ok((remote, local))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConfirmFocus {
    Ok,
    Cancel,
}

#[derive(Debug, Clone)]
struct EndPortForwardDialog {
    instance_id: String,
    remote_port: u16,
    local_port: u16,
    focus: ConfirmFocus,
}

/// Overlays owned by the instances screen (start / stop dialogs).
/// Console popup lives on `SSMApp` so it survives list refresh.
#[derive(Debug)]
enum Overlay {
    Start(PortForwardDialog),
    End(EndPortForwardDialog),
}

#[derive(Default, Debug)]
pub struct SSMInstancesScreen {
    pub instances_state: TableState,
    overlay: Option<Overlay>,
}

impl SSMInstancesScreen {
    pub fn new(instances: &[InstanceInfo]) -> Self {
        let mut state = TableState::default();
        if !instances.is_empty() {
            state.select(Some(0));
        }
        Self {
            instances_state: state,
            overlay: None,
        }
    }

    fn selected_instance_id(&self, app: &crate::ui::ssm::SSMApp) -> Option<String> {
        self.instances_state
            .selected()
            .and_then(|i| app.instances.get(i))
            .map(|inst| inst.instance_id.clone())
    }

    fn open_port_forward_action(&mut self, app: &crate::ui::ssm::SSMApp) {
        let Some(instance_id) = self.selected_instance_id(app) else {
            return;
        };

        if app.is_port_forward_starting(&instance_id) {
            return;
        }

        if let Some(info) = app.port_forward_info(&instance_id) {
            self.overlay = Some(Overlay::End(EndPortForwardDialog {
                instance_id: info.instance_id,
                remote_port: info.remote_port,
                local_port: info.local_port,
                focus: ConfirmFocus::Ok,
            }));
        } else {
            self.overlay = Some(Overlay::Start(PortForwardDialog::new(instance_id)));
        }
    }

    fn draw_start_dialog(&self, frame: &mut Frame, dialog: &PortForwardDialog) {
        let area = centered_rect(50, 12, frame.area());
        frame.render_widget(Clear, area);

        let block = Block::default()
            .title(format!(" Port Forward — {} ", dialog.instance_id))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .style(Style::default().bg(Color::Black));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Min(0),
            ])
            .margin(1)
            .split(inner);

        let field_style = |focused: bool| {
            if focused {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White).bg(Color::DarkGray)
            }
        };

        let label_style = Style::default().fg(Color::Gray);

        let remote_focus = dialog.focus == PortForwardFocus::RemotePort;
        let remote_line = Line::from(vec![
            Span::styled(format!("{:<14}", "Remote Port:"), label_style),
            Span::styled(
                format!(" {:<10} ", dialog.remote_port),
                field_style(remote_focus),
            ),
        ]);
        frame.render_widget(Paragraph::new(remote_line), chunks[1]);

        let local_focus = dialog.focus == PortForwardFocus::LocalPort;
        let local_line = Line::from(vec![
            Span::styled(format!("{:<14}", "Local Port:"), label_style),
            Span::styled(
                format!(" {:<10} ", dialog.local_port),
                field_style(local_focus),
            ),
        ]);
        frame.render_widget(Paragraph::new(local_line), chunks[3]);

        if let Some(err) = &dialog.error {
            frame.render_widget(
                Paragraph::new(err.as_str()).style(Style::default().fg(Color::Red)),
                chunks[4],
            );
        } else {
            frame.render_widget(
                Paragraph::new("Tab/↑↓ move  Enter confirm  Esc cancel")
                    .style(Style::default().fg(Color::DarkGray)),
                chunks[4],
            );
        }

        let button_style = |focused: bool| {
            if focused {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White).bg(Color::DarkGray)
            }
        };

        let buttons = Line::from(vec![
            Span::styled(
                "  OK  ",
                button_style(dialog.focus == PortForwardFocus::Ok),
            ),
            Span::raw("   "),
            Span::styled(
                " Cancel ",
                button_style(dialog.focus == PortForwardFocus::Cancel),
            ),
        ]);
        frame.render_widget(
            Paragraph::new(buttons).alignment(Alignment::Center),
            chunks[5],
        );
    }

    fn draw_end_dialog(&self, frame: &mut Frame, dialog: &EndPortForwardDialog) {
        let area = centered_rect(52, 10, frame.area());
        frame.render_widget(Clear, area);

        let block = Block::default()
            .title(" End Port Forward ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .style(Style::default().bg(Color::Black));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Min(0),
            ])
            .margin(1)
            .split(inner);

        frame.render_widget(
            Paragraph::new(format!("Instance: {}", dialog.instance_id))
                .style(Style::default().fg(Color::White)),
            chunks[0],
        );
        frame.render_widget(
            Paragraph::new(format!(
                "Active: localhost:{} → remote:{}",
                dialog.local_port, dialog.remote_port
            ))
            .style(Style::default().fg(Color::Cyan)),
            chunks[1],
        );
        frame.render_widget(
            Paragraph::new("Stop this port-forward session?")
                .style(Style::default().fg(Color::Gray)),
            chunks[2],
        );

        let button_style = |focused: bool| {
            if focused {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Red)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White).bg(Color::DarkGray)
            }
        };

        let buttons = Line::from(vec![
            Span::styled(
                "  OK  ",
                button_style(dialog.focus == ConfirmFocus::Ok),
            ),
            Span::raw("   "),
            Span::styled(
                " Cancel ",
                button_style(dialog.focus == ConfirmFocus::Cancel),
            ),
        ]);
        frame.render_widget(
            Paragraph::new(buttons).alignment(Alignment::Center),
            chunks[4],
        );
    }

    fn draw_console(frame: &mut Frame, popup: &crate::ui::ssm::ConsolePopup) {
        let area = centered_rect(70, 18, frame.area());
        frame.render_widget(Clear, area);

        let block = Block::default()
            .title(format!(" {} ", popup.title))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Red))
            .style(Style::default().bg(Color::Black))
            .title_bottom(Line::from(" Esc / Enter close  ↑↓ scroll ").centered());
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let text = Paragraph::new(popup.body.as_str())
            .wrap(Wrap { trim: false })
            .scroll((popup.scroll, 0))
            .style(Style::default().fg(Color::White));
        frame.render_widget(text, inner);
    }

    fn handle_start_key(
        &mut self,
        app: &mut crate::ui::ssm::SSMApp,
        key: KeyCode,
    ) {
        let Some(Overlay::Start(_)) = &self.overlay else {
            return;
        };

        match key {
            KeyCode::Esc => self.overlay = None,
            KeyCode::Tab | KeyCode::Down => {
                if let Some(Overlay::Start(d)) = &mut self.overlay {
                    d.focus = d.focus.next();
                    d.error = None;
                }
            }
            KeyCode::BackTab | KeyCode::Up => {
                if let Some(Overlay::Start(d)) = &mut self.overlay {
                    d.focus = d.focus.prev();
                    d.error = None;
                }
            }
            KeyCode::Left => {
                if let Some(Overlay::Start(d)) = &mut self.overlay {
                    if d.focus == PortForwardFocus::Cancel {
                        d.focus = PortForwardFocus::Ok;
                    }
                }
            }
            KeyCode::Right => {
                if let Some(Overlay::Start(d)) = &mut self.overlay {
                    if d.focus == PortForwardFocus::Ok {
                        d.focus = PortForwardFocus::Cancel;
                    }
                }
            }
            KeyCode::Backspace => {
                if let Some(Overlay::Start(d)) = &mut self.overlay {
                    if let Some(value) = d.focused_value_mut() {
                        value.pop();
                    }
                    d.error = None;
                }
            }
            KeyCode::Char(c) if c.is_ascii_digit() => {
                if let Some(Overlay::Start(d)) = &mut self.overlay {
                    if let Some(value) = d.focused_value_mut() {
                        if value.len() < 5 {
                            value.push(c);
                        }
                    }
                    d.error = None;
                }
            }
            KeyCode::Enter => {
                let focus = match &self.overlay {
                    Some(Overlay::Start(d)) => d.focus,
                    _ => return,
                };

                match focus {
                    PortForwardFocus::Cancel => self.overlay = None,
                    PortForwardFocus::RemotePort | PortForwardFocus::LocalPort => {
                        if let Some(Overlay::Start(d)) = &mut self.overlay {
                            d.focus = d.focus.next();
                        }
                    }
                    PortForwardFocus::Ok => {
                        let parse_result = match &self.overlay {
                            Some(Overlay::Start(d)) => d
                                .parse_ports()
                                .map(|(r, l)| (d.instance_id.clone(), r, l)),
                            _ => return,
                        };

                        match parse_result {
                            Ok((instance_id, remote_port, local_port)) => {
                                self.overlay = None;
                                app.begin_port_forward(instance_id, remote_port, local_port);
                            }
                            Err(msg) => {
                                if let Some(Overlay::Start(d)) = &mut self.overlay {
                                    d.error = Some(msg);
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_end_key(&mut self, app: &mut crate::ui::ssm::SSMApp, key: KeyCode) {
        let Some(Overlay::End(dialog)) = &self.overlay else {
            return;
        };
        let instance_id = dialog.instance_id.clone();

        match key {
            KeyCode::Esc => self.overlay = None,
            KeyCode::Tab | KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down => {
                if let Some(Overlay::End(d)) = &mut self.overlay {
                    d.focus = match d.focus {
                        ConfirmFocus::Ok => ConfirmFocus::Cancel,
                        ConfirmFocus::Cancel => ConfirmFocus::Ok,
                    };
                }
            }
            KeyCode::Enter => {
                let focus = match &self.overlay {
                    Some(Overlay::End(d)) => d.focus,
                    _ => return,
                };
                match focus {
                    ConfirmFocus::Cancel => self.overlay = None,
                    ConfirmFocus::Ok => {
                        self.overlay = None;
                        app.stop_port_forward(&instance_id);
                    }
                }
            }
            _ => {}
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

        let profile_text = format!("Active AWS Profile: {}", app.current_profile);
        let profile_para = Paragraph::new(profile_text)
            .block(Block::default().borders(Borders::ALL))
            .style(
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            );

        frame.render_widget(profile_para, chunks[0]);

        let header = Row::new(vec![
            Cell::from("FWD")
                .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Cell::from("ComputerName")
                .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Cell::from("InstanceId")
                .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Cell::from("LastPingDateTime")
                .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Cell::from("PingStatus")
                .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Cell::from("PlatformName")
                .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        ]);

        let rows: Vec<Row> = app
            .instances
            .iter()
            .map(|instance| {
                let (fwd_label, row_style) = if app.is_port_forward_starting(&instance.instance_id)
                {
                    (
                        "…".to_string(),
                        Style::default().fg(Color::Yellow),
                    )
                } else if let Some(info) = app.port_forward_info(&instance.instance_id) {
                    (
                        format!("{}→{}", info.local_port, info.remote_port),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    )
                } else {
                    (String::new(), Style::default())
                };

                Row::new(vec![
                    Cell::from(fwd_label).style(if row_style == Style::default() {
                        Style::default().fg(Color::DarkGray)
                    } else {
                        row_style
                    }),
                    Cell::from(instance.computer_name.as_str()),
                    Cell::from(instance.instance_id.as_str()),
                    Cell::from(instance.last_ping_date_time.as_str()),
                    Cell::from(instance.ping_status.as_str()),
                    Cell::from(instance.platform_name.as_str()),
                ])
                .style(if app.has_port_forward(&instance.instance_id) {
                    Style::default().fg(Color::Cyan)
                } else {
                    Style::default()
                })
            })
            .collect();

        let title = if app.loading {
            "SSM Instances (Loading...)"
        } else {
            "SSM Instances (↑↓ navigate, s=session, f=port-forward/stop, p=profiles, r=refresh, q=quit)"
        };

        let table = Table::new(
            rows,
            [
                Constraint::Length(11), // FWD
                Constraint::Min(20),    // ComputerName
                Constraint::Length(20), // InstanceId
                Constraint::Length(25), // LastPingDateTime
                Constraint::Length(12), // PingStatus
                Constraint::Min(16),    // PlatformName
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

        match &self.overlay {
            Some(Overlay::Start(d)) => self.draw_start_dialog(frame, d),
            Some(Overlay::End(d)) => self.draw_end_dialog(frame, d),
            None => {}
        }

        if let Some(popup) = &app.console_popup {
            Self::draw_console(frame, popup);
        }
    }

    fn handle_event(
        &mut self,
        app: &mut crate::ui::ssm::SSMApp,
        key: KeyCode,
    ) -> Option<Box<dyn Screen>> {
        // Console keys are handled globally in the main loop.
        if app.console_popup.is_some() {
            return None;
        }

        if self.overlay.is_some() {
            match &self.overlay {
                Some(Overlay::Start(_)) => self.handle_start_key(app, key),
                Some(Overlay::End(_)) => self.handle_end_key(app, key),
                None => {}
            }
            return None;
        }

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
                    Some(0) => app.instances.len().saturating_sub(1),
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

            KeyCode::Char('f') => {
                self.open_port_forward_action(app);
                None
            }

            KeyCode::Char('s') => {
                if let Some(i) = self.instances_state.selected() {
                    if let Some(instance) = app.instances.get(i) {
                        let instance_id = instance.instance_id.clone();
                        let client = app.client.clone();
                        let tx = app.session_end_tx.clone();
                        app.start_session();

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

fn centered_rect(percent_x: u16, height: u16, r: Rect) -> Rect {
    let popup_width = (r.width as u32 * percent_x as u32 / 100)
        .max(40)
        .min(r.width as u32) as u16;
    let popup_height = height.min(r.height);

    let x = r.x + (r.width.saturating_sub(popup_width)) / 2;
    let y = r.y + (r.height.saturating_sub(popup_height)) / 2;

    Rect {
        x,
        y,
        width: popup_width,
        height: popup_height,
    }
}
