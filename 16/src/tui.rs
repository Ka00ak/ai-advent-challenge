//! TUI на ratatui: ровно две вкладки — «1 Чат» и «2 API».
//! В API-вкладке видно точное тело запроса к модели, включая схемы
//! локальных инструментов и GitHub MCP.

use std::io::stdout;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::agent::{Agent, AgentEvent, TurnResult};
use crate::catalog::ToolCatalog;
use crate::domain::{ChatMessage, Exchange, Role};
use crate::llm::ReqwestClient;
use crate::settings::{Settings, load_api_key};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Chat,
    Api,
}

impl Tab {
    pub const ALL: [Tab; 2] = [Tab::Chat, Tab::Api];

    pub fn number(self) -> usize {
        match self {
            Tab::Chat => 1,
            Tab::Api => 2,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Tab::Chat => "Чат",
            Tab::Api => "API",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiFocus {
    List,
    Body,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiSide {
    Request,
    Response,
}

impl ApiSide {
    pub fn title(self) -> &'static str {
        match self {
            ApiSide::Request => "Запрос",
            ApiSide::Response => "Ответ",
        }
    }

    fn other(self) -> Self {
        match self {
            ApiSide::Request => ApiSide::Response,
            ApiSide::Response => ApiSide::Request,
        }
    }
}

#[derive(Debug)]
enum AppEvent {
    Started,
    Agent(AgentEvent),
    Finished(Result<TurnResult, String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Action {
    None,
    Quit,
    Send(String),
}

pub struct App {
    pub settings: Settings,
    pub catalog: ToolCatalog,
    pub history: Vec<ChatMessage>,
    pub exchanges: Vec<Exchange>,
    pub tab: Tab,
    pub chat_input: String,
    pub chat_focused: bool,
    pub chat_scroll: u16,
    pub chat_follow: bool,
    pub pending: Option<String>,
    pub busy: bool,
    pub status: String,
    pub banner: Option<(String, bool)>,
    pub api_selection: usize,
    pub api_focus: ApiFocus,
    pub api_side: ApiSide,
    pub api_scroll: u16,
    pub should_quit: bool,
}

impl App {
    pub fn new(settings: Settings, catalog: ToolCatalog) -> Self {
        Self {
            settings,
            catalog,
            history: Vec::new(),
            exchanges: Vec::new(),
            tab: Tab::Chat,
            chat_input: String::new(),
            chat_focused: false,
            chat_scroll: 0,
            chat_follow: true,
            pending: None,
            busy: false,
            status: "готов".into(),
            banner: None,
            api_selection: 0,
            api_focus: ApiFocus::List,
            api_side: ApiSide::Request,
            api_scroll: 0,
            should_quit: false,
        }
    }

    pub fn begin_chat(&mut self, text: String) {
        self.history.push(ChatMessage::user(text.clone()));
        self.pending = Some(text);
        self.busy = true;
        self.banner = None;
        self.chat_follow = true;
        self.chat_input.clear();
        self.chat_focused = false;
        self.status = "запрос к модели…".into();
    }

    pub fn finish_chat(&mut self, result: TurnResult) {
        self.history = result.history;
        self.pending = None;
        self.busy = false;
        self.chat_follow = true;
        self.status = format!(
            "ответ · {} шаг(ов) · {} токенов",
            result.steps, result.usage.total_tokens
        );
    }

    pub fn fail_chat(&mut self, error: String) {
        self.pending = None;
        self.busy = false;
        self.banner = Some((error, true));
        self.status = "ошибка запроса".into();
    }

    pub fn push_exchange(&mut self, exchange: Exchange) {
        let follow = self.api_selection + 1 >= self.exchanges.len();
        self.exchanges.push(exchange);
        if follow {
            self.api_selection = self.exchanges.len().saturating_sub(1);
            self.api_focus = ApiFocus::List;
            self.api_side = ApiSide::Request;
            self.api_scroll = 0;
        }
    }

    pub fn selected_exchange(&self) -> Option<&Exchange> {
        self.exchanges.get(self.api_selection)
    }

    pub fn clear(&mut self) {
        match self.tab {
            Tab::Chat => {
                self.history.clear();
                self.pending = None;
                self.chat_scroll = 0;
                self.chat_follow = true;
                self.status = "чат очищен".into();
            }
            Tab::Api => {
                self.exchanges.clear();
                self.api_selection = 0;
                self.api_scroll = 0;
                self.status = "API-журнал очищен".into();
            }
        }
    }
}

pub fn run(
    settings: Settings,
    catalog: ToolCatalog,
    warning: Option<String>,
) -> Result<(), String> {
    let api_key = load_api_key(None);
    let mut app = App::new(settings, catalog);
    if let Some(warning) = warning {
        app.banner = Some((warning, true));
    }
    if api_key.is_none() {
        app.banner = Some((
            "API-ключ не задан: экспортируйте DEEPSEEK_API_KEY".into(),
            true,
        ));
    }

    let (tx, rx) = mpsc::channel();
    enable_raw_mode().map_err(|error| error.to_string())?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen).map_err(|error| error.to_string())?;
    let backend = CrosstermBackend::new(out);
    let mut terminal = Terminal::new(backend).map_err(|error| error.to_string())?;

    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(stdout(), LeaveAlternateScreen);
        previous_hook(info);
    }));

    let result = event_loop(&mut terminal, &mut app, &tx, &rx, api_key);

    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let _ = terminal.show_cursor();
    let _ = std::panic::take_hook();
    result
}

fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    app: &mut App,
    tx: &Sender<AppEvent>,
    rx: &Receiver<AppEvent>,
    api_key: Option<String>,
) -> Result<(), String> {
    loop {
        terminal
            .draw(|frame| draw(frame, app))
            .map_err(|error| error.to_string())?;

        if event::poll(Duration::from_millis(50)).map_err(|error| error.to_string())? {
            match event::read().map_err(|error| error.to_string())? {
                Event::Key(key) if key.kind == KeyEventKind::Press => match handle_key(app, key) {
                    Action::Quit => {
                        app.should_quit = true;
                    }
                    Action::Send(text) => {
                        if let Some(key) = &api_key {
                            let history = app.history.clone();
                            let settings = app.settings.clone();
                            let catalog = app.catalog.clone();
                            let key = key.clone();
                            let tx = tx.clone();
                            app.begin_chat(text.clone());
                            std::thread::spawn(move || {
                                run_chat(settings, catalog, key, history, text, tx);
                            });
                        } else {
                            app.banner = Some((
                                "API-ключ не задан: экспортируйте DEEPSEEK_API_KEY".into(),
                                true,
                            ));
                        }
                    }
                    Action::None => {}
                },
                Event::Resize(_, _) => {}
                _ => {}
            }
        }

        while let Ok(event) = rx.try_recv() {
            match event {
                AppEvent::Started => {}
                AppEvent::Agent(AgentEvent::Exchange(exchange)) => {
                    app.push_exchange(exchange);
                    app.status = "ответ модели получен".into();
                }
                AppEvent::Agent(AgentEvent::ToolCall {
                    name,
                    source,
                    arguments,
                }) => {
                    app.status = format!(
                        "вызов {} [{}] · {}",
                        name,
                        source.as_str(),
                        truncate(&arguments, 90)
                    );
                }
                AppEvent::Agent(AgentEvent::ToolResult {
                    name,
                    source,
                    ok,
                    preview,
                }) => {
                    app.status = format!(
                        "{} {} [{}] · {}",
                        if ok {
                            "результат"
                        } else {
                            "ошибка"
                        },
                        name,
                        source.as_str(),
                        truncate(&preview, 110)
                    );
                }
                AppEvent::Finished(Ok(result)) => app.finish_chat(result),
                AppEvent::Finished(Err(error)) => app.fail_chat(error),
            }
        }

        if app.should_quit {
            return Ok(());
        }
    }
}

