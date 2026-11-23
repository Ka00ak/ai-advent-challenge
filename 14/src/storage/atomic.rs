//! Атомарная запись JSON: tmp → fsync → rename, с `.bak` и правами 0600.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SaveError {
    #[error("не удалось записать {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("не удалось сериализовать {what}: {message}")]
    Serialize { what: String, message: String },
}

fn io(path: &Path, source: std::io::Error) -> SaveError {
    SaveError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// Пишет JSON атомарно. Старый файл копируется в `.bak` перед заменой.
pub fn atomic_write_json<T: Serialize + ?Sized>(path: &Path, value: &T) -> Result<(), SaveError> {
    let data = serde_json::to_vec_pretty(value).map_err(|error| SaveError::Serialize {
        what: path.display().to_string(),
        message: error.to_string(),
    })?;
    atomic_write_bytes(path, &data)?;
    Ok(())
}

/// Атомарная запись сырых байтов (для экспорта журналов).
pub fn atomic_write_bytes(path: &Path, data: &[u8]) -> Result<(), SaveError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| io(parent, error))?;
    }
    let tmp = tmp_path(path);
    {
        let mut file = File::create(&tmp).map_err(|error| io(&tmp, error))?;
        file.write_all(data).map_err(|error| io(&tmp, error))?;
        file.flush().map_err(|error| io(&tmp, error))?;
        file.sync_all().map_err(|error| io(&tmp, error))?;
    }

    if path.exists() {
        let backup = bak_path(path);
        fs::copy(path, &backup).map_err(|error| io(&backup, error))?;
    }

    fs::rename(&tmp, path).map_err(|error| io(path, error))?;
    if let Some(parent) = path.parent() {
        if let Ok(dir) = File::open(parent) {
            let _ = dir.sync_all();
        }
    }
    Ok(())
}

/// На Unix выставляет права 0600 для чувствительных файлов.
pub fn set_private_permissions(path: &Path) -> Result<(), SaveError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let permissions = fs::Permissions::from_mode(0o600);
        fs::set_permissions(path, permissions).map_err(|error| io(path, error))?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

/// Копирует битый файл в `*.corrupt-<ts>` перед reset слоя.
pub fn quarantine_corrupt(path: &Path) -> Result<PathBuf, SaveError> {
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let target = path.with_extension(format!("corrupt-{stamp}"));
    fs::rename(path, &target).map_err(|error| io(path, error))?;
    Ok(target)
}

fn tmp_path(path: &Path) -> PathBuf {
    let mut os = path.as_os_str().to_os_string();
    os.push(".tmp");
    PathBuf::from(os)
}

fn bak_path(path: &Path) -> PathBuf {
    let mut os = path.as_os_str().to_os_string();
    os.push(".bak");
    PathBuf::from(os)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_creates_file_and_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("data.json");
        atomic_write_json(&path, &serde_json::json!({"a": 1})).unwrap();
        assert!(path.exists());
        assert!(!dir.path().join("nested").join("data.json.tmp").exists());

        atomic_write_json(&path, &serde_json::json!({"a": 2})).unwrap();
        let backup = dir.path().join("nested").join("data.json.bak");
        assert!(backup.exists());
        let raw = std::fs::read_to_string(&backup).unwrap();
        assert!(raw.contains("\"a\": 1"));
    }

    #[cfg(unix)]
    #[test]
    fn private_permissions_are_0600() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        atomic_write_json(&path, &serde_json::json!({})).unwrap();
        set_private_permissions(&path).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn corrupt_file_is_quarantined() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ltm.json");
        std::fs::write(&path, b"{broken").unwrap();
        let target = quarantine_corrupt(&path).unwrap();
        assert!(!path.exists());
        assert!(target.exists());
    }
}
