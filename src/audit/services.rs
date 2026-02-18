use crate::audit::{Finding, Severity};

/// Conflicting services that should not run alongside bop optimizations.
const CONFLICTING_SERVICES: &[(&str, &str)] = &[
    (
        "tlp.service",
        "TLP conflicts with amd-pstate on AMD systems. Framework recommends NOT using TLP on AMD.",
    ),
    (
        "power-profiles-daemon.service",
        "power-profiles-daemon conflicts with direct platform_profile management.",
    ),
    (
        "thermald.service",
        "thermald is Intel-specific and can conflict with AMD thermal management.",
    ),
];

/// Services to note but not recommend disabling.
const NOTABLE_SERVICES: &[(&str, &str)] = &[
    ("docker.service", "Docker daemon (~0.2W idle). Development tool -- not recommending disable."),
    (
        "containerd.service",
        "Container runtime (~0.1W idle). Often needed for development.",
    ),
];

pub fn check() -> Vec<Finding> {
    let mut findings = Vec::new();

    for (service, reason) in CONFLICTING_SERVICES {
        if is_service_active(service) {
            findings.push(
                Finding::new(
                    Severity::High,
                    "Services",
                    format!("{} is active - {}", service, reason),
                )
                .current("active (running)")
                .recommended("disable and stop")
                .impact("Actively harmful to power optimization")
                .weight(8),
            );
        } else if is_service_enabled(service) {
            findings.push(
                Finding::new(
                    Severity::Medium,
                    "Services",
                    format!("{} is enabled - {}", service, reason),
                )
                .current("enabled (not running)")
                .recommended("disable")
                .impact("Will interfere on next boot")
                .weight(5),
            );
        }
    }

    for (service, note) in NOTABLE_SERVICES {
        if is_service_active(service) {
            findings.push(
                Finding::new(
                    Severity::Info,
                    "Services",
                    format!("{} is running", service),
                )
                .current("active")
                .recommended(note.to_string())
                .impact("Minor power impact")
                .weight(0), // Info only, doesn't affect score
            );
        }
    }

    findings
}

fn is_service_active(service: &str) -> bool {
    std::process::Command::new("systemctl")
        .args(["is-active", "--quiet", service])
        .status()
        .is_ok_and(|s| s.success())
}

fn is_service_enabled(service: &str) -> bool {
    std::process::Command::new("systemctl")
        .args(["is-enabled", "--quiet", service])
        .status()
        .is_ok_and(|s| s.success())
}
