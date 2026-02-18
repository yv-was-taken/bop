use crate::error::{Error, Result};
use std::path::{Path, PathBuf};

/// Abstraction over sysfs/procfs filesystem root.
/// Defaults to `/` in production, redirectable to a temp directory for testing.
#[derive(Debug, Clone)]
pub struct SysfsRoot {
    root: PathBuf,
}

impl Default for SysfsRoot {
    fn default() -> Self {
        Self {
            root: PathBuf::from("/"),
        }
    }
}

impl SysfsRoot {
    /// Create a SysfsRoot pointing at the real system.
    pub fn system() -> Self {
        Self::default()
    }

    /// Create a SysfsRoot pointing at a custom directory (for testing).
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Resolve a path relative to this root.
    /// e.g., `path("sys/class/power_supply")` -> `/sys/class/power_supply` or `<test_root>/sys/class/power_supply`
    pub fn path(&self, relative: impl AsRef<Path>) -> PathBuf {
        self.root.join(relative)
    }

    /// Read a sysfs/procfs file, trimming whitespace.
    pub fn read(&self, relative: impl AsRef<Path>) -> Result<String> {
        let path = self.path(relative);
        std::fs::read_to_string(&path)
            .map(|s| s.trim().to_string())
            .map_err(|e| Error::SysfsRead {
                path,
                source: e,
            })
    }

    /// Read a sysfs file, returning None if it doesn't exist.
    pub fn read_optional(&self, relative: impl AsRef<Path>) -> Result<Option<String>> {
        let path = self.path(relative);
        match std::fs::read_to_string(&path) {
            Ok(s) => Ok(Some(s.trim().to_string())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => Ok(None),
            Err(e) => Err(Error::SysfsRead { path, source: e }),
        }
    }

    /// Write a value to a sysfs file.
    pub fn write(&self, relative: impl AsRef<Path>, value: &str) -> Result<()> {
        let path = self.path(relative);
        std::fs::write(&path, value).map_err(|e| Error::SysfsWrite {
            path,
            source: e,
        })
    }

    /// Read a sysfs file and parse it as a specific type.
    pub fn read_parse<T: std::str::FromStr>(&self, relative: impl AsRef<Path>) -> Result<T>
    where
        T::Err: std::fmt::Display,
    {
        let relative = relative.as_ref();
        let value = self.read(relative)?;
        value.parse::<T>().map_err(|e| Error::Parse {
            path: self.path(relative),
            detail: format!("failed to parse '{}': {}", value, e),
        })
    }

    /// List entries in a sysfs directory.
    pub fn list_dir(&self, relative: impl AsRef<Path>) -> Result<Vec<String>> {
        let path = self.path(relative);
        let entries = std::fs::read_dir(&path).map_err(|e| Error::SysfsRead {
            path: path.clone(),
            source: e,
        })?;
        let mut names = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| Error::SysfsRead {
                path: path.clone(),
                source: e,
            })?;
            if let Some(name) = entry.file_name().to_str() {
                names.push(name.to_string());
            }
        }
        names.sort();
        Ok(names)
    }

    /// Check if a path exists relative to this root.
    pub fn exists(&self, relative: impl AsRef<Path>) -> bool {
        self.path(relative).exists()
    }

    /// Get the root path.
    pub fn root(&self) -> &Path {
        &self.root
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_read_write() {
        let tmp = tempfile::tempdir().unwrap();
        let sysfs = SysfsRoot::new(tmp.path());

        fs::create_dir_all(tmp.path().join("sys/test")).unwrap();
        fs::write(tmp.path().join("sys/test/value"), "42\n").unwrap();

        assert_eq!(sysfs.read("sys/test/value").unwrap(), "42");
        assert_eq!(sysfs.read_parse::<u32>("sys/test/value").unwrap(), 42);
    }

    #[test]
    fn test_read_optional_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let sysfs = SysfsRoot::new(tmp.path());

        assert_eq!(sysfs.read_optional("sys/nonexistent").unwrap(), None);
    }

    #[test]
    fn test_list_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let sysfs = SysfsRoot::new(tmp.path());

        fs::create_dir_all(tmp.path().join("sys/devices")).unwrap();
        fs::write(tmp.path().join("sys/devices/a"), "").unwrap();
        fs::write(tmp.path().join("sys/devices/b"), "").unwrap();

        let entries = sysfs.list_dir("sys/devices").unwrap();
        assert_eq!(entries, vec!["a", "b"]);
    }
}