fn run_chat(
    settings: Settings,
    catalog: ToolCatalog,
    api_key: String,
    history: Vec<ChatMessage>,
    text: String,
    tx: Sender<AppEvent>,
) {
    let _ = tx.send(AppEvent::Started);
    let client = match ReqwestClient::new(&settings.base_url, Some(api_key)) {
        Ok(client) => client,
        Err(error) => {
            let _ = tx.send(AppEvent::Finished(Err(error.to_string())));
            return;
        }
    };
    let agent = Agent {
        settings: &settings,
        client: &client,
        catalog: &catalog,
    };
    let result = agent
        .run(&history, &text, &mut |event| {
            let _ = tx.send(AppEvent::Agent(event));
        })
        .map_err(|error| error.to_string());
    let _ = tx.send(AppEvent::Finished(result));
}

fn handle_key(app: &mut App, key: KeyEvent) -> Action {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return Action::Quit;
    }
    if app.chat_focused {
        match key.code {
            KeyCode::Esc => app.chat_focused = false,
            KeyCode::Enter => {
                let text = app.chat_input.trim().to_string();
                if text.is_empty() || app.busy {
                    return Action::None;
                }
                return Action::Send(text);
            }
            KeyCode::Backspace => {
                app.chat_input.pop();
            }
            KeyCode::Char(ch) if app.chat_input.chars().count() < 2000 => {
                app.chat_input.push(ch);
            }
            _ => {}
        }
        return Action::None;
    }

    match key.code {
        KeyCode::Char('q') => return Action::Quit,
        KeyCode::Char('1') => app.tab = Tab::Chat,
        KeyCode::Char('2') => app.tab = Tab::Api,
        _ => match app.tab {
            Tab::Chat => match key.code {
                KeyCode::Tab => app.tab = Tab::Api,
                KeyCode::Char('i') | KeyCode::Enter => app.chat_focused = true,
                KeyCode::Char('c') => app.clear(),
                KeyCode::Up => {
                    app.chat_follow = false;
                    app.chat_scroll = app.chat_scroll.saturating_sub(1);
                }
                KeyCode::Down => {
                    app.chat_scroll = app.chat_scroll.saturating_add(1);
                }
                KeyCode::PageUp => {
                    app.chat_follow = false;
                    app.chat_scroll = app.chat_scroll.saturating_sub(12);
                }
                KeyCode::PageDown => {
                    app.chat_scroll = app.chat_scroll.saturating_add(12);
                }
                KeyCode::Home => {
                    app.chat_follow = false;
                    app.chat_scroll = 0;
                }
                KeyCode::End => {
                    app.chat_follow = true;
                    app.chat_scroll = u16::MAX;
                }
                _ => {}
            },
            Tab::Api => match key.code {
                KeyCode::Up if app.api_focus == ApiFocus::Body => {
                    app.api_scroll = app.api_scroll.saturating_sub(1);
                }
                KeyCode::Down if app.api_focus == ApiFocus::Body => {
                    app.api_scroll = app.api_scroll.saturating_add(1);
                }
                KeyCode::Up => {
                    app.api_selection = app.api_selection.saturating_sub(1);
                    app.api_scroll = 0;
                }
                KeyCode::Down => {
                    if app.api_selection + 1 < app.exchanges.len() {
                        app.api_selection += 1;
                    }
                    app.api_scroll = 0;
                }
                KeyCode::Tab | KeyCode::Enter => {
                    app.api_focus = match app.api_focus {
                        ApiFocus::List => ApiFocus::Body,
                        ApiFocus::Body => ApiFocus::List,
                    };
                }
                KeyCode::Esc => app.api_focus = ApiFocus::List,
                KeyCode::Left | KeyCode::Right => {
                    app.api_side = app.api_side.other();
                    app.api_scroll = 0;
                }
                KeyCode::PageUp => {
                    app.api_scroll = app.api_scroll.saturating_sub(15);
                }
                KeyCode::PageDown => {
                    app.api_scroll = app.api_scroll.saturating_add(15);
                }
                KeyCode::Home => app.api_scroll = 0,
                KeyCode::End => app.api_scroll = u16::MAX,
                KeyCode::Char('c') => app.clear(),
                _ => {}
            },
        },
    }
    Action::None
}

