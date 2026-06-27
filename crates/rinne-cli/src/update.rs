//! Startup new-release notification.
//!
//! Best-effort and non-blocking: queries the cached GitHub release check and,
//! if a newer version exists, prints a one-line banner to stderr. Suppressed in
//! headless (`-p`), `--json`, and non-TTY contexts, and when the user has
//! disabled it via config or `RINNE_NO_UPDATE_CHECK`.

use std::io::IsTerminal;

use rinne_config::model::Config;

/// Print an "update available" banner if appropriate. Never errors; a failed or
/// disabled check simply prints nothing.
pub async fn notify(config: &Config) {
    if !config.update.check {
        return;
    }
    // Only nag in an interactive terminal; scripts and pipes stay quiet.
    if !std::io::stderr().is_terminal() {
        return;
    }

    let current = env!("CARGO_PKG_VERSION");
    if let Some(update) = rinne_config::update::check(current).await {
        eprintln!(
            "\n\x1b[1;33mA new release of rinne is available:\x1b[0m {} → \x1b[1;32m{}\x1b[0m",
            update.current, update.latest
        );
        eprintln!(
            "Upgrade: curl -fsSL https://raw.githubusercontent.com/GIKSN-RESEARCH/Rinne/main/install.sh | sh\n"
        );
    }
}
