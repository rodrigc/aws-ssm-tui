fn run_external_command<F>(terminal: &mut Terminal<impl Backend>, cmd_fn: F) -> Result<()>
where
    F: FnOnce() -> std::process::Child,
{
    // 1. Leave Ratatui's control
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;

    // 2. Run the external command (blocking)
    let mut child = cmd_fn()?;
    let status = child.wait()?;

    // 3. Restore Ratatui
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen)?;
    terminal.clear()?;
    terminal.hide_cursor()?;
    // Force redraw on next draw call
    terminal.resize(terminal.size()?)?;

    if !status.success() {
        bail!("Command exited with code: {:?}", status.code());
    }

    Ok(())
}