fn draw(frame: &mut ratatui::Frame, app: &mut App) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(8),
            Constraint::Length(3),
            Constraint::Length(2),
        ])
        .split(area);

    draw_header(frame, chunks[0], app);
    draw_tabs(frame, chunks[1], app);
    match app.tab {
        Tab::Chat => draw_chat(frame, chunks[2], app),
        Tab::Api => draw_api(frame, chunks[2], app),
    }
    draw_input(frame, chunks[3], app);
    draw_footer(frame, chunks[4], app);
}

fn draw_header(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let mcp = match app.catalog.mcp() {
        Some(client) => format!(
            "MCP {} · {} · {} инстр.",
            client.config().label,
            client.server().name,
            app.catalog.mcp_count()
        ),
        None => "MCP выключен".into(),
    };
    let busy = if app.busy { " · [busy]" } else { "" };
    let text = format!(
        " lab16 · {} · {} · {}{}",
        app.settings.model, mcp, app.status, busy
    );
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(text, accent()))),
        area,
    );
}

fn draw_tabs(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let mut spans = Vec::new();
    for tab in Tab::ALL {
        let style = if tab == app.tab {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        spans.push(Span::styled(
            format!(" {} {} ", tab.number(), tab.title()),
            style,
        ));
        spans.push(Span::raw(" "));
    }
    spans.push(Span::styled(
        "  · только две вкладки",
        Style::default().fg(Color::DarkGray),
    ));
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_chat(frame: &mut ratatui::Frame, area: Rect, app: &mut App) {
    let mut lines: Vec<Line<'static>> = Vec::new();
    for message in &app.history {
        match message.role {
            Role::System => {}
            Role::User => {
                lines.push(Line::from(Span::styled(
                    "Вы",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )));
                push_text(&mut lines, message.text(), 0);
                lines.push(Line::from(""));
            }
            Role::Assistant => {
                lines.push(Line::from(Span::styled(
                    "Ассистент",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                )));
                if !message.text().trim().is_empty() {
                    push_text(&mut lines, message.text(), 0);
                }
                for call in &message.tool_calls {
                    lines.push(Line::from(Span::styled(
                        format!("  → {}({})", call.name, truncate(&call.arguments, 160)),
                        Style::default().fg(Color::Yellow),
                    )));
                }
                lines.push(Line::from(""));
            }
            Role::Tool => {
                let name = message.name.as_deref().unwrap_or("tool");
                lines.push(Line::from(Span::styled(
                    format!("← {name} · результат"),
                    Style::default().fg(Color::DarkGray),
                )));
                let text = message.text();
                let preview: String = text.chars().take(900).collect();
                for line in preview.lines().take(24) {
                    lines.push(Line::from(Span::styled(
                        format!("  {line}"),
                        Style::default().fg(Color::DarkGray),
                    )));
                }
                if text.chars().count() > 900 {
                    lines.push(Line::from(Span::styled(
                        "  … (полный ответ инструмента в контексте модели)",
                        Style::default().fg(Color::DarkGray),
                    )));
                }
                lines.push(Line::from(""));
            }
        }
    }
    if app.busy {
        lines.push(Line::from(Span::styled(
            "Ассистент · …",
            Style::default().fg(Color::Green),
        )));
        lines.push(Line::from(""));
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "Начните диалог: i или Enter — ввод, Enter — отправить.",
            Style::default().fg(Color::Gray),
        )));
        lines.push(Line::from(Span::styled(
            "Модель получает схемы локальных инструментов и GitHub MCP.",
            Style::default().fg(Color::Gray),
        )));
    }

    let block = Block::default().borders(Borders::ALL).title(" Диалог ");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    let total = paragraph.line_count(inner.width);
    let max_scroll = total.saturating_sub(inner.height as usize);
    if app.chat_follow {
        app.chat_scroll = max_scroll.min(u16::MAX as usize) as u16;
    } else {
        app.chat_scroll = (app.chat_scroll as usize).min(max_scroll) as u16;
    }
    frame.render_widget(paragraph.scroll((app.chat_scroll, 0)), inner);
}

