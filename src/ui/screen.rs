use crate::SSMApp;
use crossterm::event::KeyCode;
use ratatui::Frame;

pub trait Screen {
    fn draw(&self, frame: &mut Frame, app: &SSMApp);
    fn handle_event(&mut self, app: &mut SSMApp, key: KeyCode) -> Option<Box<dyn Screen>>;
    fn name(&self) -> &'static str;
}
