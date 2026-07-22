use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{self, Event};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use ratatui::Terminal;

use goble_core::protocol::DesktopMessage;
use goble_core::worker::WorkerId;

use crate::state::DesktopState;
use crate::worker_manager::WorkerClient;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Chat,
    Agents,
    Workers,
    Executions,
}

pub struct App {
    pub state: Arc<DesktopState>,
    pub tab: Tab,
    pub input: String,
    pub workers: Vec<(WorkerId, WorkerClient)>,
    pub selected_worker: usize,
    #[allow(dead_code)]
    pub selected_log: usize,
    pub running: bool,
}

impl App {
    pub fn new(state: Arc<DesktopState>) -> Self {
        Self {
            state,
            tab: Tab::Chat,
            input: String::new(),
            workers: Vec::new(),
            selected_worker: 0,
            selected_log: 0,
            running: true,
        }
    }

    pub async fn connect_worker(
        &mut self,
        worker_id: WorkerId,
        name: String,
        url: String,
        pairing_code: String,
    ) -> anyhow::Result<()> {
        let client = WorkerClient::connect(
            self.state.clone(),
            worker_id.clone(),
            url.clone(),
            pairing_code,
        )
        .await?;
        self.state.add_worker(worker_id.clone(), name, url, true);
        self.workers.push((worker_id, client));
        Ok(())
    }

    pub fn send_to_selected(&self, msg: DesktopMessage) -> anyhow::Result<()> {
        if let Some((_, client)) = self.workers.get(self.selected_worker) {
            client.send(msg)?;
        }
        Ok(())
    }

    pub fn on_tick(&mut self) {
        // Keepalive pings
        for (_, client) in &self.workers {
            let _ = client.send(DesktopMessage::Ping);
        }
    }
}

pub async fn run_tui(state: Arc<DesktopState>) -> anyhow::Result<()> {
    let mut terminal = setup_terminal()?;
    let mut app = App::new(state);
    let result = run_app(&mut terminal, &mut app).await;
    restore_terminal(terminal)?;
    result
}

fn setup_terminal() -> anyhow::Result<Terminal<CrosstermBackend<std::io::Stdout>>> {
    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    Ok(Terminal::new(backend)?)
}

fn restore_terminal(
    mut terminal: Terminal<CrosstermBackend<std::io::Stdout>>,
) -> anyhow::Result<()> {
    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(
        terminal.backend_mut(),
        crossterm::terminal::LeaveAlternateScreen
    )?;
    Ok(())
}

async fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
) -> anyhow::Result<()> {
    let mut interval = tokio::time::interval(Duration::from_secs(5));

    while app.running {
        terminal.draw(|f| draw(f, app))?;

        let key = tokio::select! {
            _ = interval.tick() => {
                app.on_tick();
                None
            }
            key = async {
                if event::poll(Duration::from_millis(100)).unwrap_or(false) {
                    if let Ok(Event::Key(key)) = event::read() {
                        return Some(key);
                    }
                }
                None
            } => key,
        };

        if let Some(key) = key {
            handle_key(app, key).await?;
        }
    }
    Ok(())
}

async fn handle_key(app: &mut App, key: crossterm::event::KeyEvent) -> anyhow::Result<()> {
    match key.code {
        crossterm::event::KeyCode::Char('q') => app.running = false,
        crossterm::event::KeyCode::F(1) => app.tab = Tab::Chat,
        crossterm::event::KeyCode::F(2) => app.tab = Tab::Agents,
        crossterm::event::KeyCode::F(3) => app.tab = Tab::Workers,
        crossterm::event::KeyCode::F(4) => app.tab = Tab::Executions,
        crossterm::event::KeyCode::Char('p') => {
            app.send_to_selected(DesktopMessage::Ping)?;
            app.state.add_chat_log(format!(
                "[desktop] ping sent to worker {}",
                app.selected_worker
            ));
        }
        crossterm::event::KeyCode::Char('r') => {
            let agent = goble_core::agent::AgentSpec::new("quick-agent", &app.input);
            let agent_id = agent.id.clone();
            let trace_id = uuid::Uuid::new_v4().to_string();
            app.state.add_agent(agent.clone());
            app.send_to_selected(DesktopMessage::RunAgent {
                trace_id,
                agent_id,
                spec: agent,
            })?;
            app.input.clear();
        }
        crossterm::event::KeyCode::Enter => {
            if !app.input.is_empty() {
                app.state.add_chat_log(format!("[user] {}", app.input));
                app.input.clear();
            }
        }
        crossterm::event::KeyCode::Char(c) => {
            app.input.push(c);
        }
        crossterm::event::KeyCode::Backspace => {
            app.input.pop();
        }
        _ => {}
    }
    Ok(())
}

