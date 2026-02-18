use crate::error::{Error, Result};
use std::path::Path;

/// Add kernel parameters to systemd-boot configuration.
/// Modifies /boot/loader/entries/*.conf files.
pub fn add_kernel_params(params: &[String]) -> Result<()> {
    let entries_dir = Path::new("/boot/loader/entries");
    if !entries_dir.exists() {
        return Err(Error::Bootloader(
            "systemd-boot entries directory not found at /boot/loader/entries".to_string(),
        ));
    }

    let entries: Vec<_> = std::fs::read_dir(entries_dir)
        .map_err(|e| Error::Bootloader(format!("failed to read entries dir: {}", e)))?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "conf"))
        .collect();

    if entries.is_empty() {
        return Err(Error::Bootloader(
            "no .conf files found in /boot/loader/entries".to_string(),
        ));
    }

    for entry in &entries {
        let path = entry.path();
        let content = std::fs::read_to_string(&path)
            .map_err(|e| Error::Bootloader(format!("failed to read {}: {}", path.display(), e)))?;

        let mut new_lines = Vec::new();
        let mut options_found = false;

        for line in content.lines() {
            if line.starts_with("options") {
                options_found = true;
                let mut options_line = line.to_string();

                for param in params {
                    let param_name = param.split('=').next().unwrap_or(param);
                    // Check if this param already exists
                    let already_set = options_line.split_whitespace().any(|word| {
                        word == *param || word.starts_with(&format!("{}=", param_name))
                    });

                    if !already_set {
                        options_line.push(' ');
                        options_line.push_str(param);
                    }
                }

                new_lines.push(options_line);
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

        let new_content = new_lines.join("\n");
        // Preserve trailing newline if original had one
        let new_content = if content.ends_with('\n') {
            format!("{}\n", new_content)
        } else {
            new_content
        };

        std::fs::write(&path, new_content)
            .map_err(|e| Error::Bootloader(format!("failed to write {}: {}", path.display(), e)))?;
    }

    Ok(())
}

/// Remove kernel parameters from systemd-boot configuration.
pub fn remove_kernel_params(params: &[String]) -> Result<()> {
    let entries_dir = Path::new("/boot/loader/entries");
    if !entries_dir.exists() {
        return Ok(()); // Nothing to undo
    }

    let entries: Vec<_> = std::fs::read_dir(entries_dir)
        .map_err(|e| Error::Bootloader(format!("failed to read entries dir: {}", e)))?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "conf"))
        .collect();

    for entry in &entries {
        let path = entry.path();
        let content = std::fs::read_to_string(&path)
            .map_err(|e| Error::Bootloader(format!("failed to read {}: {}", path.display(), e)))?;

        let param_names: Vec<&str> = params
            .iter()
            .map(|p| p.split('=').next().unwrap_or(p))
            .collect();

        let mut new_lines = Vec::new();

        for line in content.lines() {
            if line.starts_with("options") {
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
                new_lines.push(filtered.join(" "));
            } else {
                new_lines.push(line.to_string());
            }
        }

        let new_content = new_lines.join("\n");
        let new_content = if content.ends_with('\n') {
            format!("{}\n", new_content)
        } else {
            new_content
        };

        std::fs::write(&path, new_content)
            .map_err(|e| Error::Bootloader(format!("failed to write {}: {}", path.display(), e)))?;
    }

    Ok(())
}
