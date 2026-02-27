use crate::config::BrightnessConfig;
use crate::sysfs::SysfsRoot;
use anyhow::Result;

/// Find the first backlight device and return its sysfs base path (relative to sysfs root).
fn find_backlight(sysfs: &SysfsRoot) -> Option<String> {
    let entries = sysfs.list_dir("sys/class/backlight").ok()?;
    entries
        .into_iter()
        .next()
        .map(|name| format!("sys/class/backlight/{}", name))
}

/// Dim the backlight to `config.dim_percent`% of current brightness.
/// Returns the original brightness value for later restoration.
pub fn dim(config: &BrightnessConfig, sysfs: &SysfsRoot) -> Result<Option<u64>> {
    if !config.auto_dim {
        return Ok(None);
    }

    let base = match find_backlight(sysfs) {
        Some(b) => b,
        None => return Ok(None),
    };

    let current: u64 = sysfs
        .read_optional(format!("{}/brightness", base))
        .map_err(|e| anyhow::anyhow!("{}", e))?
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    if current == 0 {
        return Ok(None);
    }

    let max: u64 = sysfs
        .read_optional(format!("{}/max_brightness", base))
        .map_err(|e| anyhow::anyhow!("{}", e))?
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    if max == 0 {
        return Ok(None);
    }

    let target = current * u64::from(config.dim_percent) / 100;
    let target = target.max(1); // never go to 0

    if target >= current {
        return Ok(None); // already dim enough
    }

    sysfs
        .write(format!("{}/brightness", base), &target.to_string())
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    Ok(Some(current))
}

/// Restore brightness to a previously saved value.
pub fn restore(original: u64, sysfs: &SysfsRoot) -> Result<()> {
    let base = match find_backlight(sysfs) {
        Some(b) => b,
        None => return Ok(()),
    };

    sysfs
        .write(format!("{}/brightness", base), &original.to_string())
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_backlight(tmp: &TempDir, brightness: u64, max_brightness: u64) -> SysfsRoot {
        let bl_dir = tmp.path().join("sys/class/backlight/amdgpu_bl1");
        fs::create_dir_all(&bl_dir).unwrap();
        fs::write(bl_dir.join("brightness"), brightness.to_string()).unwrap();
        fs::write(bl_dir.join("max_brightness"), max_brightness.to_string()).unwrap();
        SysfsRoot::new(tmp.path())
    }

    #[test]
    fn test_dim_returns_none_when_auto_dim_disabled() {
        let tmp = TempDir::new().unwrap();
        let sysfs = setup_backlight(&tmp, 1000, 1000);
        let config = BrightnessConfig {
            auto_dim: false,
            dim_percent: 60,
        };

        let result = dim(&config, &sysfs).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_dim_returns_none_when_no_backlight() {
        let tmp = TempDir::new().unwrap();
        // Create the backlight directory but leave it empty (no devices)
        fs::create_dir_all(tmp.path().join("sys/class/backlight")).unwrap();
        let sysfs = SysfsRoot::new(tmp.path());
        let config = BrightnessConfig {
            auto_dim: true,
            dim_percent: 60,
        };

        let result = dim(&config, &sysfs).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_dim_returns_none_when_no_backlight_dir() {
        let tmp = TempDir::new().unwrap();
        let sysfs = SysfsRoot::new(tmp.path());
        let config = BrightnessConfig {
            auto_dim: true,
            dim_percent: 60,
        };

        let result = dim(&config, &sysfs).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_dim_calculates_correctly() {
        let tmp = TempDir::new().unwrap();
        let sysfs = setup_backlight(&tmp, 1000, 1000);
        let config = BrightnessConfig {
            auto_dim: true,
            dim_percent: 60,
        };

        let result = dim(&config, &sysfs).unwrap();
        assert_eq!(result, Some(1000));

        // Verify the brightness was written
        let written =
            fs::read_to_string(tmp.path().join("sys/class/backlight/amdgpu_bl1/brightness"))
                .unwrap();
        assert_eq!(written, "600");
    }

    #[test]
    fn test_dim_returns_none_when_target_ge_current() {
        let tmp = TempDir::new().unwrap();
        // Current brightness is already low (e.g., 50 out of 1000)
        let sysfs = setup_backlight(&tmp, 50, 1000);
        let config = BrightnessConfig {
            auto_dim: true,
            dim_percent: 100,
        };

        let result = dim(&config, &sysfs).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_dim_never_goes_to_zero() {
        let tmp = TempDir::new().unwrap();
        // Very low brightness, dim_percent would make it 0 but we clamp to 1
        let sysfs = setup_backlight(&tmp, 1, 1000);
        let config = BrightnessConfig {
            auto_dim: true,
            dim_percent: 1, // 1% of 1 = 0, should clamp to 1
        };

        // target = 1 * 1 / 100 = 0, clamped to 1, but 1 >= 1 so returns None
        let result = dim(&config, &sysfs).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_dim_returns_none_when_max_brightness_zero() {
        let tmp = TempDir::new().unwrap();
        let sysfs = setup_backlight(&tmp, 100, 0);
        let config = BrightnessConfig {
            auto_dim: true,
            dim_percent: 60,
        };

        let result = dim(&config, &sysfs).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_restore_writes_original_value() {
        let tmp = TempDir::new().unwrap();
        let sysfs = setup_backlight(&tmp, 600, 1000);

        restore(1000, &sysfs).unwrap();

        let written =
            fs::read_to_string(tmp.path().join("sys/class/backlight/amdgpu_bl1/brightness"))
                .unwrap();
        assert_eq!(written, "1000");
    }

    #[test]
    fn test_restore_no_backlight_is_ok() {
        let tmp = TempDir::new().unwrap();
        let sysfs = SysfsRoot::new(tmp.path());

        // Should not error when there's no backlight device
        let result = restore(1000, &sysfs);
        assert!(result.is_ok());
    }
}