fn draw(frame: &mut ratatui::Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(3),
        ])
        .split(frame.area());

    let tabs = Paragraph::new(vec![Line::from(vec![
        Span::styled(
            "[F1] Chat ",
            if app.tab == Tab::Chat {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default()
            },
        ),
        Span::styled(
            "[F2] Agents ",
            if app.tab == Tab::Agents {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default()
            },
        ),
        Span::styled(
            "[F3] Workers ",
            if app.tab == Tab::Workers {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default()
            },
        ),
        Span::styled(
            "[F4] Executions ",
            if app.tab == Tab::Executions {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default()
            },
        ),
        Span::raw("| q:quit r:run-agent p:ping"),
    ])])
    .block(Block::default().borders(Borders::ALL).title("Goble"));
    frame.render_widget(tabs, chunks[0]);

    let main = chunks[1];
    match app.tab {
        Tab::Chat => draw_chat(frame, app, main),
        Tab::Agents => draw_agents(frame, app, main),
        Tab::Workers => draw_workers(frame, app, main),
        Tab::Executions => draw_executions(frame, app, main),
    }

    let input = Paragraph::new(app.input.as_str())
        .block(Block::default().borders(Borders::ALL).title("Input"));
    frame.render_widget(input, chunks[2]);
}

fn draw_chat(frame: &mut ratatui::Frame, app: &App, area: Rect) {
    let logs = app.state.get_logs();
    let items: Vec<ListItem> = logs.iter().map(|l| ListItem::new(l.as_str())).collect();
    let list = List::new(items).block(Block::default().borders(Borders::ALL).title("Chat / Logs"));
    frame.render_widget(list, area);
}

fn draw_agents(frame: &mut ratatui::Frame, app: &App, area: Rect) {
    let agents = app.state.agents.lock();
    let items: Vec<ListItem> = agents
        .iter()
        .map(|a| ListItem::new(format!("{} - {}", a.name, a.id)))
        .collect();
    let list = List::new(items).block(Block::default().borders(Borders::ALL).title("Agents"));
    frame.render_widget(list, area);
}

fn draw_workers(frame: &mut ratatui::Frame, app: &App, area: Rect) {
    let workers = app.state.workers.lock();
    let items: Vec<ListItem> = workers
        .values()
        .map(|w| ListItem::new(format!("{} ({}) paired={}", w.name, w.url, w.paired)))
        .collect();
    let list = List::new(items).block(Block::default().borders(Borders::ALL).title("Workers"));
    frame.render_widget(list, area);
}

fn draw_executions(frame: &mut ratatui::Frame, app: &App, area: Rect) {
    let executions = app.state.executions.lock();
    if executions.is_empty() {
        let block = Paragraph::new("No executions yet. Press 'r' to run an agent.")
            .block(Block::default().borders(Borders::ALL).title("Executions"));
        frame.render_widget(block, area);
        return;
    }

    let mut lines: Vec<Line> = Vec::new();
    for trace in executions.values() {
        lines.push(Line::from(vec![
            Span::styled(format!("{} ", trace.id), Style::default().fg(Color::Yellow)),
            Span::raw(format!("{:?}", trace.status)),
        ]));
        let view = trace.sequential_view();
        for (depth, step) in view {
            let indent = "  ".repeat(depth);
            let status_color = match step.status {
                goble_core::execution::ExecutionStatus::Success => Color::Green,
                goble_core::execution::ExecutionStatus::Failure(_) => Color::Red,
                _ => Color::Gray,
            };
            lines.push(Line::from(vec![
                Span::raw(format!("{}{} ", indent, step.name)),
                Span::styled(
                    format!("{:?}", step.status),
                    Style::default().fg(status_color),
                ),
            ]));
            for log in &step.logs {
                lines.push(Line::from(vec![Span::raw(format!(
                    "{}  [{:?}] {}",
                    indent, log.level, log.message
                ))]));
            }
        }
        lines.push(Line::raw(""));
    }

    let block = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title("Executions"))
        .scroll((0, 0));
    frame.render_widget(block, area);
}
