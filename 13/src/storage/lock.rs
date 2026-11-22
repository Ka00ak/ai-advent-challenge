//! Lock-файл от одновременного запуска двух экземпляров.

use std::fs::{File, OpenOptions};
use std::path::PathBuf;

use fs2::FileExt;
use thiserror::Error;

use crate::storage::paths::DataPaths;

#[derive(Debug, Error)]
pub enum LockError {
    #[error("data-dir уже используется другим экземпляром: {path}")]
    Busy { path: PathBuf },
    #[error("не удалось открыть lock-файл {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

pub struct LockGuard {
    file: File,
    #[allow(dead_code)]
    path: PathBuf,
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

pub fn acquire_lock(paths: &DataPaths) -> Result<LockGuard, LockError> {
    paths.ensure().map_err(|error| LockError::Io {
        path: paths.root.clone(),
        source: std::io::Error::other(error.to_string()),
    })?;
    let path = paths.lock();
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&path)
        .map_err(|source| LockError::Io {
            path: path.clone(),
            source,
        })?;
    match file.try_lock_exclusive() {
        Ok(()) => Ok(LockGuard { file, path }),
        Err(_) => Err(LockError::Busy { path }),
    }
}

impl std::fmt::Debug for LockGuard {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("LockGuard").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_lock_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let paths = DataPaths::new(dir.path());
        let _first = acquire_lock(&paths).unwrap();
        let second = acquire_lock(&paths);
        assert!(matches!(second, Err(LockError::Busy { .. })));
    }

    #[test]
    fn lock_released_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        let paths = DataPaths::new(dir.path());
        {
            let _guard = acquire_lock(&paths).unwrap();
        }
        assert!(acquire_lock(&paths).is_ok());
    }
}
