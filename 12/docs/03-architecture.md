# 03. Архитектура и контракты

## 1. Принципы

1. **Слой — модуль.** STM, WM, LTM не импортируют друг друга; их соединяет
   `AppController` и сборщик промпта.
2. **Хранение снаружи логики.** Доменные функции работают с чистыми структурами;
   файлы — в `storage`, TUI/CLI — тонкие адаптеры.
3. **Один вход записи.** Router, tool `memory_write` или явная команда. Прямых
   `push` в векторы из TUI нет.
4. **Контракт раньше кода.** Типы и сигнатуры фиксируются в T1.
5. **Один контроллер — два фронта.** TUI и CLI вызывают одни и те же методы.
6. **Блокирующий HTTP в фоне.** TUI не должен зависать на запросе.

## 2. Cargo-проект

```text
11/
  Cargo.toml
  Cargo.lock
  rust-toolchain.toml          # stable, edition 2024
  .gitignore                   # target/, data/, *.log
  fixtures/
    day10-facts.json           # sample facts Дня 10
  src/
    main.rs                    # clap: tui | chat | memory | import-facts | experiment
    cli/
      mod.rs
      commands.rs
      output.rs                # таблицы/JSON для CLI
    app/
      mod.rs
      controller.rs            # AppController / AppRuntime
      state.rs                 # AppState для TUI
      events.rs                # Job, AppEvent, UiCommand
      jobs.rs                  # worker thread, mpsc
    domain/
      mod.rs
      memory.rs                # типы из 02
      settings.rs              # Settings, MemoryConfig, defaults
      chat.rs                  # ChatMessage, TurnOutcome, TokenStats
    storage/
      mod.rs
      atomic.rs                # tmp+fsync+rename, .bak, permissions
      stm.rs
      working.rs
      ltm.rs
      pending.rs
      meta.rs
      settings.rs
      lock.rs                  # fs2
      paths.rs                 # data-dir resolution
    memory/
      mod.rs
      candidates.rs            # локальное извлечение
      extractor.rs             # LLM-извлечение
      router.rs                # R1–R6, pending, audit
      retrieval.rs             # score + top-K
      prompt.rs                # assemble_prompt
      tokens.rs                # эвристика токенов
      migration.rs             # facts → LTM
      audit.rs                 # маскирование, события
    llm/
      mod.rs
      client.rs                # ChatClient trait + ReqwestClient
      dto.rs                   # OpenAI-compatible JSON
      tools.rs                 # calculator/time/currency/memory_write
      agent.rs                 # tool-loop
    experiment/
      mod.rs
      fixtures.rs              # 34 сообщения
      probes.rs                # 18 probe
      runner.rs                # 7 конфигураций + очистка
      report.rs                # Markdown/JSON
    tui/
      mod.rs
      app.rs                   # terminal setup, event loop
      theme.rs
      keymap.rs
      widgets/
        mod.rs
        memory.rs
        pending.rs
        experiment.rs
        settings.rs
        api_log.rs
        json.rs                # подсветка JSON
        help.rs
        modal.rs
        input.rs               # однострочный ввод
  tests/
    storage.rs
    router.rs
    prompt.rs
    migration.rs
    experiment.rs
    cli.rs
```

Зависимости (Cargo.toml):

```toml
[package]
name = "lab11"
version = "0.1.0"
edition = "2024"
rust-version = "1.85"

[dependencies]
ratatui = "0.29"
crossterm = "0.28"
clap = { version = "4", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
chrono = { version = "0.4", features = ["serde"] }
uuid = { version = "1", features = ["v4"] }
reqwest = { version = "0.12", default-features = false, features = ["blocking", "json", "rustls-tls"] }
thiserror = "2"
anyhow = "1"
directories = "5"
fs2 = "0.4"
unicode-width = "0.2"

[dev-dependencies]
tempfile = "3"
assert_cmd = "2"
predicates = "3"
```

## 3. Контракты модулей

### Storage

