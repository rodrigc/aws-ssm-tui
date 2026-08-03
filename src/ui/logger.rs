use ratatui::{
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders},
    Frame,
};
use tui_logger::TuiLoggerWidget;

#[derive(Debug, Default)]
pub struct AppLogger {
    pub visible: bool,
}

impl AppLogger {
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    pub fn draw(&self, frame: &mut Frame, area: Rect) {
        if !self.visible {
            return;
        }

        let logger = TuiLoggerWidget::default()
            .style_error(Style::default().fg(Color::Red))
            .style_warn(Style::default().fg(Color::Yellow))
            .style_info(Style::default().fg(Color::Green))
            .style_debug(Style::default().fg(Color::Gray))
            .style_trace(Style::default().fg(Color::Cyan))
            .block(
                Block::default()
                    .title("Logs (L to toggle)")
                    .borders(Borders::ALL),
            );

        frame.render_widget(logger, area);
    }
}
