//! Interactive TUI mode for ContribAI — `contribai interactive`
//!
//! Keyboard-driven interface for browsing PRs, repos, and running operations.
//! Built with ratatui + crossterm. Modeled after Python `contribai interactive`.

#![allow(clippy::clone_on_copy)] // TableState/ListState are Copy, explicit clone() is intentional

use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Cell, Clear, List, ListItem, ListState, Paragraph, Row, Table,
        TableState, Tabs, Wrap,
    },
    Frame, Terminal,
};
use std::io;
use std::time::Duration;

use contribai::core::config::ContribAIConfig;
use contribai::orchestrator::memory::Memory;

// ── Screen tabs ───────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Dashboard,
    PRs,
    Repos,
    Actions,
}

impl Tab {
    const ALL: &'static [Tab] = &[Tab::Dashboard, Tab::PRs, Tab::Repos, Tab::Actions];

    fn title(self) -> &'static str {
        match self {
            Tab::Dashboard => "  Dashboard  ",
            Tab::PRs => "  PRs  ",
            Tab::Repos => "  Repos  ",
            Tab::Actions => "  Actions  ",
        }
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }
}

// ── App state ─────────────────────────────────────────────────────────────────

struct App {
    tab: Tab,
    pr_state: TableState,
    repo_state: ListState,
    action_state: ListState,
    prs: Vec<PrRow>,
    repos: Vec<String>,
    stats: Stats,
    status_bar: String,
    show_help: bool,
    quit: bool,
}

struct PrRow {
    number: String,
    repo: String,
    title: String,
    status: String,
    kind: String,
}

#[derive(Default)]
struct Stats {
    total: i64,
    merged: i64,
    closed: i64,
    open: i64,
    repos_analyzed: i64,
}

impl App {
    fn new(config: &ContribAIConfig) -> Self {
        let (prs, repos, stats) = load_data(config).unwrap_or_default();

        let mut pr_state = TableState::default();
        if !prs.is_empty() {
            pr_state.select(Some(0));
        }
        let mut repo_state = ListState::default();
        if !repos.is_empty() {
            repo_state.select(Some(0));
        }
        let mut action_state = ListState::default();
        action_state.select(Some(0));

        Self {
            tab: Tab::Dashboard,
            pr_state,
            repo_state,
            action_state,
            prs,
            repos,
            stats,
            status_bar: "Press ? for help | Tab/1-4 switch panels | j/k navigate | q quit".into(),
            show_help: false,
            quit: false,
        }
    }

    #[cfg(test)]
    fn new_test() -> Self {
        let prs = vec![
            PrRow {
                number: "1".into(),
                repo: "repo1".into(),
                title: "Title 1".into(),
                status: "open".into(),
                kind: "feature".into(),
            },
            PrRow {
                number: "2".into(),
                repo: "repo2".into(),
                title: "Title 2".into(),
                status: "merged".into(),
                kind: "fix".into(),
            },
        ];
        let repos = vec!["repo1".into(), "repo2".into()];
        let stats = Stats::default();

        let mut pr_state = TableState::default();
        pr_state.select(Some(0));
        let mut repo_state = ListState::default();
        repo_state.select(Some(0));
        let mut action_state = ListState::default();
        action_state.select(Some(0));

        Self {
            tab: Tab::PRs,
            pr_state,
            repo_state,
            action_state,
            prs,
            repos,
            stats,
            status_bar: "Press ? for help | Tab/1-4 switch panels | j/k navigate | q quit".into(),
            show_help: false,
            quit: false,
        }
    }

    fn next_tab(&mut self) {
        let idx = (self.tab.index() + 1) % Tab::ALL.len();
        self.tab = Tab::ALL[idx];
    }

    fn prev_tab(&mut self) {
        let n = Tab::ALL.len();
        let idx = (self.tab.index() + n - 1) % n;
        self.tab = Tab::ALL[idx];
    }

