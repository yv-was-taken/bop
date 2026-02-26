use crate::audit::{Finding, Severity};
use crate::sysfs::SysfsRoot;

pub fn check(sysfs: &SysfsRoot) -> Vec<Finding> {
    let mut findings = Vec::new();

    // NMI watchdog
    if let Some(val) = sysfs
        .read_optional("proc/sys/kernel/nmi_watchdog")
        .unwrap_or(None)
        && val == "1"
    {
        findings.push(
            Finding::new(
                Severity::Medium,
                "Kernel",
                "NMI watchdog enabled - generates interrupts that prevent deep C-states",
            )
            .current("1")
            .recommended("0")
            .impact("~0.1-0.5W savings")
            .path("/proc/sys/kernel/nmi_watchdog")
            .weight(4),
        );
    }

    // Dirty writeback interval
    if let Some(val) = sysfs
        .read_optional("proc/sys/vm/dirty_writeback_centisecs")
        .unwrap_or(None)
        && val.parse::<u32>().unwrap_or(0) < 1500
    {
        findings.push(
            Finding::new(
                Severity::Low,
                "Kernel",
                "Disk writeback interval too frequent - wakes storage unnecessarily",
            )
            .current(&val)
            .recommended("1500")
            .impact("Reduces storage wakeups (minor savings on NVMe)")
            .path("/proc/sys/vm/dirty_writeback_centisecs")
            .weight(2),
        );
    }

    findings
}
