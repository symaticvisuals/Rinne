# Security Policy

Rinne runs locally and handles your API keys through the OS keychain. We take
key handling and any path that could leak secrets seriously.

## Reporting a vulnerability

If you find a security issue — for example key leakage, a path that writes
secrets to disk or logs, command/prompt injection through a worker, or unsafe
handling of model output — please report it **privately**:

- **Preferred:** GitHub → the repo's **Security** tab → **Report a vulnerability**
  (this opens a private GitHub Security Advisory).
- **Email:** security@example.com  <!-- TODO: replace with a real address, or remove this line to rely on GitHub advisories -->

Please **do not** open a public Issue or Discussion for a vulnerability.

### Do not include secrets

**Never paste API keys or tokens** in a report. Rinne redacts secrets in its own
transcript and prompt history, but pasted shell output may not be. Replace any
secret with `***`, and **rotate any key you may have exposed**.

### What to include

- A clear description of the issue and its impact
- Steps to reproduce (with secrets redacted)
- `rinne --version`, your OS, and how Rinne was installed
- Any relevant log excerpts from `.rinne/logs/` (scrubbed)

## What to expect

- We'll acknowledge your report within a reasonable window.
- We'll work with you to confirm, fix, and coordinate disclosure.
- With your permission, we'll credit you when the fix ships.

## Scope

Rinne is local-first software with no hosted component. In scope: the `rinne`
binary and its crates (`rinne-core`, `rinne-config`, `rinne-workers`,
`rinne-conductor`). Out of scope: vulnerabilities in the third-party worker CLIs
or model APIs Rinne talks to — report those to their respective projects.