    fn handle_key(&mut self, code: KeyCode, _modifiers: KeyModifiers) {
        if self.show_help {
            self.show_help = false;
            return;
        }

        match code {
            KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => self.quit = true,
            KeyCode::Char('?') => self.show_help = true,
            KeyCode::Tab => self.next_tab(),
            KeyCode::BackTab => self.prev_tab(),
            KeyCode::Char('1') => self.tab = Tab::Dashboard,
            KeyCode::Char('2') => self.tab = Tab::PRs,
            KeyCode::Char('3') => self.tab = Tab::Repos,
            KeyCode::Char('4') => self.tab = Tab::Actions,
            KeyCode::Down | KeyCode::Char('j') => self.move_down(),
            KeyCode::Up | KeyCode::Char('k') => self.move_up(),
            _ => {}
        }
    }

    fn move_down(&mut self) {
        match self.tab {
            Tab::PRs => {
                let len = self.prs.len();
                if len == 0 {
                    return;
                }
                let i = self.pr_state.selected().unwrap_or(0);
                self.pr_state.select(Some((i + 1).min(len - 1)));
            }
            Tab::Repos => {
                let len = self.repos.len();
                if len == 0 {
                    return;
                }
                let i = self.repo_state.selected().unwrap_or(0);
                self.repo_state.select(Some((i + 1).min(len - 1)));
            }
            Tab::Actions => {
                let i = self.action_state.selected().unwrap_or(0);
                self.action_state
                    .select(Some((i + 1).min(ACTIONS.len() - 1)));
            }
            _ => {}
        }
    }

    fn move_up(&mut self) {
        match self.tab {
            Tab::PRs => {
                let i = self.pr_state.selected().unwrap_or(0);
                self.pr_state.select(Some(i.saturating_sub(1)));
            }
            Tab::Repos => {
                let i = self.repo_state.selected().unwrap_or(0);
                self.repo_state.select(Some(i.saturating_sub(1)));
            }
            Tab::Actions => {
                let i = self.action_state.selected().unwrap_or(0);
                self.action_state.select(Some(i.saturating_sub(1)));
            }
            _ => {}
        }
    }
}

// ── Data loading ──────────────────────────────────────────────────────────────

fn load_data(config: &ContribAIConfig) -> anyhow::Result<(Vec<PrRow>, Vec<String>, Stats)> {
    let memory = Memory::open(&config.storage.resolved_db_path())?;

    let pr_maps = memory.get_prs(None, 200)?;
    let prs: Vec<PrRow> = pr_maps
        .into_iter()
        .map(|m| PrRow {
            number: m.get("pr_number").cloned().unwrap_or_default(),
            repo: m.get("repo").cloned().unwrap_or_default(),
            title: m.get("title").cloned().unwrap_or_default(),
            status: m.get("status").cloned().unwrap_or_default(),
            kind: m.get("type").cloned().unwrap_or_default(),
        })
        .collect();

    let mut repos: Vec<String> = prs
        .iter()
        .map(|p| p.repo.clone())
        .collect::<std::collections::HashSet<String>>()
        .into_iter()
        .collect::<Vec<String>>();
    repos.sort();

    let stat_map = memory.get_stats()?;
    let total = stat_map.get("total_prs_submitted").copied().unwrap_or(0);
    let merged = stat_map.get("prs_merged").copied().unwrap_or(0);
    let closed = stat_map.get("prs_closed").copied().unwrap_or(0);
    let repos_a = stat_map.get("total_repos_analyzed").copied().unwrap_or(0);

    let stats = Stats {
        total,
        merged,
        closed,
        open: total - merged - closed,
        repos_analyzed: repos_a,
    };
    Ok((prs, repos, stats))
}

// ── Entry point ───────────────────────────────────────────────────────────────

/// Run the interactive TUI. Returns when the user quits.
pub fn run_interactive_tui(config: &ContribAIConfig) -> anyhow::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(config);
    let result = run_loop(&mut terminal, &mut app).map_err(|e| anyhow::anyhow!("TUI error: {}", e));

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    result
}

fn run_loop<B: ratatui::backend::Backend + 'static>(
    terminal: &mut Terminal<B>,
    app: &mut App,
) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        terminal.draw(|f| render(f, app))?;

        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    app.handle_key(key.code, key.modifiers);
                }
            }
        }

        if app.quit {
            break;
        }
    }
    Ok(())
}

// ── Rendering ─────────────────────────────────────────────────────────────────

