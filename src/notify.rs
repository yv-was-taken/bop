use anyhow::Result;

/// Send a desktop notification. Finds the active graphical session and
/// runs notify-send as that user with their D-Bus session address.
/// Fails silently if no graphical session is found.
pub fn send(title: &str, body: &str) -> Result<()> {
    // Find graphical sessions via loginctl
    let output = std::process::Command::new("loginctl")
        .args(["list-sessions", "--no-legend", "--no-pager"])
        .output()?;

    if !output.status.success() {
        return Ok(());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    for line in stdout.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 3 {
            continue;
        }
        let session_id = fields[0];
        let uid = fields[1];
        let user = fields[2];

        // Check if this session is graphical
        let session_type = std::process::Command::new("loginctl")
            .args(["show-session", session_id, "--property=Type", "--value"])
            .output();

        let session_type = match session_type {
            Ok(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
            Err(_) => continue,
        };

        if session_type != "wayland" && session_type != "x11" {
            continue;
        }

        // Get the session's runtime directory for D-Bus
        let runtime_dir = format!("/run/user/{}", uid);
        let dbus_addr = format!("unix:path={}/bus", runtime_dir);

        // Run notify-send as the session user
        let _ = std::process::Command::new("runuser")
            .args(["-u", user, "--", "notify-send", title, body])
            .env("DBUS_SESSION_BUS_ADDRESS", &dbus_addr)
            .status();

        return Ok(()); // Only notify the first graphical session
    }

    Ok(()) // No graphical session found, fail silently
}