```rust
pub struct DataPaths { pub root: PathBuf, /* ... */ }

pub fn resolve_data_dir(cli: Option<PathBuf>) -> Result<DataPaths>;

pub fn load_stm(paths: &DataPaths) -> Result<StmState, LoadError>;
pub fn save_stm(paths: &DataPaths, state: &StmState) -> Result<(), SaveError>;

pub fn load_working(paths: &DataPaths) -> Result<WorkingMemoryState, LoadError>;
pub fn save_working(paths: &DataPaths, state: &WorkingMemoryState) -> Result<(), SaveError>;

pub fn load_ltm(paths: &DataPaths) -> Result<LtmState, LoadError>;
pub fn save_ltm(paths: &DataPaths, state: &LtmState) -> Result<(), SaveError>;

pub fn load_pending(paths: &DataPaths) -> Result<Vec<PendingWrite>, LoadError>;
pub fn save_pending(paths: &DataPaths, pending: &[PendingWrite]) -> Result<(), SaveError>;

pub fn load_meta(paths: &DataPaths) -> Result<MemoryMetaState, LoadError>;
pub fn save_meta(paths: &DataPaths, meta: &MemoryMetaState) -> Result<(), SaveError>;

pub fn load_bundle(paths: &DataPaths) -> Result<MemoryBundle, LoadError>;
pub fn acquire_lock(paths: &DataPaths) -> Result<LockGuard, LockError>;
```

`LoadError` различает `NotFound` (пустой слой — норма), `Corrupt { path }`,
`UnsupportedVersion { found }`, `Io`. `save_*` используют `atomic_write_json`:

```rust
pub fn atomic_write_json<T: Serialize + ?Sized>(path: &Path, value: &T) -> Result<(), SaveError>;
```

Порядок: создать каталог → записать `path.tmp` → `sync_all` → `rename` поверх
`path` (старый файл предварительно копируется в `path.bak`).

### Слои

```rust
pub fn select_stm_window(stm: &StmState, window: usize, max_tokens: usize) -> StmWindow;
pub fn stm_summary_tokens(stm: &StmState) -> usize;

pub fn empty_working(now: DateTime<Utc>) -> WorkingMemoryState;
pub fn upsert_wm_item(state: &mut WorkingMemoryState, input: WmUpsert, now: DateTime<Utc>) -> WmChange;
pub fn set_wm_status(state: &mut WorkingMemoryState, id: &str, status: WmStatus, now: DateTime<Utc>);
pub fn remove_wm_item(state: &mut WorkingMemoryState, id: &str);
pub fn archive_task(state: &mut WorkingMemoryState, now: DateTime<Utc>);
pub fn prune_wm(state: &mut WorkingMemoryState, config: &WmConfig, now: DateTime<Utc>);
pub fn render_wm(state: &WorkingMemoryState, max_tokens: usize, included_ltm: &[Id]) -> RenderedBlock;

pub fn active_ltm(state: &LtmState) -> impl Iterator<Item = &LtmRecord>;
pub fn find_active_ltm<'a>(state: &'a LtmState, key: &str) -> Option<&'a LtmRecord>;
pub fn upsert_ltm(state: &mut LtmState, input: UpsertLtm, now: DateTime<Utc>) -> LtmChange;
pub fn replace_ltm(state: &mut LtmState, input: UpsertLtm, now: DateTime<Utc>) -> LtmChange;
pub fn remove_ltm(state: &mut LtmState, id: &str);
pub fn render_ltm_profile(state: &LtmState, max_tokens: usize) -> RenderedBlock;
```

Семантика:

- `upsert_ltm`: create; при совпадении значения — `Same`; при другом —
  делегирует `replace_ltm`;
- `replace_ltm`: создаёт новую активную версию, старая получает
  `superseded_by`; прямое изменение `value` запрещено;
- `render_wm`: active + done-plan как `[x]`; порядок goal → plan → status →
  entity → open-question → note; `ref_ltm_id`, уже включённый в LTM, пропускается;
- `render_ltm_profile`: `profile/project/preference`, pinned выше.

### Router

```rust
pub struct RouteInput<'a> {
    pub message: &'a ChatMessage,
    pub message_index: usize,
    pub wm: &'a WorkingMemoryState,
    pub ltm: &'a LtmState,
    pub pending: &'a [PendingWrite],
    pub config: &'a MemoryConfig,
    pub now: DateTime<Utc>,
}

pub struct RouteOutcome {
    pub wm: WorkingMemoryState,
    pub ltm: LtmState,
    pub pending: Vec<PendingWrite>,
    pub audit: Vec<MemoryWriteEvent>,
    pub rejected: Vec<RejectedCandidate>,
    pub stats: RouteStats,
}

pub fn route_candidates(
    input: RouteInput<'_>,
    extractor: &dyn CandidateExtractor,
) -> Result<RouteOutcome, RouteError>;

pub trait CandidateExtractor: Send + Sync {
    fn extract(&self, message: &ChatMessage, ltm: &LtmState) -> Result<Extraction, ExtractError>;
}
```

