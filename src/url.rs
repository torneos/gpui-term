//! Safe URL opening from terminal content.

/// Schemes that are safe to hand to the OS opener from terminal content.
const ALLOWED_SCHEMES: &[&str] = &[
    "http://",
    "https://",
    "file://",
    "ftp://",
    "ssh://",
    "mailto:",
];

/// Whether a string looks like a URL we are willing to open.
///
/// Besides requiring a known scheme, this rejects whitespace, control
/// characters and shell metacharacters. The latter is important because the
/// Windows opener routes through `cmd`, where characters like `&`, `|`, `^`
/// could otherwise inject commands when the URL originates from untrusted
/// terminal output.
pub fn is_openable_url(s: &str) -> bool {
    if !ALLOWED_SCHEMES.iter().any(|p| s.starts_with(p)) {
        return false;
    }
    if s.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return false;
    }
    !s.contains(['"', '\'', '&', '|', '<', '>', '^', '`', '$', '(', ')', ';'])
}

/// Open a URL in the system's default handler. No-op for unsafe input.
pub fn open_url(url: &str) {
    if !is_openable_url(url) {
        return;
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/c", "start", "", url])
            .spawn();
    }
}