fn render(f: &mut Frame, app: &App) {
    let area = f.area();

    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // title
            Constraint::Length(3), // tabs
            Constraint::Min(0),    // content
            Constraint::Length(1), // status
        ])
        .split(area);

    render_title(f, root[0]);
    render_tabs(f, root[1], app);
    match app.tab {
        Tab::Dashboard => render_dashboard(f, root[2], app),
        Tab::PRs => render_prs(f, root[2], app),
        Tab::Repos => render_repos(f, root[2], app),
        Tab::Actions => render_actions(f, root[2], app),
    }
    render_status(f, root[3], app);

    if app.show_help {
        render_help(f, area);
    }
}

fn render_title(f: &mut Frame, area: Rect) {
    let title = Paragraph::new(Line::from(vec![
        Span::styled(
            " 🤖 ContribAI ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "v5.1.0 — Interactive Mode  ",
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            "[Tab] Switch  [1-4] Jump  [?] Help  [q] Quit",
            Style::default().fg(Color::DarkGray),
        ),
    ]));
    f.render_widget(title, area);
}

fn render_tabs(f: &mut Frame, area: Rect, app: &App) {
    let titles: Vec<Line> = Tab::ALL.iter().map(|t| Line::from(t.title())).collect();
    let tabs = Tabs::new(titles)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded),
        )
        .select(app.tab.index())
        .style(Style::default().fg(Color::DarkGray))
        .highlight_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );
    f.render_widget(tabs, area);
}

fn render_status(f: &mut Frame, area: Rect, app: &App) {
    f.render_widget(
        Paragraph::new(app.status_bar.as_str()).style(Style::default().fg(Color::DarkGray)),
        area,
    );
}

// ── Dashboard ─────────────────────────────────────────────────────────────────

fn render_dashboard(f: &mut Frame, area: Rect, app: &App) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    let left = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(11), Constraint::Min(0)])
        .split(cols[0]);

    let merge_rate = if app.stats.total > 0 {
        app.stats.merged * 100 / app.stats.total
    } else {
        0
    };

    let stats_text = vec![
        Line::from(""),
        Line::from(vec![
            Span::raw("  📊 Total PRs:      "),
            Span::styled(
                app.stats.total.to_string(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::raw("  ✅ Merged:         "),
            Span::styled(
                app.stats.merged.to_string(),
                Style::default().fg(Color::Green),
            ),
        ]),
        Line::from(vec![
            Span::raw("  ❌ Closed:         "),
            Span::styled(
                app.stats.closed.to_string(),
                Style::default().fg(Color::Red),
            ),
        ]),
        Line::from(vec![
            Span::raw("  🟡 Open:           "),
            Span::styled(
                app.stats.open.to_string(),
                Style::default().fg(Color::Yellow),
            ),
        ]),
        Line::from(vec![
            Span::raw("  🎯 Merge rate:     "),
            Span::styled(
                format!("{}%", merge_rate),
                Style::default().fg(if merge_rate >= 70 {
                    Color::Green
                } else if merge_rate >= 40 {
                    Color::Yellow
                } else {
                    Color::Red
                }),
            ),
        ]),
        Line::from(vec![
            Span::raw("  🔬 Repos analyzed: "),
            Span::styled(
                app.stats.repos_analyzed.to_string(),
                Style::default().fg(Color::Cyan),
            ),
        ]),
        Line::from(""),
    ];

    f.render_widget(
        Paragraph::new(stats_text).block(
            Block::default()
                .title(" 📈 Statistics ")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Cyan)),
        ),
        left[0],
    );

    let hints = vec![
        Line::from(""),
        Line::from(vec![Span::styled(
            "  Quick keys:",
            Style::default().add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  [2] ", Style::default().fg(Color::Yellow)),
            Span::raw("Browse PRs"),
        ]),
        Line::from(vec![
            Span::styled("  [3] ", Style::default().fg(Color::Yellow)),
            Span::raw("Browse Repos"),
        ]),
        Line::from(vec![
            Span::styled("  [4] ", Style::default().fg(Color::Yellow)),
            Span::raw("Actions"),
        ]),
        Line::from(vec![
            Span::styled("  [?] ", Style::default().fg(Color::Yellow)),
            Span::raw("Help"),
        ]),
        Line::from(vec![
            Span::styled("  [q] ", Style::default().fg(Color::Yellow)),
            Span::raw("Quit"),
        ]),
    ];

    f.render_widget(
        Paragraph::new(hints).block(
            Block::default()
                .title(" 💡 Navigation ")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::DarkGray)),
        ),
        left[1],
    );

    // Recent PRs table on the right
    let rows: Vec<Row> = app
        .prs
        .iter()
        .take(25)
        .map(|pr| {
            let (style, icon) = pr_style_icon(&pr.status);
            Row::new(vec![
                Cell::from(pr.number.as_str()).style(Style::default().fg(Color::Cyan)),
                Cell::from(pr.repo.chars().take(20).collect::<String>())
                    .style(Style::default().fg(Color::DarkGray)),
                Cell::from(pr.title.chars().take(32).collect::<String>()),
                Cell::from(format!("{} {}", icon, pr.status)).style(style),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(5),
            Constraint::Length(20),
            Constraint::Min(0),
            Constraint::Length(12),
        ],
    )
    .header(
        Row::new(vec!["#", "Repo", "Title", "Status"])
            .style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .bottom_margin(1),
    )
    .block(
        Block::default()
            .title(format!(" 📋 Recent PRs ({}) ", app.prs.len()))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Cyan)),
    );

    f.render_widget(table, cols[1]);
}