fn draw_input(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let title = if app.chat_focused {
        " Сообщение · Enter отправить · Esc выйти "
    } else if app.busy {
        " Ассистент отвечает… "
    } else {
        " Сообщение · i или Enter "
    };
    let line = if app.chat_focused {
        Line::from(vec![
            Span::styled("> ", accent()),
            Span::raw(app.chat_input.clone()),
            Span::styled("▏", Style::default().bg(Color::Cyan)),
        ])
    } else {
        Line::from(Span::styled(
            "> нажмите i или Enter, чтобы написать модели",
            Style::default().fg(Color::DarkGray),
        ))
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(if app.chat_focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default()
        });
    frame.render_widget(Paragraph::new(line).block(block), area);
    if app.chat_focused {
        let width = area.width.saturating_sub(2);
        let cursor = 2 + app.chat_input.chars().count() as u16;
        frame.set_cursor_position(Position {
            x: area.x + cursor.min(width.saturating_sub(1)),
            y: area.y + 1,
        });
    }
}

fn draw_api(frame: &mut ratatui::Frame, area: Rect, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(38), Constraint::Percentage(62)])
        .split(area);
    draw_api_list(frame, chunks[0], app);
    draw_api_details(frame, chunks[1], app);
}

fn draw_api_list(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let mut lines = vec![Line::from(Span::styled(
        "#  время  итер  статус  мс".to_string(),
        Style::default().fg(Color::DarkGray),
    ))];
    if app.exchanges.is_empty() {
        lines.push(Line::from(Span::styled(
            "Запросов ещё нет",
            Style::default().fg(Color::Gray),
        )));
    }
    for (index, exchange) in app.exchanges.iter().enumerate() {
        let marker = if index == app.api_selection { ">" } else { " " };
        let style = if index == app.api_selection {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        lines.push(Line::from(Span::styled(
            format!(
                "{marker}{}  {}  {:>3}   {:<7} {:>5}",
                index + 1,
                exchange.started_at,
                exchange.iteration,
                exchange.status(),
                exchange.duration_ms
            ),
            style,
        )));
    }
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Запросы к модели ")
        .border_style(if app.api_focus == ApiFocus::List {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default()
        });
    let inner_height = area.height.saturating_sub(2) as usize;
    let total = lines.len();
    let selected_line = app.api_selection + 1;
    let scroll = if total > inner_height {
        (selected_line + 1)
            .saturating_sub(inner_height)
            .min(total.saturating_sub(inner_height))
    } else {
        0
    };
    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .scroll((scroll as u16, 0)),
        area,
    );
}

