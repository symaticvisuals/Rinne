//! Caching of `doctor` installation detection so the expensive PATH lookup and
//! smoke tests need not re-run on every invocation (`CONTEXT.md` §18).
//!
//! Only installation status is cached. Auth mode, the Claude footgun, and
//! API-key presence are derived from the live environment on every run, since
//! they can change between invocations without any reinstall.

use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use rinne_core::{Result, RinneError};

use crate::paths;
use crate::probe::InstallMap;

/// How long a cached installation probe is considered fresh.
const CACHE_TTL: Duration = Duration::from_secs(60 * 60 * 24);

#[derive(Debug, Serialize, Deserialize)]
struct CachedInstalls {
    /// Unix seconds when the probe ran.
    probed_at: u64,
    installs: InstallMap,
}

/// Load cached installation statuses if present and still fresh.
pub fn load_fresh() -> Option<InstallMap> {
    let path = paths::probe_cache_file()?;
    let bytes = std::fs::read(&path).ok()?;
    let cached: CachedInstalls = serde_json::from_slice(&bytes).ok()?;

    let age = now_secs().checked_sub(cached.probed_at)?;
    if Duration::from_secs(age) <= CACHE_TTL {
        Some(cached.installs)
    } else {
        None
    }
}

/// Persist installation statuses to the cache (best-effort dir creation).
pub fn store(installs: &InstallMap) -> Result<()> {
    let path = paths::probe_cache_file()
        .ok_or_else(|| RinneError::Config("no cache directory available".into()))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let cached = CachedInstalls {
        probed_at: now_secs(),
        installs: installs.clone(),
    };
    let bytes = serde_json::to_vec_pretty(&cached)?;
    std::fs::write(&path, bytes)?;
    Ok(())
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
