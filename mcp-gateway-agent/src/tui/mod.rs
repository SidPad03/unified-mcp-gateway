pub mod dashboard;
pub mod events;
pub mod setup;
pub mod theme;
pub mod widgets;

use std::io;

use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

pub type Tui = Terminal<CrosstermBackend<io::Stdout>>;

pub fn init_terminal() -> anyhow::Result<Tui> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

pub fn restore_terminal(terminal: &mut Tui) -> anyhow::Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    Ok(())
}

pub async fn run_setup() -> anyhow::Result<(bool, bool)> {
    let mut terminal = init_terminal()?;
    let mut wizard = setup::SetupWizard::new();

    let result = run_setup_loop(&mut terminal, &mut wizard).await;

    restore_terminal(&mut terminal)?;
    result?;

    Ok((wizard.should_start, wizard.should_install_service))
}

async fn run_setup_loop(
    terminal: &mut Tui,
    wizard: &mut setup::SetupWizard,
) -> anyhow::Result<()> {
    use crossterm::event::{self, Event};
    use std::time::Duration;

    loop {
        terminal.draw(|f| wizard.render(f))?;

        if wizard.finished {
            break;
        }

        if event::poll(Duration::from_millis(100))? {
            let ev = event::read()?;
            if let Event::Key(_) = &ev {
                wizard.handle_event(&ev);
            }
        }
    }

    Ok(())
}

pub async fn run_dashboard(
    mut event_rx: tokio::sync::mpsc::UnboundedReceiver<events::AgentEvent>,
) -> anyhow::Result<()> {
    let mut terminal = init_terminal()?;
    let mut dash = dashboard::Dashboard::new();

    let result = run_dashboard_loop(&mut terminal, &mut dash, &mut event_rx).await;

    restore_terminal(&mut terminal)?;
    result
}

async fn run_dashboard_loop(
    terminal: &mut Tui,
    dash: &mut dashboard::Dashboard,
    event_rx: &mut tokio::sync::mpsc::UnboundedReceiver<events::AgentEvent>,
) -> anyhow::Result<()> {
    use crossterm::event::EventStream;
    use futures_util::StreamExt;

    let mut reader = EventStream::new();

    loop {
        terminal.draw(|f| dash.render(f))?;

        if dash.should_quit {
            break;
        }

        tokio::select! {
            // Keyboard events
            maybe_event = reader.next() => {
                if let Some(Ok(event)) = maybe_event {
                    dash.handle_event(&event);
                }
            }
            // Agent events
            maybe_agent_event = event_rx.recv() => {
                if let Some(agent_event) = maybe_agent_event {
                    dash.handle_agent_event(agent_event);
                }
            }
            // Tick for re-render
            _ = tokio::time::sleep(std::time::Duration::from_millis(250)) => {}
        }
    }

    Ok(())
}