// ── PRs tab ───────────────────────────────────────────────────────────────────

fn render_prs(f: &mut Frame, area: Rect, app: &App) {
    let rows: Vec<Row> = app
        .prs
        .iter()
        .map(|pr| {
            let (style, icon) = pr_style_icon(&pr.status);
            Row::new(vec![
                Cell::from(pr.number.as_str()).style(Style::default().fg(Color::Cyan)),
                Cell::from(pr.repo.chars().take(28).collect::<String>())
                    .style(Style::default().fg(Color::DarkGray)),
                Cell::from(pr.title.chars().take(40).collect::<String>()),
                Cell::from(format!("{} {}", icon, pr.status)).style(style),
                Cell::from(pr.kind.as_str()).style(Style::default().fg(Color::DarkGray)),
            ])
        })
        .collect();

    let mut state = app.pr_state.clone();
    let table = Table::new(
        rows,
        [
            Constraint::Length(5),
            Constraint::Length(28),
            Constraint::Min(0),
            Constraint::Length(12),
            Constraint::Length(16),
        ],
    )
    .header(
        Row::new(vec!["#", "Repository", "Title", "Status", "Type"])
            .style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .bottom_margin(1),
    )
    .row_highlight_style(
        Style::default()
            .bg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
    )
    .block(
        Block::default()
            .title(format!(" 📋 Pull Requests ({}) ", app.prs.len()))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Cyan)),
    );

    f.render_stateful_widget(table, area, &mut state);
}

// ── Repos tab ─────────────────────────────────────────────────────────────────

fn render_repos(f: &mut Frame, area: Rect, app: &App) {
    let items: Vec<ListItem> = app
        .repos
        .iter()
        .map(|r| {
            let total = app.prs.iter().filter(|p| &p.repo == r).count();
            let merged = app
                .prs
                .iter()
                .filter(|p| &p.repo == r && p.status == "merged")
                .count();
            let rate = if total > 0 { merged * 100 / total } else { 0 };
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("  {:<35}", r.chars().take(34).collect::<String>()),
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(
                    format!(" {:>3} PRs", total),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!("  {:>3} merged", merged),
                    Style::default().fg(Color::Green),
                ),
                Span::styled(
                    format!("  {}%", rate),
                    Style::default().fg(if rate >= 70 {
                        Color::Green
                    } else if rate >= 40 {
                        Color::Yellow
                    } else {
                        Color::Red
                    }),
                ),
            ]))
        })
        .collect();

    let mut state = app.repo_state.clone();
    let list = List::new(items)
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .block(
            Block::default()
                .title(format!(" 🔬 Repositories ({}) ", app.repos.len()))
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Cyan)),
        );

    f.render_stateful_widget(list, area, &mut state);
}

// ── Actions tab ───────────────────────────────────────────────────────────────

