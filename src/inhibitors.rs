use crate::config::InhibitorMode;
use anyhow::Result;

#[derive(Debug, Clone)]
pub struct Inhibitor {
    pub who: String,
    pub why: String,
    pub what: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyScope {
    /// Apply all optimizations normally.
    Full,
    /// Only apply safe subset (sysfs writes, no service changes, no kernel params).
    Reduced,
    /// Skip all optimizations.
    Skip,
}

/// Check for active systemd inhibitors by parsing `systemd-inhibit --list`.
pub fn check_inhibitors() -> Result<Vec<Inhibitor>> {
    let output = std::process::Command::new("systemd-inhibit")
        .args(["--list", "--no-pager", "--no-legend"])
        .output()?;

    if !output.status.success() {
        return Ok(Vec::new()); // fail open -- no inhibitors detected
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut inhibitors = Vec::new();

    for line in stdout.lines() {
        let fields: Vec<&str> = line
            .splitn(6, char::is_whitespace)
            .filter(|s| !s.is_empty())
            .collect();
        // Fields: Who, UID, PID, What, Why, Mode (columns vary)
        // Simpler: just capture that inhibitors exist
        if fields.len() >= 4 {
            inhibitors.push(Inhibitor {
                who: fields[0].to_string(),
                what: fields.get(3).unwrap_or(&"").to_string(),
                why: fields.get(4).map_or(String::new(), |s| s.to_string()),
            });
        }
    }

    Ok(inhibitors)
}

/// Determine the apply scope based on inhibitor mode and active inhibitors.
pub fn should_apply(mode: &InhibitorMode, inhibitors: &[Inhibitor]) -> ApplyScope {
    if inhibitors.is_empty() {
        return ApplyScope::Full;
    }

    match mode {
        InhibitorMode::Skip => ApplyScope::Skip,
        InhibitorMode::Reduced => ApplyScope::Reduced,
        InhibitorMode::Full => ApplyScope::Full,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_inhibitor() -> Inhibitor {
        Inhibitor {
            who: "test".to_string(),
            what: "sleep".to_string(),
            why: "testing".to_string(),
        }
    }

    #[test]
    fn test_should_apply_empty_inhibitors_returns_full() {
        let scope = should_apply(&InhibitorMode::Skip, &[]);
        assert_eq!(scope, ApplyScope::Full);
    }

    #[test]
    fn test_should_apply_skip_mode_with_inhibitors() {
        let scope = should_apply(&InhibitorMode::Skip, &[make_inhibitor()]);
        assert_eq!(scope, ApplyScope::Skip);
    }

    #[test]
    fn test_should_apply_reduced_mode_with_inhibitors() {
        let scope = should_apply(&InhibitorMode::Reduced, &[make_inhibitor()]);
        assert_eq!(scope, ApplyScope::Reduced);
    }

    #[test]
    fn test_should_apply_full_mode_with_inhibitors() {
        let scope = should_apply(&InhibitorMode::Full, &[make_inhibitor()]);
        assert_eq!(scope, ApplyScope::Full);
    }
}
