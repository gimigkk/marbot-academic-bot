// src/tui/render.rs
//! Clean TUI implementation - renders trying line and animated countdown

use std::sync::Arc;
use std::time::Duration;
use std::io;

use crossterm::{
    event::{self, Event, KeyCode, MouseEventKind},
    terminal::{enable_raw_mode, disable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame, Terminal,
};

use super::state::{TuiState, JobEntry, JobStatus, GeneralLogLine};

#[derive(Debug, Clone, Copy, PartialEq)]
enum ViewMode {
    Tasks,
    General,
}

#[derive(Debug, Clone)]
struct ScrollState {
    offset: usize,
    auto_scroll: bool,
    last_offset: usize,
}

impl ScrollState {
    fn new() -> Self {
        Self {
            offset: 0,
            auto_scroll: true,
            last_offset: 0,
        }
    }

    fn scroll_up(&mut self, amount: usize) {
        self.auto_scroll = false;
        self.offset = self.offset.saturating_sub(amount);
    }

    fn scroll_down(&mut self, amount: usize, max_offset: usize) {
        self.auto_scroll = false;
        self.offset = (self.offset + amount).min(max_offset);
    }

    fn ensure_visible(&mut self, max_offset: usize) {
        if self.offset > max_offset {
            self.offset = max_offset;
        }
    }

    fn scroll_to_bottom(&mut self, max_offset: usize) {
        self.offset = max_offset;
        self.auto_scroll = true;
    }

    fn did_change(&mut self) -> bool {
        if self.offset != self.last_offset {
            self.last_offset = self.offset;
            true
        } else {
            false
        }
    }
}

struct AppState {
    view_mode: ViewMode,
    job_list_state: ListState,
    general_scroll: ScrollState,
    job_scroll: ScrollState,
    last_job_count: usize,
    animation_frame: usize,
    last_selected_job: Option<usize>,
}

impl Default for AppState {
    fn default() -> Self {
        let mut state = Self {
            view_mode: ViewMode::Tasks,
            job_list_state: ListState::default(),
            general_scroll: ScrollState::new(),
            job_scroll: ScrollState::new(),
            last_job_count: 0,
            animation_frame: 0,
            last_selected_job: None,
        };
        state.job_list_state.select(Some(0));
        state.last_selected_job = Some(0);
        state
    }
}

const SPINNER_FRAMES: &[&str; 10] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub async fn run_tui(tui_state: Arc<TuiState>) -> Result<(), Box<dyn std::error::Error>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;
    stdout.execute(crossterm::event::EnableMouseCapture)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.hide_cursor()?;
    terminal.clear()?;

    let result = run_app(&mut terminal, tui_state.clone()).await;

    disable_raw_mode()?;
    let mut stdout = terminal.backend_mut();
    stdout.execute(crossterm::event::DisableMouseCapture)?;
    stdout.execute(LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    if let Err(e) = result {
        eprintln!("TUI error: {:?}", e);
    }

    Ok(())
}

async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    tui_state: Arc<TuiState>,
) -> io::Result<()> {
    let mut app_state = AppState::default();
    let mut last_draw = std::time::Instant::now();
    let mut force_clear = false;

    loop {
        let jobs = tui_state.get_jobs().await;
        let general_log = tui_state.get_general_log().await;

        if jobs.len() > app_state.last_job_count {
            app_state.last_job_count = jobs.len();
            if app_state.view_mode != ViewMode::Tasks {
                app_state.view_mode = ViewMode::Tasks;
                force_clear = true;
            }
            app_state.job_list_state.select(Some(0));
            app_state.last_selected_job = Some(0);
            app_state.job_scroll = ScrollState::new();
        }

        let current_selected = app_state.job_list_state.selected();
        if current_selected != app_state.last_selected_job {
            app_state.last_selected_job = current_selected;
            force_clear = true;
        }

        if app_state.general_scroll.did_change() || app_state.job_scroll.did_change() {
            force_clear = true;
        }

        if last_draw.elapsed() > Duration::from_millis(100) {
            app_state.animation_frame = (app_state.animation_frame + 1) % SPINNER_FRAMES.len();
            last_draw = std::time::Instant::now();
        }

        if force_clear {
            terminal.clear()?;
            force_clear = false;
        }

        terminal.draw(|f| {
            render_ui(f, &jobs, &general_log, &mut app_state);
        })?;

        if event::poll(Duration::from_millis(16))? {
            match event::read()? {
                Event::Key(key) => {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => return Ok(()),

                        KeyCode::Char('t') => {
                            if app_state.view_mode != ViewMode::Tasks {
                                app_state.view_mode = ViewMode::Tasks;
                                force_clear = true;
                                if !jobs.is_empty() {
                                    app_state.job_list_state.select(Some(0));
                                }
                            }
                        }
                        KeyCode::Char('g') => {
                            if app_state.view_mode != ViewMode::General {
                                app_state.view_mode = ViewMode::General;
                                force_clear = true;
                            }
                        }

                        KeyCode::Down => {
                            if app_state.view_mode == ViewMode::Tasks && !jobs.is_empty() {
                                let current = app_state.job_list_state.selected().unwrap_or(0);
                                let next = if current >= jobs.len() - 1 { 0 } else { current + 1 };
                                app_state.job_list_state.select(Some(next));
                                app_state.job_scroll = ScrollState::new();
                            }
                        }
                        KeyCode::Up => {
                            if app_state.view_mode == ViewMode::Tasks && !jobs.is_empty() {
                                let current = app_state.job_list_state.selected().unwrap_or(0);
                                let prev = if current == 0 { jobs.len() - 1 } else { current - 1 };
                                app_state.job_list_state.select(Some(prev));
                                app_state.job_scroll = ScrollState::new();
                            }
                        }

                        KeyCode::Right => {
                            let viewport = terminal.size()?.height.saturating_sub(5) as usize;
                            let content_len = if app_state.view_mode == ViewMode::General {
                                general_log.len()
                            } else {
                                1000
                            };
                            let max_offset = content_len.saturating_sub(viewport);
                            
                            let scroll = match app_state.view_mode {
                                ViewMode::General => &mut app_state.general_scroll,
                                ViewMode::Tasks => &mut app_state.job_scroll,
                            };
                            scroll.scroll_down(1, max_offset);
                        }
                        KeyCode::Left => {
                            let scroll = match app_state.view_mode {
                                ViewMode::General => &mut app_state.general_scroll,
                                ViewMode::Tasks => &mut app_state.job_scroll,
                            };
                            scroll.scroll_up(1);
                        }
                        KeyCode::PageDown => {
                            let viewport = terminal.size()?.height.saturating_sub(5) as usize;
                            let content_len = if app_state.view_mode == ViewMode::General {
                                general_log.len()
                            } else {
                                1000
                            };
                            let max_offset = content_len.saturating_sub(viewport);
                            
                            let scroll = match app_state.view_mode {
                                ViewMode::General => &mut app_state.general_scroll,
                                ViewMode::Tasks => &mut app_state.job_scroll,
                            };
                            scroll.scroll_down(10, max_offset);
                        }
                        KeyCode::PageUp => {
                            let scroll = match app_state.view_mode {
                                ViewMode::General => &mut app_state.general_scroll,
                                ViewMode::Tasks => &mut app_state.job_scroll,
                            };
                            scroll.scroll_up(10);
                        }

                        KeyCode::Char('x') => {
                            tui_state.clear_completed().await;
                        }

                        _ => {}
                    }
                }

                Event::Mouse(mouse) => {
                    match mouse.kind {
                        MouseEventKind::ScrollDown => {
                            let viewport = terminal.size()?.height.saturating_sub(5) as usize;
                            let content_len = if app_state.view_mode == ViewMode::General {
                                general_log.len()
                            } else {
                                1000
                            };
                            let max_offset = content_len.saturating_sub(viewport);
                            
                            let scroll = match app_state.view_mode {
                                ViewMode::General => &mut app_state.general_scroll,
                                ViewMode::Tasks => &mut app_state.job_scroll,
                            };
                            scroll.scroll_down(3, max_offset);
                        }
                        MouseEventKind::ScrollUp => {
                            let scroll = match app_state.view_mode {
                                ViewMode::General => &mut app_state.general_scroll,
                                ViewMode::Tasks => &mut app_state.job_scroll,
                            };
                            scroll.scroll_up(3);
                        }
                        _ => {}
                    }
                }

                Event::Resize(_, _) => {
                    terminal.clear()?;
                }

                _ => {}
            }
        }

        tokio::time::sleep(Duration::from_millis(16)).await;
    }
}