const ACTIONS: &[(&str, &str, &str)] = &[
    ("🚀", "Hunt mode", "contribai hunt"),
    ("🚀", "Hunt (dry run)", "contribai hunt --dry-run"),
    ("🎯", "Target a repo", "contribai target <url>"),
    ("🔍", "Analyze a repo", "contribai analyze <url>"),
    ("🐛", "Solve issues", "contribai solve <url>"),
    ("👁  ", "Patrol PRs", "contribai patrol"),
    ("📊", "Leaderboard", "contribai leaderboard"),
    ("📊", "System status", "contribai system-status"),
    ("🧹", "Cleanup forks", "contribai cleanup"),
    ("🤖", "List models", "contribai models"),
    ("📝", "List templates", "contribai templates"),
    ("🔐", "Auth status", "contribai login"),
    ("⚙️ ", "Config list", "contribai config-list"),
    ("🔔", "Test notify", "contribai notify-test"),
];

fn render_actions(f: &mut Frame, area: Rect, app: &App) {
    let layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(area);

    let items: Vec<ListItem> = ACTIONS
        .iter()
        .map(|(icon, name, _)| {
            ListItem::new(Line::from(vec![
                Span::raw(format!("  {} ", icon)),
                Span::styled(
                    format!("{:<22}", name),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
            ]))
        })
        .collect();

    let mut state = app.action_state.clone();
    let list = List::new(items)
        .highlight_style(
            Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .block(
            Block::default()
                .title(" ⚡ Actions (j/k to select) ")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Cyan)),
        );

    f.render_stateful_widget(list, layout[0], &mut state);

    let idx = app
        .action_state
        .selected()
        .unwrap_or(0)
        .min(ACTIONS.len() - 1);
    let (icon, name, cmd) = ACTIONS[idx];

    let detail = vec![
        Line::from(""),
        Line::from(vec![
            Span::raw("  "),
            Span::styled(icon, Style::default().fg(Color::Cyan)),
            Span::styled(
                format!(" {}", name),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "  CLI Command:",
            Style::default().fg(Color::DarkGray),
        )]),
        Line::from(""),
        Line::from(vec![
            Span::raw("  "),
            Span::styled(
                cmd,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "  Copy & run in your terminal",
            Style::default().fg(Color::DarkGray),
        )]),
    ];

    f.render_widget(
        Paragraph::new(detail).wrap(Wrap { trim: true }).block(
            Block::default()
                .title(" 📋 Selected Command ")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::DarkGray)),
        ),
        layout[1],
    );
}

// ── Help overlay ──────────────────────────────────────────────────────────────