`LocalExtractor` — 0 запросов; `LlmExtractor` — 1 запрос, при ошибке контроллер
откатывается на локальный и пишет `stats.error`.

### Retrieval и prompt

```rust
pub struct RetrievalInput<'a> {
    pub records: &'a [LtmRecord],
    pub query: &'a str,
    pub wm: &'a WorkingMemoryState,
    pub max_tokens: usize,
    pub top_k: usize,
}

pub fn select_relevant_ltm(input: RetrievalInput<'_>) -> RetrievalResult;

pub struct AssembleInput<'a> {
    pub system_prompt: &'a str,
    pub stm: &'a StmState,
    pub wm: &'a WorkingMemoryState,
    pub ltm: &'a LtmState,
    pub config: &'a MemoryConfig,
    pub tools: &'a [ToolSchema],
    pub layers: LayerFlags,          // для ablation
    pub now: DateTime<Utc>,
}

pub struct AssembleOutput {
    pub messages: Vec<ChatMessage>,
    pub breakdown: MemoryBreakdown,
    pub stm_window: StmWindow,
    pub included_ltm: Vec<LtmRecord>,
    pub dropped_ltm: Vec<LtmRecord>,
    pub notes: Vec<String>,
}

pub fn assemble_prompt(input: AssembleInput<'_>) -> Result<AssembleOutput, PromptError>;
```

Текущая user-реплика (последняя в `stm.messages`) добавляется всегда, вне окна;
при `layers.stm=false` в промпте остаются только системные блоки, tools и она.

### LLM и Agent

```rust
pub trait ChatClient: Send + Sync {
    fn complete(&self, request: &ChatRequest) -> Result<ChatResponse, LlmError>;
}

pub struct ReqwestClient { /* base_url, api_key, http */ }

pub struct TurnInput<'a> {
    pub user_text: &'a str,
    pub memory: MemoryBundle,
    pub settings: &'a Settings,
    pub tools: &'a ToolRegistry,
    pub client: &'a dyn ChatClient,
    pub cancel: &'a AtomicBool,
}

pub struct TurnOutcome {
    pub final_text: String,
    pub memory: MemoryBundle,
    pub messages: Vec<ChatMessage>,
    pub breakdown: MemoryBreakdown,
    pub token_stats: TokenStats,
    pub audit: Vec<MemoryWriteEvent>,
    pub route_stats: RouteStats,
}

pub fn run_turn(input: TurnInput<'_>, progress: &mut dyn FnMut(Progress)) -> Result<TurnOutcome, AgentError>;
```

`memory_write` перехватывается в tool-loop до `ToolRegistry`; запись действует
со следующего хода, промпт текущего прогона не пересобирается (ADR-6).

### Controller и jobs

```rust
pub struct AppRuntime {
    pub paths: DataPaths,
    pub settings: Settings,
    pub client: Arc<dyn ChatClient>,
}

pub struct AppController {
    runtime: AppRuntime,
    bundle: MemoryBundle,
}

impl AppController {
    pub fn load(runtime: AppRuntime) -> Result<Self, AppError>;
    pub fn bundle(&self) -> &MemoryBundle;
    pub fn send_user_message(&mut self, text: &str, progress: &mut dyn FnMut(Progress)) -> Result<TurnOutcome, AppError>;
    pub fn accept_pending(&mut self, id: &str) -> Result<(), AppError>;
    pub fn reject_pending(&mut self, id: &str) -> Result<(), AppError>;
    pub fn change_pending_layer(&mut self, id: &str, layer: MemoryLayer) -> Result<(), AppError>;
    pub fn clear_layer(&mut self, layer: MemoryLayer) -> Result<(), AppError>;
    pub fn import_facts(&mut self, path: &Path) -> Result<MigrationReport, AppError>;
    pub fn run_experiment(&mut self, opts: ExperimentOptions, progress: &mut dyn FnMut(ExperimentProgress)) -> Result<ExperimentReport, AppError>;
    pub fn save_report(&self, report: &ExperimentReport, out: &Path, json: &Path) -> Result<(), AppError>;
}
```

