//! Файловое хранилище data-dir.
//!
//! Каждый слой пишет только в свои файлы. Битый JSON не перезаписывается:
//! вызывающий код получает `LoadError::Corrupt` и предлагает reset слоя.

pub mod atomic;
pub mod lock;
pub mod paths;

use std::path::Path;

use chrono::Utc;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::domain::memory::{
    LtmState, MemoryBundle, MemoryMetaState, PendingWrite, STATE_VERSION, StmState,
    WorkingMemoryState,
};
use crate::domain::profile::ProfileState;
use crate::domain::settings::Settings;
use crate::storage::atomic::{SaveError, atomic_write_json, set_private_permissions};
use crate::storage::paths::DataPaths;

pub use lock::{LockError, LockGuard, acquire_lock};
pub use paths::resolve_data_dir;

#[derive(Debug, Error)]
pub enum LoadError {
    #[error("файл не найден: {path}")]
    NotFound { path: std::path::PathBuf },
    #[error("файл повреждён: {path}: {message}")]
    Corrupt {
        path: std::path::PathBuf,
        message: String,
    },
    #[error("неподдерживаемая версия {found} в {path} (ожидается {expected})")]
    UnsupportedVersion {
        path: std::path::PathBuf,
        found: u64,
        expected: u32,
    },
    #[error("ошибка чтения {path}: {source}")]
    Io {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
}

impl LoadError {
    pub fn is_not_found(&self) -> bool {
        matches!(self, LoadError::NotFound { .. })
    }
}

fn read_json<T: DeserializeOwned>(path: &Path, check_version: bool) -> Result<T, LoadError> {
    if !path.exists() {
        return Err(LoadError::NotFound {
            path: path.to_path_buf(),
        });
    }
    let data = std::fs::read_to_string(path).map_err(|source| LoadError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let value: serde_json::Value =
        serde_json::from_str(&data).map_err(|error| LoadError::Corrupt {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;
    if check_version {
        if let Some(version) = value.get("version").and_then(|v| v.as_u64()) {
            if version != u64::from(STATE_VERSION) {
                return Err(LoadError::UnsupportedVersion {
                    path: path.to_path_buf(),
                    found: version,
                    expected: STATE_VERSION,
                });
            }
        }
    }
    serde_json::from_value(value).map_err(|error| LoadError::Corrupt {
        path: path.to_path_buf(),
        message: error.to_string(),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StmSummaryFile {
    summary: Option<String>,
    covered_messages: usize,
}

pub fn load_stm(paths: &DataPaths) -> Result<StmState, LoadError> {
    let mut state: StmState = read_json(&paths.stm_conversation(), true)?;
    match read_json::<StmSummaryFile>(&paths.stm_summary(), false) {
        Ok(summary) => {
            state.summary = summary.summary;
            state.covered_messages = summary.covered_messages;
        }
        Err(error) if error.is_not_found() => {}
        Err(error) => return Err(error),
    }
    Ok(state)
}

pub fn save_stm(paths: &DataPaths, state: &StmState) -> Result<(), SaveError> {
    let mut conversation = state.clone();
    conversation.summary = None;
    atomic_write_json(&paths.stm_conversation(), &conversation)?;
    atomic_write_json(
        &paths.stm_summary(),
        &StmSummaryFile {
            summary: state.summary.clone(),
            covered_messages: state.covered_messages,
        },
    )?;
    Ok(())
}

pub fn load_working(paths: &DataPaths) -> Result<WorkingMemoryState, LoadError> {
    read_json(&paths.working(), true)
}

pub fn save_working(paths: &DataPaths, state: &WorkingMemoryState) -> Result<(), SaveError> {
    atomic_write_json(&paths.working(), state)
}

pub fn load_ltm(paths: &DataPaths) -> Result<LtmState, LoadError> {
    read_json(&paths.ltm(), true)
}

pub fn save_ltm(paths: &DataPaths, state: &LtmState) -> Result<(), SaveError> {
    atomic_write_json(&paths.ltm(), state)
}

pub fn load_pending(paths: &DataPaths) -> Result<Vec<PendingWrite>, LoadError> {
    read_json(&paths.pending(), false)
}

pub fn save_pending(paths: &DataPaths, pending: &[PendingWrite]) -> Result<(), SaveError> {
    atomic_write_json(&paths.pending(), pending)
}

pub fn load_meta(paths: &DataPaths) -> Result<MemoryMetaState, LoadError> {
    read_json(&paths.meta(), true)
}

pub fn save_meta(paths: &DataPaths, meta: &MemoryMetaState) -> Result<(), SaveError> {
    atomic_write_json(&paths.meta(), meta)
}

pub fn load_settings(paths: &DataPaths) -> Result<Settings, LoadError> {
    read_json(&paths.settings(), true)
}

pub fn save_settings(paths: &DataPaths, settings: &Settings) -> Result<(), SaveError> {
    let path = paths.settings();
    atomic_write_json(&path, settings)?;
    set_private_permissions(&path)
}

pub fn load_profiles(paths: &DataPaths) -> Result<ProfileState, LoadError> {
    read_json(&paths.profiles(), true)
}

pub fn save_profiles(paths: &DataPaths, state: &ProfileState) -> Result<(), SaveError> {
    atomic_write_json(&paths.profiles(), state)
}

pub fn load_branch_file(paths: &DataPaths) -> Result<branches::BranchFile, LoadError> {
    read_json(&paths.branches(), true)
}

pub fn save_branch_file(paths: &DataPaths, file: &branches::BranchFile) -> Result<(), SaveError> {
    atomic_write_json(&paths.branches(), file)
}

/// Загружает все слои. Отсутствующие файлы — пустое состояние; битые — ошибка.
pub fn load_bundle(paths: &DataPaths) -> Result<MemoryBundle, LoadError> {
    paths.ensure().map_err(|error| LoadError::Io {
        path: paths.root.clone(),
        source: std::io::Error::other(error.to_string()),
    })?;
    let now = Utc::now();
    let stm = load_stm(paths).or_else(|e| {
        if e.is_not_found() {
            Ok(StmState::empty(now))
        } else {
            Err(e)
        }
    })?;
    let wm = load_working(paths).or_else(|e| {
        if e.is_not_found() {
            Ok(WorkingMemoryState::empty(now))
        } else {
            Err(e)
        }
    })?;
    let ltm = load_ltm(paths).or_else(|e| {
        if e.is_not_found() {
            Ok(LtmState::empty(now))
        } else {
            Err(e)
        }
    })?;
    let pending = load_pending(paths).or_else(|e| {
        if e.is_not_found() {
            Ok(Vec::new())
        } else {
            Err(e)
        }
    })?;
    let meta = load_meta(paths).or_else(|e| {
        if e.is_not_found() {
            Ok(MemoryMetaState::empty())
        } else {
            Err(e)
        }
    })?;
    Ok(MemoryBundle {
        stm,
        wm,
        ltm,
        pending,
        meta,
    })
}

/// Сохраняет все слои отдельными файлами (без транзакции между ними).
pub fn save_bundle(paths: &DataPaths, bundle: &MemoryBundle) -> Result<(), SaveError> {
    save_stm(paths, &bundle.stm)?;
    save_working(paths, &bundle.wm)?;
    save_ltm(paths, &bundle.ltm)?;
    save_pending(paths, &bundle.pending)?;
    save_meta(paths, &bundle.meta)?;
    Ok(())
}

pub mod branches {
    use chrono::{DateTime, Utc};
    use serde::{Deserialize, Serialize};

    use crate::domain::memory::{
        LtmState, MemoryMetaState, PendingWrite, STATE_VERSION, StmState, WorkingMemoryState,
    };

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct BranchSnapshot {
        pub id: String,
        pub label: String,
        pub created_at: DateTime<Utc>,
        pub stm: StmState,
        pub wm: WorkingMemoryState,
        pub ltm: LtmState,
        pub pending: Vec<PendingWrite>,
        pub meta: MemoryMetaState,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct BranchFile {
        pub version: u32,
        pub active_branch: Option<String>,
        pub branches: Vec<BranchSnapshot>,
    }

    impl Default for BranchFile {
        fn default() -> Self {
            Self {
                version: STATE_VERSION,
                active_branch: None,
                branches: Vec::new(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::memory::{ChatMessage, MemoryLayer};

    fn paths(dir: &tempfile::TempDir) -> DataPaths {
        DataPaths::new(dir.path())
    }

    #[test]
    fn stm_roundtrip_splits_summary() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(&dir);
        let now = Utc::now();
        let mut state = StmState::empty(now);
        state.messages.push(ChatMessage::user("привет"));
        state.summary = Some("сводка".into());
        state.covered_messages = 3;
        save_stm(&paths, &state).unwrap();

        let raw = std::fs::read_to_string(paths.stm_conversation()).unwrap();
        assert!(!raw.contains("сводка"));
        let summary = std::fs::read_to_string(paths.stm_summary()).unwrap();
        assert!(summary.contains("сводка"));

        let loaded = load_stm(&paths).unwrap();
        assert_eq!(loaded.messages.len(), 1);
        assert_eq!(loaded.summary.as_deref(), Some("сводка"));
        assert_eq!(loaded.covered_messages, 3);
    }

    #[test]
    fn each_layer_roundtrips_independently() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(&dir);
        let now = Utc::now();
        let mut wm = WorkingMemoryState::empty(now);
        wm.task_title = "Смета".into();
        let ltm = LtmState::empty(now);
        let pending = Vec::new();
        let meta = MemoryMetaState::empty();
        save_working(&paths, &wm).unwrap();
        save_ltm(&paths, &ltm).unwrap();
        save_pending(&paths, &pending).unwrap();
        save_meta(&paths, &meta).unwrap();

        assert_eq!(load_working(&paths).unwrap(), wm);
        assert_eq!(load_ltm(&paths).unwrap(), ltm);
        assert!(load_pending(&paths).unwrap().is_empty());
        assert_eq!(load_meta(&paths).unwrap(), meta);
    }

    #[test]
    fn missing_bundle_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = load_bundle(&paths(&dir)).unwrap();
        assert!(bundle.stm.messages.is_empty());
        assert!(bundle.wm.items.is_empty());
        assert!(bundle.ltm.records.is_empty());
        assert!(bundle.pending.is_empty());
    }

    #[test]
    fn corrupt_json_is_reported_and_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(&dir);
        std::fs::create_dir_all(paths.root.join("ltm")).unwrap();
        std::fs::write(paths.ltm(), "{ это не json".as_bytes()).unwrap();
        let error = load_ltm(&paths).unwrap_err();
        assert!(matches!(error, LoadError::Corrupt { .. }));
        let raw = std::fs::read(paths.ltm()).unwrap();
        assert_eq!(raw, "{ это не json".as_bytes());
    }

    #[test]
    fn unsupported_version_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(&dir);
        paths.ensure().unwrap();
        std::fs::write(
            paths.working(),
            br#"{"version":99,"task_id":null,"task_title":"","started_at":null,"updated_at":"2026-11-17T10:00:00Z","items":[]}"#,
        )
        .unwrap();
        match load_working(&paths) {
            Err(LoadError::UnsupportedVersion { found, .. }) => assert_eq!(found, 99),
            other => panic!("ожидали UnsupportedVersion, получили {other:?}"),
        }
    }

    #[test]
    fn settings_roundtrip_and_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(&dir);
        let settings = Settings::default();
        save_settings(&paths, &settings).unwrap();
        assert_eq!(load_settings(&paths).unwrap(), settings);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(paths.settings())
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600);
        }
    }

    #[test]
    fn clear_marker_does_not_touch_other_layers() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(&dir);
        let now = Utc::now();
        let mut stm = StmState::empty(now);
        stm.messages.push(ChatMessage::user("останусь"));
        save_stm(&paths, &stm).unwrap();
        let mut wm = WorkingMemoryState::empty(now);
        wm.task_title = "задача".into();
        save_working(&paths, &wm).unwrap();
        save_ltm(&paths, &LtmState::empty(now)).unwrap();

        // Очистка LTM — только файл LTM и pending (см. controller).
        save_ltm(&paths, &LtmState::empty(now)).unwrap();
        save_pending(&paths, &[]).unwrap();
        assert_eq!(load_stm(&paths).unwrap().messages.len(), 1);
        assert_eq!(load_working(&paths).unwrap().task_title, "задача");
        assert!(load_ltm(&paths).unwrap().records.is_empty());
    }

    #[test]
    fn bundle_save_skips_no_layer_files() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(&dir);
        let bundle = MemoryBundle::empty(Utc::now());
        save_bundle(&paths, &bundle).unwrap();
        for file in [
            paths.stm_conversation(),
            paths.stm_summary(),
            paths.working(),
            paths.ltm(),
            paths.pending(),
            paths.meta(),
        ] {
            assert!(file.exists(), "нет файла {}", file.display());
        }
        let loaded = load_bundle(&paths).unwrap();
        assert_eq!(loaded.stm.conversation_id, bundle.stm.conversation_id);
    }

    #[test]
    fn layer_files_are_distinct() {
        let paths = DataPaths::new("/tmp/x");
        let files = [
            paths.stm_conversation(),
            paths.working(),
            paths.ltm(),
            paths.pending(),
            paths.meta(),
        ];
        for (index, left) in files.iter().enumerate() {
            for right in files.iter().skip(index + 1) {
                assert_ne!(left, right);
            }
        }
        let _ = MemoryLayer::Ltm;
    }
}
