use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Preset {
    Off,
    Default,
    Moderate,
    Saver,
    Supersaver,
}

impl std::fmt::Display for Preset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Preset::Off => write!(f, "off"),
            Preset::Default => write!(f, "default"),
            Preset::Moderate => write!(f, "moderate"),
            Preset::Saver => write!(f, "saver"),
            Preset::Supersaver => write!(f, "supersaver"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformProfilePolicy {
    NoChange,
    FixPerformance,
    ForceLowPower,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsbPolicy {
    NoChange,
    SkipInputExpansion,
    All,
}

#[derive(Debug, Clone)]
pub struct PresetKnobs {
    pub epp: Option<Cow<'static, str>>,
    pub platform_profile: PlatformProfilePolicy,
    pub aspm_policy: Option<Cow<'static, str>>,
    pub pci_runtime_pm: bool,
    pub usb_autosuspend: UsbPolicy,
    pub turbo_boost: Option<bool>,
    pub audio_power_save: bool,
    pub nmi_watchdog_disable: bool,
    pub dirty_writeback: Option<u32>,
    pub kernel_params: bool,
    pub acpi_wakeup_filter: bool,
    pub gpu_dpm: bool,
    /// Set by clamp_for_reduced() or resolve_knobs() when EPP was explicitly
    /// set (override or clamp). Prevents adaptive resolution from overriding
    /// the value, and allows writing EPP even when current is "power".
    pub epp_locked: bool,
}

impl PresetKnobs {
    /// Clamp knobs to at most Moderate-level safety for reduced inhibitor scope.
    /// Dials back aggressive volatile writes (turbo disable, powersupersave ASPM,
    /// full USB autosuspend) that could degrade performance during protected workloads.
    pub fn clamp_for_reduced(&mut self) {
        let moderate = Preset::Moderate.knobs();

        // Turbo: never disable in reduced mode
        if self.turbo_boost == Some(false) {
            self.turbo_boost = moderate.turbo_boost; // None
        }

        // ASPM: cap at powersave (not powersupersave)
        if self.aspm_policy.as_deref() == Some("powersupersave") {
            self.aspm_policy = moderate.aspm_policy.clone();
        }

        // USB: cap at SkipInputExpansion (not All)
        if self.usb_autosuspend == UsbPolicy::All {
            self.usb_autosuspend = moderate.usb_autosuspend;
        }

        // Platform profile: cap at FixPerformance (not ForceLowPower)
        if self.platform_profile == PlatformProfilePolicy::ForceLowPower {
            self.platform_profile = moderate.platform_profile;
        }

        // EPP: cap at balance_power (not power), and lock to prevent
        // adaptive resolution from escalating back to "power"
        if self.epp.as_deref() == Some("power") {
            self.epp = moderate.epp.clone();
        }
        if self.epp.is_some() {
            self.epp_locked = true;
        }
    }

    /// Returns true if any knob would cause changes to the system.
    pub fn has_any_active(&self) -> bool {
        self.epp.is_some()
            || self.platform_profile != PlatformProfilePolicy::NoChange
            || self.aspm_policy.is_some()
            || self.pci_runtime_pm
            || self.usb_autosuspend != UsbPolicy::NoChange
            || self.turbo_boost.is_some()
            || self.audio_power_save
            || self.nmi_watchdog_disable
            || self.dirty_writeback.is_some()
            || self.kernel_params
            || self.acpi_wakeup_filter
            || self.gpu_dpm
    }
}

impl Preset {
    pub fn knobs(&self) -> PresetKnobs {
        match self {
            Preset::Off => PresetKnobs {
                epp: None,
                platform_profile: PlatformProfilePolicy::NoChange,
                aspm_policy: None,
                pci_runtime_pm: false,
                usb_autosuspend: UsbPolicy::NoChange,
                turbo_boost: None,
                audio_power_save: false,
                nmi_watchdog_disable: false,
                dirty_writeback: None,
                kernel_params: false,
                acpi_wakeup_filter: false,
                gpu_dpm: false,
                epp_locked: false,
            },
            Preset::Default => PresetKnobs {
                epp: None,
                platform_profile: PlatformProfilePolicy::NoChange,
                aspm_policy: None,
                pci_runtime_pm: false,
                usb_autosuspend: UsbPolicy::NoChange,
                turbo_boost: None,
                audio_power_save: true,
                nmi_watchdog_disable: true,
                dirty_writeback: Some(1500),
                kernel_params: true,
                acpi_wakeup_filter: true,
                gpu_dpm: false,
                epp_locked: false,
            },
            Preset::Moderate => PresetKnobs {
                epp: Some(Cow::Borrowed("balance_power")),
                platform_profile: PlatformProfilePolicy::FixPerformance,
                aspm_policy: Some(Cow::Borrowed("powersave")),
                pci_runtime_pm: true,
                usb_autosuspend: UsbPolicy::SkipInputExpansion,
                turbo_boost: None,
                audio_power_save: true,
                nmi_watchdog_disable: true,
                dirty_writeback: Some(1500),
                kernel_params: true,
                acpi_wakeup_filter: true,
                gpu_dpm: true,
                epp_locked: false,
            },
            Preset::Saver => PresetKnobs {
                epp: Some(Cow::Borrowed("balance_power")),
                platform_profile: PlatformProfilePolicy::ForceLowPower,
                aspm_policy: Some(Cow::Borrowed("powersave")),
                pci_runtime_pm: true,
                usb_autosuspend: UsbPolicy::SkipInputExpansion,
                turbo_boost: None,
                audio_power_save: true,
                nmi_watchdog_disable: true,
                dirty_writeback: Some(1500),
                kernel_params: true,
                acpi_wakeup_filter: true,
                gpu_dpm: true,
                epp_locked: false,
            },
            Preset::Supersaver => PresetKnobs {
                epp: Some(Cow::Borrowed("power")),
                platform_profile: PlatformProfilePolicy::ForceLowPower,
                aspm_policy: Some(Cow::Borrowed("powersupersave")),
                pci_runtime_pm: true,
                usb_autosuspend: UsbPolicy::All,
                turbo_boost: Some(false),
                audio_power_save: true,
                nmi_watchdog_disable: true,
                dirty_writeback: Some(1500),
                kernel_params: true,
                acpi_wakeup_filter: true,
                gpu_dpm: true,
                epp_locked: false,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_preset_ordering() {
        assert!(Preset::Off < Preset::Default);
        assert!(Preset::Default < Preset::Moderate);
        assert!(Preset::Moderate < Preset::Saver);
        assert!(Preset::Saver < Preset::Supersaver);
    }

    #[test]
    fn test_preset_knobs_off() {
        let k = Preset::Off.knobs();
        assert_eq!(k.epp, None);
        assert_eq!(k.platform_profile, PlatformProfilePolicy::NoChange);
        assert_eq!(k.aspm_policy, None);
        assert!(!k.pci_runtime_pm);
        assert_eq!(k.usb_autosuspend, UsbPolicy::NoChange);
        assert_eq!(k.turbo_boost, None);
        assert!(!k.audio_power_save);
        assert!(!k.nmi_watchdog_disable);
        assert_eq!(k.dirty_writeback, None);
        assert!(!k.kernel_params);
        assert!(!k.acpi_wakeup_filter);
        assert!(!k.gpu_dpm);
    }

    #[test]
    fn test_preset_knobs_default() {
        let k = Preset::Default.knobs();
        assert_eq!(k.epp, None);
        assert!(k.audio_power_save);
        assert!(k.nmi_watchdog_disable);
        assert_eq!(k.dirty_writeback, Some(1500));
        assert!(k.kernel_params);
        assert!(k.acpi_wakeup_filter);
        assert!(!k.pci_runtime_pm);
        assert!(!k.gpu_dpm);
    }

    #[test]
    fn test_preset_knobs_moderate() {
        let k = Preset::Moderate.knobs();
        assert_eq!(k.epp.as_deref(), Some("balance_power"));
        assert_eq!(k.aspm_policy.as_deref(), Some("powersave"));
        assert!(k.pci_runtime_pm);
        assert_eq!(k.usb_autosuspend, UsbPolicy::SkipInputExpansion);
        assert_eq!(k.turbo_boost, None);
        assert!(k.gpu_dpm);
    }

    #[test]
    fn test_preset_knobs_supersaver() {
        let k = Preset::Supersaver.knobs();
        assert_eq!(k.epp.as_deref(), Some("power"));
        assert_eq!(k.aspm_policy.as_deref(), Some("powersupersave"));
        assert_eq!(k.turbo_boost, Some(false));
        assert_eq!(k.usb_autosuspend, UsbPolicy::All);
        assert_eq!(k.platform_profile, PlatformProfilePolicy::ForceLowPower);
    }

    #[test]
    fn test_clamp_for_reduced_caps_supersaver() {
        let mut k = Preset::Supersaver.knobs();
        k.clamp_for_reduced();
        // Aggressive knobs should be clamped to moderate level
        assert_eq!(k.turbo_boost, None);
        assert_eq!(k.aspm_policy.as_deref(), Some("powersave"));
        assert_eq!(k.usb_autosuspend, UsbPolicy::SkipInputExpansion);
        assert_eq!(k.platform_profile, PlatformProfilePolicy::FixPerformance);
        assert_eq!(k.epp.as_deref(), Some("balance_power"));
        // Non-aggressive knobs should be unchanged
        assert!(k.pci_runtime_pm);
        assert!(k.audio_power_save);
        assert!(k.gpu_dpm);
    }

    #[test]
    fn test_clamp_for_reduced_preserves_moderate() {
        let mut k = Preset::Moderate.knobs();
        let original = Preset::Moderate.knobs();
        k.clamp_for_reduced();
        // Moderate knobs should be unchanged
        assert_eq!(k.turbo_boost, original.turbo_boost);
        assert_eq!(k.aspm_policy.as_deref(), original.aspm_policy.as_deref());
        assert_eq!(k.usb_autosuspend, original.usb_autosuspend);
        assert_eq!(k.platform_profile, original.platform_profile);
        assert_eq!(k.epp.as_deref(), original.epp.as_deref());
    }

    #[test]
    fn test_preset_display() {
        assert_eq!(Preset::Off.to_string(), "off");
        assert_eq!(Preset::Default.to_string(), "default");
        assert_eq!(Preset::Moderate.to_string(), "moderate");
        assert_eq!(Preset::Saver.to_string(), "saver");
        assert_eq!(Preset::Supersaver.to_string(), "supersaver");
    }

    #[test]
    fn test_preset_serde_roundtrip() {
        let preset = Preset::Moderate;
        let json = serde_json::to_string(&preset).unwrap();
        assert_eq!(json, "\"moderate\"");
        let deserialized: Preset = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, preset);
    }
}