fn draw_api_details(frame: &mut ratatui::Frame, area: Rect, app: &mut App) {
    let Some(exchange) = app.selected_exchange().cloned() else {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Тело запроса ");
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "Сделайте запрос в чате — здесь появится точное тело запроса к модели.",
                Style::default().fg(Color::Gray),
            )))
            .block(block)
            .wrap(Wrap { trim: false }),
            area,
        );
        return;
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(6),
            Constraint::Length(1),
            Constraint::Min(4),
        ])
        .split(area);

    let usage = &exchange.usage;
    let summary = vec![
        Line::from(vec![
            Span::styled(
                format!(
                    "#{} · HTTP-запрос {}",
                    exchange.iteration, exchange.iteration
                ),
                accent(),
            ),
            Span::raw(format!(
                " · {} · {} мс · {}",
                exchange.started_at,
                exchange.duration_ms,
                exchange.status()
            )),
        ]),
        Line::from(format!(
            "модель: {} · инструментов в запросе: {}",
            app.settings.model,
            exchange.tool_names().len()
        )),
        Line::from(format!(
            "tokens: prompt {} · completion {} · total {}",
            usage.prompt_tokens, usage.completion_tokens, usage.total_tokens
        )),
        Line::from(format!(
            "request: {} символов · response: {} символов",
            exchange.request.to_string().chars().count(),
            exchange
                .response
                .as_ref()
                .map(|value| value.to_string().chars().count())
                .unwrap_or(0)
        )),
        Line::from(Span::styled(
            if let Some(error) = &exchange.error {
                format!("error: {error}")
            } else {
                "→ Enter/Tab: перейти к телу · ←→: запрос/ответ".into()
            },
            Style::default().fg(if exchange.error.is_some() {
                Color::Red
            } else {
                Color::DarkGray
            }),
        )),
    ];
    frame.render_widget(
        Paragraph::new(summary).block(Block::default().borders(Borders::ALL).title(" Сводка ")),
        chunks[0],
    );

    let mut tabs = Vec::new();
    for side in [ApiSide::Request, ApiSide::Response] {
        let style = if side == app.api_side {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        tabs.push(Span::styled(format!(" {} ", side.title()), style));
        tabs.push(Span::raw(" "));
    }
    tabs.push(Span::styled(
        "Tab/Enter — фокус · ←→ — сторона",
        Style::default().fg(Color::DarkGray),
    ));
    frame.render_widget(Paragraph::new(Line::from(tabs)), chunks[1]);

    let body_value = match app.api_side {
        ApiSide::Request => Some(exchange.request.clone()),
        ApiSide::Response => exchange.response.clone(),
    };
    let body = match body_value {
        Some(value) => serde_json::to_string_pretty(&value)
            .unwrap_or_else(|error| format!("не удалось сериализовать JSON: {error}")),
        None => exchange
            .error
            .clone()
            .unwrap_or_else(|| "Тело ответа не сохранено.".into()),
    };
    let lines: Vec<Line<'static>> = body
        .lines()
        .map(|line| Line::from(line.to_string()))
        .collect();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(match app.api_side {
            ApiSide::Request => " Тело запроса к модели ",
            ApiSide::Response => " Тело ответа ",
        })
        .border_style(if app.api_focus == ApiFocus::Body {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default()
        });
    let inner_height = chunks[2].height.saturating_sub(2) as usize;
    let paragraph = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(block);
    let total = paragraph.line_count(chunks[2].width.saturating_sub(2));
    let max_scroll = total.saturating_sub(inner_height);
    let scroll = if app.api_scroll == u16::MAX {
        max_scroll
    } else {
        (app.api_scroll as usize).min(max_scroll)
    };
    app.api_scroll = scroll.min(u16::MAX as usize) as u16;
    frame.render_widget(paragraph.scroll((app.api_scroll, 0)), chunks[2]);
}