fn render_ui(
    f: &mut Frame,
    jobs: &[JobEntry],
    general_log: &[GeneralLogLine],
    app_state: &mut AppState,
) {
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(3)])
        .split(f.size());

    let content_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
        .split(main_chunks[0]);

    render_left_panel(f, content_chunks[0], jobs, app_state);
    render_right_panel(f, content_chunks[1], jobs, general_log, app_state);
    render_footer(f, main_chunks[1]);
}

fn render_left_panel(
    f: &mut Frame,
    area: Rect,
    jobs: &[JobEntry],
    app_state: &mut AppState,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(6), Constraint::Min(0)])
        .split(area);

    render_tabs(f, chunks[0], app_state.view_mode);

    match app_state.view_mode {
        ViewMode::Tasks => render_job_list(f, chunks[1], jobs, app_state),
        ViewMode::General => render_stats(f, chunks[1], jobs),
    }
}

fn render_tabs(f: &mut Frame, area: Rect, view_mode: ViewMode) {
    let tabs = vec![
        ("T", "Tasks", ViewMode::Tasks),
        ("G", "General", ViewMode::General),
    ];

    let mut lines = vec![Line::from("")];

    for (key, label, mode) in tabs {
        let is_selected = view_mode == mode;
        let style = if is_selected {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let indicator = if is_selected { "●" } else { " " };

        lines.push(Line::from(vec![
            Span::styled(format!("[{}] ", key), style),
            Span::styled(indicator, style),
            Span::styled(format!(" {}", label), style),
        ]));
    }

    let widget = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title("📊 Navigation"));

    f.render_widget(widget, area);
}

