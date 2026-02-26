use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const ENTRIES_DIR: &str = "/boot/loader/entries";

/// Backup of a systemd-boot entry before bop changed kernel params.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KernelParamBackup {
    pub path: String,
    pub original_content: String,
}

/// Add kernel parameters to systemd-boot configuration.
/// Modifies /boot/loader/entries/*.conf files.
pub fn add_kernel_params(params: &[String]) -> Result<Vec<KernelParamBackup>> {
    add_kernel_params_in_dir(params, Path::new(ENTRIES_DIR))
}

fn add_kernel_params_in_dir(
    params: &[String],
    entries_dir: &Path,
) -> Result<Vec<KernelParamBackup>> {
    if !entries_dir.exists() {
        return Err(Error::Bootloader(format!(
            "systemd-boot entries directory not found at {}",
            entries_dir.display()
        )));
    }

    let entries = list_entry_files(entries_dir)?;
    let mut backups = Vec::new();

    if entries.is_empty() {
        return Err(Error::Bootloader(format!(
            "no .conf files found in {}",
            entries_dir.display()
        )));
    }

    for entry in &entries {
        let path = entry.clone();
        let content = std::fs::read_to_string(&path)
            .map_err(|e| Error::Bootloader(format!("failed to read {}: {}", path.display(), e)))?;

        let new_content = build_content_with_added_params(&content, &path, params)?;

        if new_content != content {
            backups.push(KernelParamBackup {
                path: path.display().to_string(),
                original_content: content,
            });
            if let Err(e) = std::fs::write(&path, new_content) {
                let _ = restore_kernel_param_backups(&backups);
                return Err(Error::Bootloader(format!(
                    "failed to write {}: {}",
                    path.display(),
                    e
                )));
            }
        }
    }

    Ok(backups)
}

/// Remove kernel parameters from systemd-boot configuration.
pub fn remove_kernel_params(params: &[String]) -> Result<()> {
    remove_kernel_params_in_dir(params, Path::new(ENTRIES_DIR))
}

fn remove_kernel_params_in_dir(params: &[String], entries_dir: &Path) -> Result<()> {
    if !entries_dir.exists() {
        return Ok(()); // Nothing to undo
    }

    let entries = list_entry_files(entries_dir)?;

    for entry in &entries {
        let path = entry.clone();
        let content = std::fs::read_to_string(&path)
            .map_err(|e| Error::Bootloader(format!("failed to read {}: {}", path.display(), e)))?;

        let param_names: Vec<&str> = params
            .iter()
            .map(|p| p.split('=').next().unwrap_or(p))
            .collect();

        let new_content = build_content_with_removed_params(&content, &param_names);
        if new_content != content {
            std::fs::write(&path, new_content).map_err(|e| {
                Error::Bootloader(format!("failed to write {}: {}", path.display(), e))
            })?;
        }
    }

    Ok(())
}

/// Restore systemd-boot entries to the exact content captured before `add_kernel_params`.
/// Attempts every backup even if some fail, then reports all errors.
pub fn restore_kernel_param_backups(backups: &[KernelParamBackup]) -> Result<()> {
    let errors: Vec<String> = backups
        .iter()
        .filter_map(|backup| {
            std::fs::write(&backup.path, &backup.original_content)
                .err()
                .map(|e| format!("{}: {}", backup.path, e))
        })
        .collect();

    if errors.is_empty() {
        Ok(())
    } else {
        Err(Error::Bootloader(format!(
            "failed to restore {} of {} entries: {}",
            errors.len(),
            backups.len(),
            errors.join("; ")
        )))
    }
}

fn list_entry_files(entries_dir: &Path) -> Result<Vec<PathBuf>> {
    Ok(std::fs::read_dir(entries_dir)
        .map_err(|e| Error::Bootloader(format!("failed to read entries dir: {}", e)))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "conf"))
        .collect())
}

fn build_content_with_added_params(
    content: &str,
    path: &Path,
    params: &[String],
) -> Result<String> {
    let mut new_lines = Vec::new();
    let mut options_found = false;

    for line in content.lines() {
        if line.starts_with("options") {
            options_found = true;
            new_lines.push(add_params_to_options_line(line, params));
        } else {
            new_lines.push(line.to_string());
        }
    }

    if !options_found {
        return Err(Error::Bootloader(format!(
            "no 'options' line found in {}",
            path.display()
        )));
    }

    Ok(preserve_newline(&new_lines.join("\n"), content))
}

fn build_content_with_removed_params(content: &str, param_names: &[&str]) -> String {
    let mut new_lines = Vec::new();

    for line in content.lines() {
        if line.starts_with("options") {
            new_lines.push(remove_params_from_options_line(line, param_names));
        } else {
            new_lines.push(line.to_string());
        }
    }

    preserve_newline(&new_lines.join("\n"), content)
}