fn draw_footer(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let hint = match app.tab {
        Tab::Chat if app.chat_focused => "Enter отправить · Esc выйти из ввода · Backspace удалить",
        Tab::Chat => "i/Enter ввод · ↑↓/PgUp/PgDn прокрутка · End низ · c очистить · Tab API",
        Tab::Api if app.api_focus == ApiFocus::Body => {
            "↑↓/PgUp/PgDn тело · Home/End · ←→ запрос/ответ · Tab/Esc к списку"
        }
        Tab::Api => "↑↓ выбрать · Tab/Enter к телу · ←→ сторона · c очистить · 1 чат",
    };
    let first = match &app.banner {
        Some((message, true)) => Line::from(Span::styled(
            format!(" {message}"),
            Style::default().fg(Color::Red),
        )),
        Some((message, false)) => Line::from(Span::styled(
            format!(" {message}"),
            Style::default().fg(Color::Green),
        )),
        None => Line::from(Span::styled(
            format!(" {hint}"),
            Style::default().fg(Color::DarkGray),
        )),
    };
    let second = if app.busy {
        " q/Ctrl+C выход · идёт запрос к модели"
    } else {
        " q выход · 1 Чат · 2 API"
    };
    frame.render_widget(
        Paragraph::new(vec![
            first,
            Line::from(Span::styled(second, Style::default().fg(Color::DarkGray))),
        ]),
        area,
    );
}

fn push_text(lines: &mut Vec<Line<'static>>, text: &str, indent: usize) {
    let prefix = " ".repeat(indent);
    for line in text.lines() {
        lines.push(Line::from(format!("{prefix}{line}")));
    }
}

