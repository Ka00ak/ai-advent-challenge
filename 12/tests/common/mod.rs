#![allow(dead_code)]

use std::path::Path;
use std::sync::Arc;

use lab12::app::controller::{AppController, AppRuntime};
use lab12::domain::memory::ChatMessage;
use lab12::domain::settings::Settings;
use lab12::experiment::fixtures::seeded_bundle;
use lab12::llm::client::FakeChatClient;
use lab12::storage::paths::DataPaths;

pub fn test_settings() -> Settings {
    let mut settings = Settings::default();
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
