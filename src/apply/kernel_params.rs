use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const SYSTEMD_BOOT_ENTRIES_DIR: &str = "/boot/loader/entries";
const GRUB_DEFAULT: &str = "/etc/default/grub";
const GRUB_CMDLINE_VAR: &str = "GRUB_CMDLINE_LINUX_DEFAULT";

/// Detected bootloader type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootloaderType {
    SystemdBoot,
    Grub,
}

/// Detect which bootloader is in use.
///
/// Checks systemd-boot first (`/boot/loader/entries`) because it is more
/// definitive — `/etc/default/grub` can linger after switching bootloaders.
pub fn detect_bootloader() -> Result<BootloaderType> {
    detect_bootloader_with_root(Path::new("/"))
}

fn detect_bootloader_with_root(root: &Path) -> Result<BootloaderType> {
    if root.join("boot/loader/entries").exists() {
        return Ok(BootloaderType::SystemdBoot);
    }
    if root.join("etc/default/grub").exists() {
        return Ok(BootloaderType::Grub);
    }
    Err(Error::Bootloader(
        "no supported bootloader found (checked systemd-boot and GRUB)".into(),
    ))
}

/// Backup of a boot entry before bop changed kernel params.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KernelParamBackup {
    pub path: String,
    pub original_content: String,
}

// ---------------------------------------------------------------------------
// Public API — auto-detects bootloader and dispatches
// ---------------------------------------------------------------------------

/// Add kernel parameters to the detected bootloader configuration.
pub fn add_kernel_params(params: &[String]) -> Result<Vec<KernelParamBackup>> {
    match detect_bootloader()? {
        BootloaderType::SystemdBoot => {
            add_kernel_params_systemd_boot(params, Path::new(SYSTEMD_BOOT_ENTRIES_DIR))
        }
        BootloaderType::Grub => {
            let backups = add_kernel_params_grub(params, Path::new(GRUB_DEFAULT))?;
            if !backups.is_empty() {
                regenerate_grub_config()?;
            }
            Ok(backups)
        }
    }
}

/// Remove kernel parameters from the detected bootloader configuration.
pub fn remove_kernel_params(params: &[String]) -> Result<()> {
    match detect_bootloader()? {
        BootloaderType::SystemdBoot => {
            remove_kernel_params_systemd_boot(params, Path::new(SYSTEMD_BOOT_ENTRIES_DIR))
        }
        BootloaderType::Grub => {
            let changed = remove_kernel_params_grub(params, Path::new(GRUB_DEFAULT))?;
            if changed {
                regenerate_grub_config()?;
            }
            Ok(())
        }
    }
}

