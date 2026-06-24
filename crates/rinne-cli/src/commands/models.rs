//! `rinne models <provider>` — list the models an API provider's key can
//! access, with pricing/context where the platform reports it (`CONTEXT.md` §7).
//! Local-only network call to the provider's `/v1/models`.

use anyhow::{anyhow, Result};

/// List models for a configured API provider.
pub async fn run(provider: &str) -> Result<()> {
    for line in list_lines(provider).await {
        println!("{line}");
    }
    Ok(())
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
