//! `rinne models [provider]` — with a provider, list the models its key can
//! access (live `/v1/models` call, with pricing/context where reported). With no
//! provider, list every available worker and its model ladder, mirroring the
//! startup intro (`CONTEXT.md` §7).

use anyhow::{anyhow, Result};

/// List models. With a provider, the provider's live catalog; otherwise the full
/// worker/ladder overview (same data as the intro).
pub async fn run(provider: Option<&str>) -> Result<()> {
    let lines = match provider {
        Some(p) => list_lines(p).await,
        None => overview_lines().await,
    };
    for line in lines {
        println!("{line}");
    }
    Ok(())
}

/// A text overview of every available worker and its model ladder — the same
/// data the startup intro shows, formatted for the `/models` (no-arg) command.
pub async fn overview_lines() -> Vec<String> {
    let config = match rinne_config::load_cwd() {
        Ok(c) => c,
        Err(e) => return vec![format!("config error: {e}")],
    };
    let (registry, names) = match crate::runner::build_registry(&config).await {
        Ok(r) => r,
        Err(e) => return vec![format!("could not probe workers: {e}")],
    };
    if names.is_empty() {
        return vec![
            "no workers available — `rinne doctor` to see why, or `/connect` to add one.".to_string(),
        ];
    }
    let ladders = rinne_core::pool::profile(&registry.descriptors()).ladders();
    let mut out = vec![format!("{} worker(s) available:", names.len())];
    for name in &names {
        let detail = match ladders.get(name) {
            Some(l) if l.len() > 1 => l.join(" · "),
            _ => "default model".to_string(),
        };
        out.push(format!("  ✔ {name:<14} {detail}"));
    }
    out.push(format!(
        "conductor: {} · {}",
        format!("{:?}", config.conductor.backend).to_lowercase(),
        config.conductor.model
    ));
    out.push("`/models <provider>` for a provider's full catalog with pricing.".to_string());
    out
}

/// Fetch and format the model list for a provider (shared with the TUI).
pub async fn list_lines(provider: &str) -> Vec<String> {
    let config = match rinne_config::load_cwd() {
        Ok(c) => c,
        Err(e) => return vec![format!("config error: {e}")],
    };
    let Some(p) = config.backends.api.providers.get(provider) else {
        return vec![format!(
            "`{provider}` is not configured — `rinne connect {provider} <key> --base-url <url>` first."
        )];
    };
    let Some(base) = p.base_url.clone() else {
        return vec![format!("`{provider}` has no base_url set in config.")];
    };
    let Some(key) = rinne_config::secrets::resolve_api_key(provider, &p.key_env) else {
        return vec![format!(
            "no key for `{provider}` — `rinne connect {provider} <key>` or export {}.",
            p.key_env
        )];
    };

    match fetch(&base, &key).await {
        Ok(models) if models.is_empty() => vec![format!("`{provider}` returned no models.")],
        Ok(models) => {
            let mut out = vec![format!(
                "{} model(s) on `{provider}` (cheapest first):",
                models.len()
            )];
            for m in models.iter().take(40) {
                let price = m
                    .prompt_price
                    .map(|p| format!("${:.2}/M tok", p * 1_000_000.0))
                    .unwrap_or_else(|| "price n/a".into());
                let ctx = m
                    .context
                    .map(|c| format!("{}k ctx", c / 1000))
                    .unwrap_or_default();
                out.push(format!("  {:<48} {:<14} {}", m.id, price, ctx));
            }
            if models.len() > 40 {
                out.push(format!("  … +{} more", models.len() - 40));
            }
            out.push(format!(
                "set the ones you want: `rinne connect {provider} --model <id> --model <id>` (cheap→strong)"
            ));
            out
        }
        Err(e) => vec![format!("could not list models for `{provider}`: {e}")],
    }
}

async fn fetch(
    base: &str,
    key: &str,
) -> Result<Vec<rinne_workers::transport::http::DiscoveredModel>> {
    let client = rinne_workers::transport::http::OpenAiClient::new(base, Some(key.to_string()));
    client.list_models().await.map_err(|e| anyhow!(e.to_string()))
}
