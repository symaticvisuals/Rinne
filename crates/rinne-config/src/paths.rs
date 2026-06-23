//! Per-OS configuration and blackboard paths (`CONTEXT.md` §12, §18).

use std::path::PathBuf;

use directories::ProjectDirs;

use rinne_core::BLACKBOARD_DIR;

/// Qualifier/org/app used to derive per-OS config locations.
const QUALIFIER: &str = "";
const ORGANIZATION: &str = "rinne";
const APPLICATION: &str = "rinne";

/// The global config file path, e.g. `~/.config/rinne/config.toml` on Linux or
/// the platform equivalent on macOS/Windows.
///
/// Returns `None` only when no home directory can be determined.
pub fn global_config_file() -> Option<PathBuf> {
    ProjectDirs::from(QUALIFIER, ORGANIZATION, APPLICATION)
        .map(|dirs| dirs.config_dir().join("config.toml"))
}

/// The per-project config file path: `<blackboard>/config.toml` under the
/// given project root.
pub fn project_config_file(project_root: &std::path::Path) -> PathBuf {
    project_root.join(BLACKBOARD_DIR).join("config.toml")
}

/// The cached `doctor` probe results path, under the global cache dir.
pub fn probe_cache_file() -> Option<PathBuf> {
    ProjectDirs::from(QUALIFIER, ORGANIZATION, APPLICATION)
        .map(|dirs| dirs.cache_dir().join("doctor-probe.json"))
}