TUI job:

```rust
pub enum Job {
    Chat(String),
    AcceptPending(String),
    RejectPending(String),
    ChangePending { id: String, layer: MemoryLayer },
    ClearLayer(MemoryLayer),
    ImportFacts(PathBuf),
    Experiment(ExperimentOptions),
}

pub enum AppEvent {
    JobStarted(&'static str),
    MemoryUpdated(MemoryBundle),
    ChatFinished(TurnOutcome),
    ExperimentProgress(ExperimentProgress),
    ExperimentFinished(ExperimentReport),
    MigrationFinished(MigrationReport),
    Error(String),
    JobFinished,
}

pub fn spawn_job(runtime: AppRuntime, job: Job, tx: Sender<AppEvent>, cancel: Arc<AtomicBool>);
```

TUI не вызывает сеть в главном потоке: job выполняется в отдельном
`std::thread`, результат приходит `AppEvent`.

## 4. TUI runtime

```text
main thread
  ├─ crossterm event::poll(50ms) → KeyEvent
  ├─ rx.try_recv() → AppEvent
  ├─ update AppState
  └─ terminal.draw(...)

worker thread (one job at a time)
  ├─ AppController method
  ├─ progress callback → tx.send(...)
  └─ tx.send(AppEvent::...)
```

- `ratatui::Terminal<CrosstermBackend<Stdout>>`, alternate screen, raw mode.
- Panic hook и `Drop` восстанавливают терминал.
- При resize — пересчёт layout, без перезапуска.
- Пока job активен, destructive-действия блокируются, в футере `[busy]`.
- `q`/`Ctrl+C` отменяет job через `AtomicBool` и выходит после join.
- Логи не идут в stdout: только панель «API» и файл (если включено).

Клавиши и экраны — `06-ui-spec.md`.

## 5. CLI

```text
lab11 [--data-dir PATH] [--config PATH] <command>

commands:
  tui
  chat <TEXT> [--json] [--no-stream]
  memory show [--layer stm|wm|ltm] [--json]
  memory add --layer <...> --key ... --value ... [--kind ...]
  memory update --layer <...> --key ... --value ...
  memory delete --layer <...> --key ...
  memory move --from wm|ltm --key ... --to ltm|wm
  memory pin --key ... [--off]
  memory pending list|accept <ID>|reject <ID>
  memory clear --layer <...> [--yes]
  import-facts <PATH>
  experiment [--quick|--full] [--out PATH] [--json PATH] [--check]
  dump [--json]
```

Глобальные флаги: `--data-dir`, `--config`, `--verbose`.
API-ключ: `DEEPSEEK_API_KEY`, `KIMI_API_KEY`, `--api-key-file`.
CLI возвращает коды: `0` успех, `2` ошибка ввода, `3` ошибка API,
`4` провал порогов `--check`.

## 6. Поток данных

```text
TUI/CLI
  │ user text
  ▼
AppController.send_user_message
  ├─ STM: append user message
  ├─ router: candidates → WM / LTM pending / reject
  ├─ retrieval + assemble_prompt
  ├─ LLM tool-loop (calculator/time/currency/memory_write)
  └─ save STM/WM/LTM/pending/meta отдельными файлами
        │
        ▼
AppEvent / CLI output: final text, tokens, breakdown, audit
```

## 7. Обработка ошибок

| Ситуация | Поведение |
|---|---|
| LLM вернул мусор при извлечении | откат на Local, `stats.error`, запись в API-лог |
| JSON слоя повреждён | `LoadError::Corrupt`; слой пустой, баннер, файл не перезаписывается |
| Нет прав на запись | `SaveError::Io`; TUI-баннер, CLI код 2 |
| Нет API-ключа | TUI: модалка; CLI: код 2, без сетевого запроса |
| Lock занят | вторая копия завершается с подсказкой |
| Эксперимент отменён | частичный отчёт не сохраняется |
| Паника в worker | `AppEvent::Error`, TUI жив; паника логируется в файл |

## 8. Настройки

`settings.json` — `Settings` без ключа. При первом запуске создаются defaults.
Опционально `import-settings <path>` (P2) читает экспорт Дня 10 и маппит
`factsMode → extraction_mode`, `keepRecentMessages → stm.window`.

Ветки удалены из проекта: отдельного файла снимков и переключения слоёв нет.
