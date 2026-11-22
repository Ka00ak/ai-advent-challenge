//! Разрешение data-dir и пути файлов.

use std::path::{Path, PathBuf};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum PathError {
    #[error("не удалось определить домашний каталог")]
    NoHome,
    #[error("не удалось создать каталог {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataPaths {
    pub root: PathBuf,
}

impl DataPaths {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn stm_conversation(&self) -> PathBuf {
        self.root.join("stm").join("conversation.json")
    }

    pub fn stm_summary(&self) -> PathBuf {
        self.root.join("stm").join("summary.json")
    }

    pub fn working(&self) -> PathBuf {
        self.root.join("wm").join("working.json")
    }

    pub fn ltm(&self) -> PathBuf {
        self.root.join("ltm").join("ltm.json")
    }

    pub fn pending(&self) -> PathBuf {
        self.root.join("pending").join("pending.json")
    }

    pub fn meta(&self) -> PathBuf {
        self.root.join("meta").join("meta.json")
    }

    pub fn migration_report(&self) -> PathBuf {
        self.root.join("migration").join("report.json")
    }

    pub fn profiles(&self) -> PathBuf {
        self.root.join("profiles").join("profiles.json")
    }

    pub fn settings(&self) -> PathBuf {
        self.root.join("settings.json")
    }

    pub fn secrets(&self) -> PathBuf {
        self.root.join("secrets.json")
    }

    pub fn lock(&self) -> PathBuf {
        self.root.join("lock")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }

    pub fn ensure(&self) -> Result<(), PathError> {
        for dir in [
            self.root.clone(),
            self.root.join("stm"),
            self.root.join("wm"),
            self.root.join("ltm"),
            self.root.join("pending"),
            self.root.join("meta"),
            self.root.join("migration"),
            self.root.join("profiles"),
            self.root.join("logs"),
        ] {
            std::fs::create_dir_all(&dir).map_err(|source| PathError::Io {
                path: dir.clone(),
                source,
            })?;
        }
        Ok(())
    }
}

/// CLI → `LAB12_HOME` (или совместимый `LAB11_HOME`) → `XDG_DATA_HOME/lab12`
/// → `~/.local/share/lab12`.
pub fn resolve_data_dir(cli: Option<&Path>) -> Result<DataPaths, PathError> {
    if let Some(path) = cli {
        return Ok(DataPaths::new(path));
    }
    for variable in ["LAB12_HOME", "LAB11_HOME"] {
        if let Ok(value) = std::env::var(variable) {
            if !value.trim().is_empty() {
                return Ok(DataPaths::new(PathBuf::from(value)));
            }
        }
    }
    if let Ok(value) = std::env::var("XDG_DATA_HOME") {
        if !value.trim().is_empty() {
            return Ok(DataPaths::new(PathBuf::from(value).join("lab12")));
        }
    }
    if let Some(base) = directories::BaseDirs::new() {
        return Ok(DataPaths::new(base.data_dir().join("lab12")));
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or(PathError::NoHome)?;
    Ok(DataPaths::new(
        home.join(".local").join("share").join("lab12"),
    ))
}

pub fn expand_tilde(input: &str) -> PathBuf {
    if let Some(rest) = input.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_separated_by_layer() {
        let paths = DataPaths::new("/tmp/lab12-test");
        assert!(paths.stm_conversation().ends_with("stm/conversation.json"));
        assert!(paths.working().ends_with("wm/working.json"));
        assert!(paths.ltm().ends_with("ltm/ltm.json"));
        assert!(paths.pending().ends_with("pending/pending.json"));
        assert_ne!(paths.stm_conversation(), paths.ltm());
    }

    #[test]
    fn cli_path_wins() {
        let paths = resolve_data_dir(Some(Path::new("/tmp/custom-lab12"))).unwrap();
        assert_eq!(paths.root, PathBuf::from("/tmp/custom-lab12"));
    }

    #[test]
    fn ensure_creates_directories() {
        let dir = tempfile::tempdir().unwrap();
        let paths = DataPaths::new(dir.path().join("data"));
        paths.ensure().unwrap();
        assert!(paths.root.join("stm").is_dir());
    }
}