fn add_params_to_options_line(line: &str, params: &[String]) -> String {
    let mut options_line = line.to_string();

    for param in params {
        let param_name = param.split('=').next().unwrap_or(param);

        // Check if this exact param=value already exists.
        let already_set = options_line
            .split_whitespace()
            .any(|word| word == param.as_str());
        if already_set {
            continue;
        }

        // Replace existing value in place, or append if not present.
        let words: Vec<&str> = options_line.split_whitespace().collect();
        let mut found = false;
        let replaced: Vec<&str> = words
            .into_iter()
            .map(|word| {
                let word_name = word.split('=').next().unwrap_or(word);
                if word_name == param_name && word != "options" {
                    found = true;
                    param.as_str()
                } else {
                    word
                }
            })
            .collect();
        options_line = replaced.join(" ");
        if !found {
            options_line.push(' ');
            options_line.push_str(param);
        }
    }

    options_line
}

fn remove_params_from_options_line(line: &str, param_names: &[&str]) -> String {
    let words: Vec<&str> = line.split_whitespace().collect();
    let filtered: Vec<&str> = words
        .into_iter()
        .filter(|word| {
            if *word == "options" {
                return true;
            }
            let word_name = word.split('=').next().unwrap_or(word);
            !param_names.contains(&word_name)
        })
        .collect();
    filtered.join(" ")
}

fn preserve_newline(new_content: &str, original_content: &str) -> String {
    if original_content.ends_with('\n') {
        format!("{}\n", new_content)
    } else {
        new_content.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_add_kernel_params_records_backup_and_restore_recovers_old_value() {
        let tmp = TempDir::new().expect("create temp dir");
        let entries = tmp.path().join("entries");
        fs::create_dir_all(&entries).expect("create entries dir");
        let entry = entries.join("linux.conf");

        let original = "\
title Linux
linux /vmlinuz-linux
options root=UUID=abc quiet acpi.ec_no_wakeup=0 rtc_cmos.use_acpi_alarm=0
";
        fs::write(&entry, original).expect("write entry");

        let params = vec![
            "acpi.ec_no_wakeup=1".to_string(),
            "rtc_cmos.use_acpi_alarm=1".to_string(),
        ];
        let backups = add_kernel_params_in_dir(&params, &entries).expect("apply params");

        assert_eq!(backups.len(), 1);
        assert_eq!(backups[0].path, entry.display().to_string());
        assert_eq!(backups[0].original_content, original);

        let updated = fs::read_to_string(&entry).expect("read updated entry");
        assert!(updated.contains("acpi.ec_no_wakeup=1"));
        assert!(updated.contains("rtc_cmos.use_acpi_alarm=1"));
        assert!(!updated.contains("acpi.ec_no_wakeup=0"));
        assert!(!updated.contains("rtc_cmos.use_acpi_alarm=0"));

        restore_kernel_param_backups(&backups).expect("restore backups");
        let restored = fs::read_to_string(&entry).expect("read restored entry");
        assert_eq!(restored, original);
    }

    #[test]
    fn test_add_kernel_params_no_change_returns_no_backup() {
        let tmp = TempDir::new().expect("create temp dir");
        let entries = tmp.path().join("entries");
        fs::create_dir_all(&entries).expect("create entries dir");
        let entry = entries.join("linux.conf");
        let content = "options quiet acpi.ec_no_wakeup=1 rtc_cmos.use_acpi_alarm=1\n";
        fs::write(&entry, content).expect("write entry");

        let params = vec![
            "acpi.ec_no_wakeup=1".to_string(),
            "rtc_cmos.use_acpi_alarm=1".to_string(),
        ];
        let backups = add_kernel_params_in_dir(&params, &entries).expect("apply params");

        assert!(backups.is_empty());
        let after = fs::read_to_string(&entry).expect("read entry");
        assert_eq!(after, content);
    }

    #[test]
    fn test_remove_kernel_params_strips_matching_params() {
        let tmp = TempDir::new().expect("create temp dir");
        let entries = tmp.path().join("entries");
        fs::create_dir_all(&entries).expect("create entries dir");
        let entry = entries.join("linux.conf");
        let content = "title Linux\noptions root=UUID=abc quiet acpi.ec_no_wakeup=1 rtc_cmos.use_acpi_alarm=1\n";
        fs::write(&entry, content).expect("write entry");

        let params = vec![
            "acpi.ec_no_wakeup=1".to_string(),
            "rtc_cmos.use_acpi_alarm=1".to_string(),
        ];
        remove_kernel_params_in_dir(&params, &entries).expect("remove params");

        let after = fs::read_to_string(&entry).expect("read entry");
        assert!(!after.contains("acpi.ec_no_wakeup"));
        assert!(!after.contains("rtc_cmos.use_acpi_alarm"));
        assert!(after.contains("root=UUID=abc"));
        assert!(after.contains("quiet"));
    }

    #[test]
    fn test_add_kernel_params_preserves_ordering() {
        let tmp = TempDir::new().expect("create temp dir");
        let entries = tmp.path().join("entries");
        fs::create_dir_all(&entries).expect("create entries dir");
        let entry = entries.join("linux.conf");
        let content = "options root=UUID=abc acpi.ec_no_wakeup=0 quiet\n";
        fs::write(&entry, content).expect("write entry");

        let params = vec!["acpi.ec_no_wakeup=1".to_string()];
        let backups = add_kernel_params_in_dir(&params, &entries).expect("apply params");

        assert_eq!(backups.len(), 1);
        let after = fs::read_to_string(&entry).expect("read entry");
        // Param should be replaced in place, not moved to end
        assert_eq!(after, "options root=UUID=abc acpi.ec_no_wakeup=1 quiet\n");
    }
}
