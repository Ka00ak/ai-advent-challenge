#![allow(dead_code)]

use std::path::Path;
use std::sync::Arc;

use lab15::app::controller::{AppController, AppRuntime};
use lab15::domain::memory::ChatMessage;
use lab15::domain::settings::Settings;
use lab15::experiment::fixtures::seeded_bundle;
use lab15::llm::client::FakeChatClient;
use lab15::storage::paths::DataPaths;

pub fn test_settings() -> Settings {
    let mut settings = Settings::default();
    // Интеграционные тесты старых сценариев проверяют pending; приложение
    // по умолчанию использует auto (День 15), тесты включают confirm явно.
    settings.memory.approval = lab15::domain::settings::Approval::Confirm;
    settings.memory.stm.summary_enabled = false;
    settings.temperature = 0.0;
    settings
}

pub fn runtime_at(root: &Path) -> AppRuntime {
    let paths = DataPaths::new(root);
    let _ = paths.ensure();
    AppRuntime::new(paths, test_settings(), Arc::new(FakeChatClient))
}

pub fn controller_at(root: &Path) -> AppController {
    AppController::load(runtime_at(root)).unwrap()
}

pub fn seeded_controller(root: &Path) -> AppController {
    let mut controller = controller_at(root);
    controller
        .replace_bundle(seeded_bundle(chrono::Utc::now()))
        .unwrap();
    controller
}

pub fn user(text: &str) -> ChatMessage {
    ChatMessage::user(text)
}