fn truncate(text: &str, limit: usize) -> String {
    let mut result: String = text.chars().take(limit).collect();
    if text.chars().count() > limit {
        result.push('…');
    }
    result
}

fn accent() -> Style {
    Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD)
}

pub fn render_for_test(frame: &mut ratatui::Frame, app: &mut App) {
    draw(frame, app);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn app() -> App {
        App::new(Settings::default(), ToolCatalog::local_only())
    }

    #[test]
    fn only_two_tabs_exist() {
        assert_eq!(Tab::ALL.len(), 2);
        assert_eq!(Tab::Chat.number(), 1);
        assert_eq!(Tab::Api.number(), 2);
    }

    #[test]
    fn tab_switch_and_input_are_pure_state() {
        let mut app = app();
        let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
        assert_eq!(handle_key(&mut app, key(KeyCode::Tab)), Action::None);
        assert_eq!(app.tab, Tab::Api);
        assert_eq!(handle_key(&mut app, key(KeyCode::Char('1'))), Action::None);
        assert_eq!(app.tab, Tab::Chat);
        assert_eq!(handle_key(&mut app, key(KeyCode::Char('i'))), Action::None);
        assert!(app.chat_focused);
        for ch in "2+2".chars() {
            assert_eq!(handle_key(&mut app, key(KeyCode::Char(ch))), Action::None);
        }
        assert_eq!(
            handle_key(&mut app, key(KeyCode::Enter)),
            Action::Send("2+2".into())
        );
    }

    #[test]
    fn api_tab_uses_tab_for_body_focus() {
        let mut app = app();
        app.tab = Tab::Api;
        let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
        assert_eq!(handle_key(&mut app, key(KeyCode::Tab)), Action::None);
        assert_eq!(app.api_focus, ApiFocus::Body);
        assert_eq!(handle_key(&mut app, key(KeyCode::Tab)), Action::None);
        assert_eq!(app.api_focus, ApiFocus::List);
        assert_eq!(handle_key(&mut app, key(KeyCode::Char('1'))), Action::None);
        assert_eq!(app.tab, Tab::Chat);
    }

    #[test]
    fn finished_turn_replaces_history() {
        let mut app = app();
        app.begin_chat("привет".into());
        assert_eq!(app.history.len(), 1);
        app.finish_chat(TurnResult {
            final_text: "ответ".into(),
            history: vec![ChatMessage::user("привет"), ChatMessage::assistant("ответ")],
            exchanges: Vec::new(),
            usage: Default::default(),
            steps: 1,
        });
        assert_eq!(app.history.len(), 2);
        assert!(!app.busy);
        assert!(app.pending.is_none());
    }

    #[test]
    fn render_shows_exactly_two_tabs() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = app();
        terminal
            .draw(|frame| render_for_test(frame, &mut app))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let text: String = buffer
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<Vec<_>>()
            .join("");
        assert!(text.contains("1 Чат"));
        assert!(text.contains("2 API"));
        assert!(!text.contains("Память"));
        assert!(!text.contains("Настройки"));
        assert!(!text.contains("3 "));
    }

    #[test]
    fn api_tab_renders_request_body() {
        let backend = TestBackend::new(140, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = app();
        app.exchanges.push(Exchange {
            id: "1".into(),
            iteration: 1,
            started_at: "12:00:00".into(),
            duration_ms: 10,
            request: serde_json::json!({
                "model": "deepseek-chat",
                "messages": [{"role": "user", "content": "привет"}],
                "tools": [{"type": "function", "function": {"name": "calculate"}}]
            }),
            response: None,
            error: None,
            usage: Default::default(),
        });
        app.tab = Tab::Api;
        terminal
            .draw(|frame| render_for_test(frame, &mut app))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let text: String = buffer
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<Vec<_>>()
            .join("");
        assert!(text.contains("Тело запроса"));
        assert!(text.contains("deepseek-chat"));
        assert!(text.contains("calculate"));
    }
}