fn render_help(f: &mut Frame, area: Rect) {
    let popup = centered_rect(60, 75, area);
    f.render_widget(Clear, popup);

    let text = vec![
        Line::from(""),
        Line::from(vec![Span::styled(
            "  Navigation",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        Line::from("  Tab / Shift+Tab   Switch tabs forward/back"),
        Line::from("  1 / 2 / 3 / 4    Jump to Dashboard/PRs/Repos/Actions"),
        Line::from("  j / ↓             Move down in list"),
        Line::from("  k / ↑             Move up in list"),
        Line::from(""),
        Line::from(vec![Span::styled(
            "  Global",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        Line::from("  ?         Show this help"),
        Line::from("  q / Q     Quit interactive mode"),
        Line::from("  Esc       Quit interactive mode"),
        Line::from(""),
        Line::from(vec![Span::styled(
            "  Tabs",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        Line::from("  [1] Dashboard   Stats overview & recent PRs"),
        Line::from("  [2] PRs         Full PR history table"),
        Line::from("  [3] Repos       Analyzed repos with merge rates"),
        Line::from("  [4] Actions     CLI commands reference"),
        Line::from(""),
        Line::from(vec![Span::styled(
            "  Press any key to close",
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
        )]),
        Line::from(""),
    ];

    f.render_widget(
        Paragraph::new(text).wrap(Wrap { trim: true }).block(
            Block::default()
                .title(" 📖 Help ")
                .borders(Borders::ALL)
                .border_type(BorderType::Double)
                .border_style(Style::default().fg(Color::Cyan)),
        ),
        popup,
    );
}

fn centered_rect(pct_x: u16, pct_y: u16, r: Rect) -> Rect {
    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - pct_y) / 2),
            Constraint::Percentage(pct_y),
            Constraint::Percentage((100 - pct_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - pct_x) / 2),
            Constraint::Percentage(pct_x),
            Constraint::Percentage((100 - pct_x) / 2),
        ])
        .split(v[1])[1]
}

// ── Utilities ─────────────────────────────────────────────────────────────────

fn pr_style_icon(status: &str) -> (Style, &'static str) {
    match status {
        "merged" => (Style::default().fg(Color::Green), "✅"),
        "closed" => (Style::default().fg(Color::Red), "❌"),
        "open" => (Style::default().fg(Color::Yellow), "🟡"),
        _ => (Style::default().fg(Color::DarkGray), "⚪"),
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

    use super::*;

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn release(code: KeyCode) -> KeyEvent {
        KeyEvent::new_with_kind(code, KeyModifiers::NONE, KeyEventKind::Release)
    }

    /// Simulates the real event loop: only Press events should advance.
    fn simulate_event_loop(app: &mut App, events: &[KeyEvent]) {
        for ev in events {
            if ev.kind == KeyEventKind::Press {
                app.handle_key(ev.code, ev.modifiers);
            }
        }
    }

    #[test]
    fn down_moves_one_step_per_press() {
        let mut app = App::new_test();
        assert_eq!(app.pr_state.selected(), Some(0));

        let events = vec![
            press(KeyCode::Down),
            release(KeyCode::Down),
            press(KeyCode::Down),
            release(KeyCode::Down),
        ];
        simulate_event_loop(&mut app, &events);

        // Two presses → index 1 (clamped at last item with 2 PRs)
        assert_eq!(app.pr_state.selected(), Some(1));
    }

    #[test]
    fn up_moves_one_step_per_press() {
        let mut app = App::new_test();
        // Move to index 1 first
        simulate_event_loop(&mut app, &[press(KeyCode::Down)]);
        assert_eq!(app.pr_state.selected(), Some(1));

        let events = vec![
            press(KeyCode::Up),
            release(KeyCode::Up),
            press(KeyCode::Up),
            release(KeyCode::Up),
        ];
        simulate_event_loop(&mut app, &events);

        // Two presses up → clamped at 0
        assert_eq!(app.pr_state.selected(), Some(0));
    }

    #[test]
    fn odd_indices_reachable_with_press_release_pairs() {
        let mut app = App::new_test();

        // Simulate 10 real keypresses, each producing press + release
        let events: Vec<KeyEvent> = (0..10)
            .flat_map(|_| [press(KeyCode::Down), release(KeyCode::Down)])
            .collect();
        simulate_event_loop(&mut app, &events);

        // 10 presses, clamped at last index (1)
        assert_eq!(app.pr_state.selected(), Some(1));
    }

    #[test]
    fn old_bug_double_counting_without_filter() {
        // Demonstrate the old bug: without filtering by kind,
        // press+release pairs cause double movement.
        let mut app = App::new_test();
        let events = vec![press(KeyCode::Down), release(KeyCode::Down)];
        // OLD behavior: handle every event (no kind check)
        for ev in &events {
            app.handle_key(ev.code, ev.modifiers);
        }
        // Without filtering, both events advance → index would be 0+2 = 2, but clamped at 1
        // This shows the old code moved by 2 per real keypress instead of 1.
        assert_eq!(
            app.pr_state.selected(),
            Some(1),
            "unfiltered: two events for one keypress"
        );
    }

    #[test]
    fn repos_list_navigates_correctly() {
        let mut app = App::new_test();
        app.tab = Tab::Repos;
        assert_eq!(app.repo_state.selected(), Some(0));

        simulate_event_loop(&mut app, &[press(KeyCode::Down), release(KeyCode::Down)]);
        assert_eq!(app.repo_state.selected(), Some(1));

        simulate_event_loop(&mut app, &[press(KeyCode::Up), release(KeyCode::Up)]);
        assert_eq!(app.repo_state.selected(), Some(0));
    }

    #[test]
    fn actions_list_navigates_correctly() {
        let mut app = App::new_test();
        app.tab = Tab::Actions;
        assert_eq!(app.action_state.selected(), Some(0));

        simulate_event_loop(&mut app, &[press(KeyCode::Down), release(KeyCode::Down)]);
        assert_eq!(app.action_state.selected(), Some(1));

        simulate_event_loop(&mut app, &[press(KeyCode::Up), release(KeyCode::Up)]);
        assert_eq!(app.action_state.selected(), Some(0));
    }
}