fn render_stats(f: &mut Frame, area: Rect, jobs: &[JobEntry]) {
    let active = jobs.iter().filter(|j| matches!(j.status, JobStatus::Active)).count();
    let completed = jobs.iter().filter(|j| matches!(j.status, JobStatus::Completed)).count();
    let failed = jobs.iter().filter(|j| matches!(j.status, JobStatus::Failed)).count();

    let stats = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("Active:    ", Style::default().fg(Color::Gray)),
            Span::styled(format!("{}", active), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("Completed: ", Style::default().fg(Color::Gray)),
            Span::styled(format!("{}", completed), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("Failed:    ", Style::default().fg(Color::Gray)),
            Span::styled(format!("{}", failed), Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
        ]),
    ];

    let widget = Paragraph::new(stats)
        .block(Block::default().borders(Borders::ALL).title("Statistics"));
    f.render_widget(widget, area);
}

fn render_job_list(
    f: &mut Frame,
    area: Rect,
    jobs: &[JobEntry],
    app_state: &mut AppState,
) {
    let mut sorted_jobs: Vec<_> = jobs.iter().collect();
    sorted_jobs.sort_by(|a, b| b.started_at.cmp(&a.started_at));

    let items: Vec<ListItem> = sorted_jobs.iter()
        .map(|job| {
            let (icon, icon_color) = match job.status {
                JobStatus::Active => {
                    let spinner = SPINNER_FRAMES[app_state.animation_frame];
                    (spinner, Color::Green)
                }
                JobStatus::Completed => ("✓", Color::Cyan),
                JobStatus::Failed => ("✗", Color::Red),
            };

            let status_text = if let Some(cd) = &job.current_countdown {
                format!(" ⏳{}s", cd.remaining)
            } else {
                String::new()
            };

            let chat: String = job.chat_id.chars().take(15).collect();
            let dur = format!("{:.1}s", job.duration().as_secs_f32());

            ListItem::new(Line::from(vec![
                Span::styled(format!("{} ", icon), Style::default().fg(icon_color).add_modifier(Modifier::BOLD)),
                Span::styled(chat, Style::default().fg(Color::White)),
                Span::styled(format!(" ({})", dur), Style::default().fg(Color::DarkGray)),
                Span::styled(status_text, Style::default().fg(Color::Cyan)),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(format!("Tasks ({})", sorted_jobs.len())))
        .highlight_style(Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD))
        .highlight_symbol("▶ ");

    f.render_stateful_widget(list, area, &mut app_state.job_list_state);
}

fn render_right_panel(
    f: &mut Frame,
    area: Rect,
    jobs: &[JobEntry],
    general_log: &[GeneralLogLine],
    app_state: &mut AppState,
) {
    match app_state.view_mode {
        ViewMode::General => render_general_log(f, area, general_log, app_state),
        ViewMode::Tasks => {
            if let Some(selected_idx) = app_state.job_list_state.selected() {
                let mut sorted_jobs: Vec<_> = jobs.iter().collect();
                sorted_jobs.sort_by(|a, b| b.started_at.cmp(&a.started_at));
                
                if selected_idx < sorted_jobs.len() {
                    render_job_detail(f, area, sorted_jobs[selected_idx], app_state);
                } else {
                    render_empty_state(f, area);
                }
            } else {
                render_empty_state(f, area);
            }
        }
    }
}

fn render_general_log(
    f: &mut Frame,
    area: Rect,
    log: &[GeneralLogLine],
    app_state: &mut AppState,
) {
    let inner_height = area.height.saturating_sub(2) as usize;

    let lines: Vec<Line> = log.iter()
        .map(|entry| {
            let job_num = entry.job_id.trim_start_matches("req_");
            Line::from(format!("[#{}] {}", job_num, entry.message))
        })
        .collect();

    let max_offset = lines.len().saturating_sub(inner_height);
    
    if app_state.general_scroll.auto_scroll {
        app_state.general_scroll.scroll_to_bottom(max_offset);
    } else {
        app_state.general_scroll.ensure_visible(max_offset);
    }

    let start = app_state.general_scroll.offset;
    let end = (start + inner_height).min(lines.len());
    
    let mut visible: Vec<Line> = lines[start..end].to_vec();
    while visible.len() < inner_height {
        visible.push(Line::from(""));
    }

    let scroll_indicator = if lines.len() > inner_height {
        let percent = if max_offset == 0 {
            100
        } else {
            (start * 100) / max_offset
        };
        format!(" {}%", percent)
    } else {
        String::new()
    };

    let paragraph = Paragraph::new(visible)
        .block(Block::default()
            .borders(Borders::ALL)
            .title(format!("📋 General Log{}", scroll_indicator)))
        .wrap(Wrap { trim: false });

    f.render_widget(paragraph, area);
}

fn render_job_detail(
    f: &mut Frame,
    area: Rect,
    job: &JobEntry,
    app_state: &mut AppState,
) {
    let inner_height = area.height.saturating_sub(2) as usize;

    let mut lines: Vec<Line> = job.logs.iter()
        .map(|msg| Line::from(msg.as_str()))
        .collect();

    // Add TRYING line if present (overwrites position)
    if let Some(ref trying_msg) = job.current_trying {
        lines.push(Line::from(vec![
            Span::styled(trying_msg, Style::default().fg(Color::Cyan)),
        ]));
    }

    // Add countdown if present (live animated)
    if let Some(cd) = &job.current_countdown {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("⏳ RETRY #", Style::default().fg(Color::Yellow)),
            Span::styled(format!("{}", cd.attempt), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(format!(" - Waiting {} seconds...", cd.remaining), Style::default().fg(Color::Yellow)),
        ]));
    }

    let max_offset = lines.len().saturating_sub(inner_height);
    
    if app_state.job_scroll.auto_scroll {
        app_state.job_scroll.scroll_to_bottom(max_offset);
    } else {
        app_state.job_scroll.ensure_visible(max_offset);
    }

    let start = app_state.job_scroll.offset;
    let end = (start + inner_height).min(lines.len());
    
    let mut visible: Vec<Line> = lines[start..end].to_vec();
    while visible.len() < inner_height {
        visible.push(Line::from(""));
    }

    let scroll_indicator = if lines.len() > inner_height {
        let percent = if max_offset == 0 {
            100
        } else {
            (start * 100) / max_offset
        };
        format!(" {}%", percent)
    } else {
        String::new()
    };

    let title = format!("Task Details{}", scroll_indicator);

    let paragraph = Paragraph::new(visible)
        .block(Block::default().borders(Borders::ALL).title(title))
        .wrap(Wrap { trim: false });

    f.render_widget(paragraph, area);
}

fn render_empty_state(f: &mut Frame, area: Rect) {
    let text = vec![
        Line::from(""),
        Line::from(""),
        Line::from(Span::styled("No tasks yet", Style::default().fg(Color::DarkGray))),
        Line::from(""),
        Line::from(Span::styled("Tasks will appear here when jobs start", Style::default().fg(Color::DarkGray))),
    ];

    let paragraph = Paragraph::new(text)
        .block(Block::default().borders(Borders::ALL).title("Task Details"))
        .alignment(ratatui::layout::Alignment::Center);

    f.render_widget(paragraph, area);
}

fn render_footer(f: &mut Frame, area: Rect) {
    let help = Line::from(vec![
        Span::styled("Q", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::raw(":Quit | "),
        Span::styled("T/G", Style::default().fg(Color::Cyan)),
        Span::raw(":Tabs | "),
        Span::styled("↑↓", Style::default().fg(Color::Cyan)),
        Span::raw(":Select | "),
        Span::styled("←→", Style::default().fg(Color::Cyan)),
        Span::raw(":Scroll | "),
        Span::styled("X", Style::default().fg(Color::Red)),
        Span::raw(":Clear Done"),
    ]);

    let footer = Paragraph::new(help)
        .block(Block::default().borders(Borders::ALL));

    f.render_widget(footer, area);
}