/// Restore boot entries to the exact content captured before `add_kernel_params`.
/// Attempts every backup even if some fail, then reports all errors.
/// If any backup targets a GRUB file, runs `grub-mkconfig` after restore.
pub fn restore_kernel_param_backups(backups: &[KernelParamBackup]) -> Result<()> {
    let errors: Vec<String> = backups
        .iter()
        .filter_map(|backup| {
            std::fs::write(&backup.path, &backup.original_content)
                .err()
                .map(|e| format!("{}: {}", backup.path, e))
        })
        .collect();

    if !errors.is_empty() {
        return Err(Error::Bootloader(format!(
            "failed to restore {} of {} entries: {}",
            errors.len(),
            backups.len(),
            errors.join("; ")
        )));
    }

    // If we restored a GRUB config, regenerate grub.cfg.
    let has_grub = backups.iter().any(|b| b.path == GRUB_DEFAULT);
    if has_grub {
        regenerate_grub_config()?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// systemd-boot implementation
// ---------------------------------------------------------------------------

fn add_kernel_params_systemd_boot(
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
            if let Err(e) = std::fs::write(&path, &new_content) {
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

fn remove_kernel_params_systemd_boot(params: &[String], entries_dir: &Path) -> Result<()> {
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
            new_lines.push(add_params_to_line(line, params));
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
            new_lines.push(remove_params_from_line(line, param_names));
        } else {
            new_lines.push(line.to_string());
        }
    }

    preserve_newline(&new_lines.join("\n"), content)
}

// ---------------------------------------------------------------------------
// GRUB implementation
// ---------------------------------------------------------------------------

/// Add kernel parameters to `/etc/default/grub`.
/// Returns backups if changes were made.
fn add_kernel_params_grub(params: &[String], grub_path: &Path) -> Result<Vec<KernelParamBackup>> {
    let content = std::fs::read_to_string(grub_path)
        .map_err(|e| Error::Bootloader(format!("failed to read {}: {}", grub_path.display(), e)))?;

    let new_content = build_grub_content_with_added_params(&content, params)?;

    if new_content == content {
        return Ok(Vec::new());
    }

    let backup = KernelParamBackup {
        path: grub_path.display().to_string(),
        original_content: content,
    };

    std::fs::write(grub_path, &new_content).map_err(|e| {
        Error::Bootloader(format!("failed to write {}: {}", grub_path.display(), e))
    })?;

    Ok(vec![backup])
}

/// Remove kernel parameters from `/etc/default/grub`.
/// Returns true if the file was modified.
fn remove_kernel_params_grub(params: &[String], grub_path: &Path) -> Result<bool> {
    if !grub_path.exists() {
        return Ok(false);
    }

    let content = std::fs::read_to_string(grub_path)
        .map_err(|e| Error::Bootloader(format!("failed to read {}: {}", grub_path.display(), e)))?;

    let param_names: Vec<&str> = params
        .iter()
        .map(|p| p.split('=').next().unwrap_or(p))
        .collect();

    let new_content = build_grub_content_with_removed_params(&content, &param_names);

    if new_content == content {
        return Ok(false);
    }

    std::fs::write(grub_path, &new_content).map_err(|e| {
        Error::Bootloader(format!("failed to write {}: {}", grub_path.display(), e))
    })?;

    Ok(true)
}

fn build_grub_content_with_added_params(content: &str, params: &[String]) -> Result<String> {
    let mut new_lines = Vec::new();
    let mut found = false;

    for line in content.lines() {
        if is_grub_cmdline_line(line) {
            found = true;
            new_lines.push(modify_grub_cmdline(line, |value| {
                add_params_to_value(value, params)
            }));
        } else {
            new_lines.push(line.to_string());
        }
    }

    if !found {
        return Err(Error::Bootloader(format!(
            "no {} line found in GRUB config",
            GRUB_CMDLINE_VAR
        )));
    }

    Ok(preserve_newline(&new_lines.join("\n"), content))
}

fn build_grub_content_with_removed_params(content: &str, param_names: &[&str]) -> String {
    let mut new_lines = Vec::new();

    for line in content.lines() {
        if is_grub_cmdline_line(line) {
            new_lines.push(modify_grub_cmdline(line, |value| {
                remove_params_from_value(value, param_names)
            }));
        } else {
            new_lines.push(line.to_string());
        }
    }

    preserve_newline(&new_lines.join("\n"), content)
}

/// Check if a line is the GRUB_CMDLINE_LINUX_DEFAULT assignment.
fn is_grub_cmdline_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with(GRUB_CMDLINE_VAR) && trimmed[GRUB_CMDLINE_VAR.len()..].starts_with('=')
}

/// Parse a `GRUB_CMDLINE_LINUX_DEFAULT="..."` line, apply a transformation
/// to the value, and reconstruct the line preserving quoting style.
fn modify_grub_cmdline(line: &str, f: impl FnOnce(&str) -> String) -> String {
    // Find the = sign
    let eq_pos = match line.find('=') {
        Some(pos) => pos,
        None => return line.to_string(),
    };

    let prefix = &line[..=eq_pos]; // "GRUB_CMDLINE_LINUX_DEFAULT="
    let raw_value = &line[eq_pos + 1..];

    // Detect and strip quotes
    let (open_quote, value, close_quote) = if let Some(stripped) = raw_value.strip_prefix('"') {
        let inner = stripped.strip_suffix('"').unwrap_or(stripped);
        ("\"", inner, "\"")
    } else if let Some(stripped) = raw_value.strip_prefix('\'') {
        let inner = stripped.strip_suffix('\'').unwrap_or(stripped);
        ("'", inner, "'")
    } else {
        ("\"", raw_value, "\"")
    };

    let new_value = f(value);
    format!("{}{}{}{}", prefix, open_quote, new_value, close_quote)
}

/// Run `grub-mkconfig` to regenerate `/boot/grub/grub.cfg`.
fn regenerate_grub_config() -> Result<()> {
    let output_path = if Path::new("/boot/grub/grub.cfg").exists() {
        "/boot/grub/grub.cfg"
    } else if Path::new("/boot/grub2/grub.cfg").exists() {
        "/boot/grub2/grub.cfg"
    } else {
        return Err(Error::Bootloader(
            "grub.cfg not found at /boot/grub/grub.cfg or /boot/grub2/grub.cfg".into(),
        ));
    };

    let status = std::process::Command::new("grub-mkconfig")
        .args(["-o", output_path])
        .status()
        .map_err(|e| Error::Bootloader(format!("failed to run grub-mkconfig: {}", e)))?;

    if !status.success() {
        return Err(Error::Bootloader(format!(
            "grub-mkconfig -o {} failed",
            output_path
        )));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Shared param manipulation helpers
// ---------------------------------------------------------------------------

/// Add params to a space-separated line like `options root=UUID=abc quiet`.
/// Used for systemd-boot `options` lines.
fn add_params_to_line(line: &str, params: &[String]) -> String {
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

/// Remove params from a space-separated line like `options root=UUID=abc quiet`.
/// Used for systemd-boot `options` lines.
fn remove_params_from_line(line: &str, param_names: &[&str]) -> String {
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

/// Add params to a bare value string (no prefix keyword). Used for GRUB values.
fn add_params_to_value(value: &str, params: &[String]) -> String {
    let mut tokens: Vec<String> = value.split_whitespace().map(String::from).collect();

    for param in params {
        let param_name = param.split('=').next().unwrap_or(param);

        // Check if exact param=value already exists.
        if tokens.iter().any(|t| t == param) {
            continue;
        }

        // Replace existing same-name param in place, or append.
        let mut found = false;
        for token in &mut tokens {
            let token_name = token.split('=').next().unwrap_or(token);
            if token_name == param_name {
                *token = param.clone();
                found = true;
                break;
            }
        }
        if !found {
            tokens.push(param.clone());
        }
    }

    tokens.join(" ")
}

/// Remove params from a bare value string. Used for GRUB values.
fn remove_params_from_value(value: &str, param_names: &[&str]) -> String {
    value
        .split_whitespace()
        .filter(|token| {
            let name = token.split('=').next().unwrap_or(token);
            !param_names.contains(&name)
        })
        .collect::<Vec<_>>()
        .join(" ")
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

    // -----------------------------------------------------------------------
    // Bootloader detection
    // -----------------------------------------------------------------------

    #[test]
    fn test_detect_bootloader_systemd_boot() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join("boot/loader/entries")).unwrap();
        assert_eq!(
            detect_bootloader_with_root(tmp.path()).unwrap(),
            BootloaderType::SystemdBoot
        );
    }

    #[test]
    fn test_detect_bootloader_grub() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join("etc/default")).unwrap();
        fs::write(
            tmp.path().join("etc/default/grub"),
            "GRUB_CMDLINE_LINUX_DEFAULT=\"quiet\"\n",
        )
        .unwrap();
        assert_eq!(
            detect_bootloader_with_root(tmp.path()).unwrap(),
            BootloaderType::Grub
        );
    }

    #[test]
    fn test_detect_bootloader_prefers_systemd_boot() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join("boot/loader/entries")).unwrap();
        fs::create_dir_all(tmp.path().join("etc/default")).unwrap();
        fs::write(
            tmp.path().join("etc/default/grub"),
            "GRUB_CMDLINE_LINUX_DEFAULT=\"quiet\"\n",
        )
        .unwrap();
        assert_eq!(
            detect_bootloader_with_root(tmp.path()).unwrap(),
            BootloaderType::SystemdBoot
        );
    }

    #[test]
    fn test_detect_bootloader_none_found() {
        let tmp = TempDir::new().unwrap();
        let result = detect_bootloader_with_root(tmp.path());
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // systemd-boot (existing tests, updated function names)
    // -----------------------------------------------------------------------

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
        let backups = add_kernel_params_systemd_boot(&params, &entries).expect("apply params");

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
        let backups = add_kernel_params_systemd_boot(&params, &entries).expect("apply params");

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
        remove_kernel_params_systemd_boot(&params, &entries).expect("remove params");

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
        let backups = add_kernel_params_systemd_boot(&params, &entries).expect("apply params");

        assert_eq!(backups.len(), 1);
        let after = fs::read_to_string(&entry).expect("read entry");
        assert_eq!(after, "options root=UUID=abc acpi.ec_no_wakeup=1 quiet\n");
    }

    // -----------------------------------------------------------------------
    // GRUB
    // -----------------------------------------------------------------------

    #[test]
    fn test_add_kernel_params_grub_appends_new() {
        let tmp = TempDir::new().unwrap();
        let grub = tmp.path().join("grub");
        fs::write(
            &grub,
            "GRUB_TIMEOUT=5\nGRUB_CMDLINE_LINUX_DEFAULT=\"quiet splash\"\n",
        )
        .unwrap();

        let params = vec![
            "acpi.ec_no_wakeup=1".to_string(),
            "rtc_cmos.use_acpi_alarm=1".to_string(),
        ];
        let backups = add_kernel_params_grub(&params, &grub).unwrap();

        assert_eq!(backups.len(), 1);
        let after = fs::read_to_string(&grub).unwrap();
        assert!(after.contains("quiet splash acpi.ec_no_wakeup=1 rtc_cmos.use_acpi_alarm=1"));
        assert!(after.contains("GRUB_TIMEOUT=5"));
    }

    #[test]
    fn test_add_kernel_params_grub_replaces_existing() {
        let tmp = TempDir::new().unwrap();
        let grub = tmp.path().join("grub");
        fs::write(
            &grub,
            "GRUB_CMDLINE_LINUX_DEFAULT=\"quiet acpi.ec_no_wakeup=0\"\n",
        )
        .unwrap();

        let params = vec!["acpi.ec_no_wakeup=1".to_string()];
        let backups = add_kernel_params_grub(&params, &grub).unwrap();

        assert_eq!(backups.len(), 1);
        let after = fs::read_to_string(&grub).unwrap();
        assert!(after.contains("acpi.ec_no_wakeup=1"));
        assert!(!after.contains("acpi.ec_no_wakeup=0"));
    }

    #[test]
    fn test_add_kernel_params_grub_no_change() {
        let tmp = TempDir::new().unwrap();
        let grub = tmp.path().join("grub");
        fs::write(
            &grub,
            "GRUB_CMDLINE_LINUX_DEFAULT=\"quiet acpi.ec_no_wakeup=1\"\n",
        )
        .unwrap();

        let params = vec!["acpi.ec_no_wakeup=1".to_string()];
        let backups = add_kernel_params_grub(&params, &grub).unwrap();

        assert!(backups.is_empty());
    }

    #[test]
    fn test_add_kernel_params_grub_single_quotes() {
        let tmp = TempDir::new().unwrap();
        let grub = tmp.path().join("grub");
        fs::write(&grub, "GRUB_CMDLINE_LINUX_DEFAULT='quiet'\n").unwrap();

        let params = vec!["acpi.ec_no_wakeup=1".to_string()];
        let backups = add_kernel_params_grub(&params, &grub).unwrap();

        assert_eq!(backups.len(), 1);
        let after = fs::read_to_string(&grub).unwrap();
        assert!(after.contains("'quiet acpi.ec_no_wakeup=1'"));
    }

    #[test]
    fn test_remove_kernel_params_grub() {
        let tmp = TempDir::new().unwrap();
        let grub = tmp.path().join("grub");
        fs::write(
            &grub,
            "GRUB_CMDLINE_LINUX_DEFAULT=\"quiet acpi.ec_no_wakeup=1 rtc_cmos.use_acpi_alarm=1\"\n",
        )
        .unwrap();

        let params = vec![
            "acpi.ec_no_wakeup=1".to_string(),
            "rtc_cmos.use_acpi_alarm=1".to_string(),
        ];
        let changed = remove_kernel_params_grub(&params, &grub).unwrap();

        assert!(changed);
        let after = fs::read_to_string(&grub).unwrap();
        assert!(!after.contains("acpi.ec_no_wakeup"));
        assert!(!after.contains("rtc_cmos.use_acpi_alarm"));
        assert!(after.contains("quiet"));
    }

    #[test]
    fn test_remove_kernel_params_grub_no_change() {
        let tmp = TempDir::new().unwrap();
        let grub = tmp.path().join("grub");
        fs::write(&grub, "GRUB_CMDLINE_LINUX_DEFAULT=\"quiet\"\n").unwrap();

        let params = vec!["acpi.ec_no_wakeup=1".to_string()];
        let changed = remove_kernel_params_grub(&params, &grub).unwrap();

        assert!(!changed);
    }

    #[test]
    fn test_grub_backup_and_restore_round_trip() {
        let tmp = TempDir::new().unwrap();
        let grub = tmp.path().join("grub");
        let original = "GRUB_TIMEOUT=5\nGRUB_CMDLINE_LINUX_DEFAULT=\"quiet\"\n";
        fs::write(&grub, original).unwrap();

        let params = vec!["acpi.ec_no_wakeup=1".to_string()];
        let backups = add_kernel_params_grub(&params, &grub).unwrap();

        assert_eq!(backups.len(), 1);
        assert_eq!(backups[0].original_content, original);

        let modified = fs::read_to_string(&grub).unwrap();
        assert!(modified.contains("acpi.ec_no_wakeup=1"));

        // Restore (skip grub-mkconfig since we're testing file manipulation)
        for backup in &backups {
            fs::write(&backup.path, &backup.original_content).unwrap();
        }
        let restored = fs::read_to_string(&grub).unwrap();
        assert_eq!(restored, original);
    }

    #[test]
    fn test_grub_preserves_surrounding_lines() {
        let tmp = TempDir::new().unwrap();
        let grub = tmp.path().join("grub");
        let content = "\
# Comment line
GRUB_DEFAULT=0
GRUB_TIMEOUT=5
GRUB_CMDLINE_LINUX_DEFAULT=\"quiet splash\"
GRUB_CMDLINE_LINUX=\"\"
";
        fs::write(&grub, content).unwrap();

        let params = vec!["acpi.ec_no_wakeup=1".to_string()];
        add_kernel_params_grub(&params, &grub).unwrap();

        let after = fs::read_to_string(&grub).unwrap();
        assert!(after.contains("# Comment line"));
        assert!(after.contains("GRUB_DEFAULT=0"));
        assert!(after.contains("GRUB_TIMEOUT=5"));
        assert!(after.contains("GRUB_CMDLINE_LINUX=\"\""));
        assert!(after.contains("acpi.ec_no_wakeup=1"));
    }

    #[test]
    fn test_grub_only_modifies_cmdline_default_not_cmdline_linux() {
        let tmp = TempDir::new().unwrap();
        let grub = tmp.path().join("grub");
        let content = "\
GRUB_CMDLINE_LINUX=\"crashkernel=auto\"
GRUB_CMDLINE_LINUX_DEFAULT=\"quiet\"
";
        fs::write(&grub, content).unwrap();

        let params = vec!["acpi.ec_no_wakeup=1".to_string()];
        add_kernel_params_grub(&params, &grub).unwrap();

        let after = fs::read_to_string(&grub).unwrap();
        // GRUB_CMDLINE_LINUX should be untouched
        assert!(after.contains("GRUB_CMDLINE_LINUX=\"crashkernel=auto\""));
        // Only GRUB_CMDLINE_LINUX_DEFAULT should be modified
        assert!(after.contains("GRUB_CMDLINE_LINUX_DEFAULT=\"quiet acpi.ec_no_wakeup=1\""));
    }
}
