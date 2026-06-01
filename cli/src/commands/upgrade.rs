use anyhow::{bail, Context, Result};
use reqwest::Client;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::time::Duration;

use crate::output;

const REPO: &str = "okx/onchainos-skills";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(clap::Args)]
pub struct UpgradeArgs {
    /// Include pre-release (beta) versions
    #[arg(long)]
    pub beta: bool,

    /// Upgrade even if already on the latest version
    #[arg(long)]
    pub force: bool,

    /// Only check for a newer version, do not install
    #[arg(long)]
    pub check: bool,
}

// ── Version comparison ──────────────────────────────────────────────────

/// Returns true if `a` is strictly newer than `b` (semver, with pre-release support).
fn semver_gt(a: &str, b: &str) -> bool {
    fn parse(s: &str) -> (u32, u32, u32, Option<u32>) {
        let (base, pre) = match s.splitn(2, '-').collect::<Vec<_>>()[..] {
            [b, p] => (b, Some(p)),
            [b] => (b, None),
            _ => return (0, 0, 0, None),
        };
        let parts: Vec<u32> = base.split('.').map(|x| x.parse().unwrap_or(0)).collect();
        let pre_num = pre.and_then(|p| {
            p.chars()
                .filter(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse()
                .ok()
        });
        (
            parts.first().copied().unwrap_or(0),
            parts.get(1).copied().unwrap_or(0),
            parts.get(2).copied().unwrap_or(0),
            pre_num,
        )
    }

    let (maj_a, min_a, pat_a, pre_a) = parse(a);
    let (maj_b, min_b, pat_b, pre_b) = parse(b);

    if maj_a != maj_b {
        return maj_a > maj_b;
    }
    if min_a != min_b {
        return min_a > min_b;
    }
    if pat_a != pat_b {
        return pat_a > pat_b;
    }

    match (pre_a, pre_b) {
        (None, None) => false,           // equal
        (None, Some(_)) => true,         // stable > pre-release
        (Some(_), None) => false,        // pre-release < stable
        (Some(na), Some(nb)) => na > nb, // higher pre-release number wins
    }
}

// ── GitHub API ──────────────────────────────────────────────────────────
//
// Both version-lookup paths avoid api.github.com (60/hr unauthenticated limit)
// on the happy path: stable follows the /releases/latest redirect, beta lists
// tags via `git ls-remote`. The api.github.com fallback runs only when the
// primary path fails, and honors $GITHUB_TOKEN to raise the limit to 5000/hr.

/// Attach a bearer token from `$GITHUB_TOKEN` when present. Used only on the
/// fallback api.github.com path — the token is sent as a request header and
/// never appears in argv / logs. Raises the rate limit from 60/hr to 5000/hr.
fn with_github_token(req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    match std::env::var("GITHUB_TOKEN") {
        Ok(token) if !token.is_empty() => req.bearer_auth(token),
        _ => req,
    }
}

/// Parse a semver (without the leading `v`) from the final URL of a
/// `/releases/latest` redirect. Returns `None` for any URL that is not a
/// `/releases/tag/v<digit>…` page, forcing the caller to fall back to the API
/// rather than trusting a non-tag landing page.
fn parse_release_tag_url(url: &str) -> Option<String> {
    let after = url.split("/releases/tag/").nth(1)?;
    // Cut at the first path/query/fragment boundary, then require a v<digit> tag.
    let tag = after.split(['/', '?', '#']).next().unwrap_or(after).trim();
    let ver = tag.strip_prefix('v')?;
    if ver.chars().next()?.is_ascii_digit() {
        Some(ver.to_string())
    } else {
        None
    }
}

/// Parse the stdout of `git ls-remote --tags` into a deduped list of versions
/// (without the leading `v`). Peeled-tag refs (`^{}`) and non-`v<digit>` refs
/// (branches, lightweight non-semver tags) are dropped.
fn parse_ls_remote_versions(stdout: &str) -> Vec<String> {
    let mut versions: Vec<String> = Vec::new();
    for line in stdout.lines() {
        // Each line is "<sha>\trefs/tags/<tag>"; the ref may be peeled ("^{}").
        let Some(refname) = line.split('\t').nth(1) else {
            continue;
        };
        let tag = refname
            .rsplit('/')
            .next()
            .unwrap_or("")
            .trim_end_matches("^{}");
        let Some(ver) = tag.strip_prefix('v') else {
            continue;
        };
        if !ver.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            continue;
        }
        let ver = ver.to_string();
        if !versions.contains(&ver) {
            versions.push(ver);
        }
    }
    versions
}

/// Return the highest version by semver (pre-releases ranked below their base
/// version, per `semver_gt`), or `None` for an empty list.
fn highest_version(versions: &[String]) -> Option<String> {
    let mut best: Option<&str> = None;
    for v in versions {
        match best {
            None => best = Some(v),
            Some(b) if semver_gt(v, b) => best = Some(v),
            _ => {}
        }
    }
    best.map(str::to_string)
}

/// Primary stable lookup: follow the `/releases/latest` redirect (served by the
/// github.com website backend, NOT counted against the API limit) and read the
/// final URL. Returns `None` on any failure so the caller can fall back.
async fn latest_stable_via_redirect(client: &Client) -> Option<String> {
    let url = format!("https://github.com/{}/releases/latest", REPO);
    let resp = client
        .head(&url)
        .header("User-Agent", "onchainos-cli")
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    parse_release_tag_url(resp.url().as_str())
}

/// Primary beta lookup: list tags via `git ls-remote` (git smart-http, NOT
/// counted against the API limit). Returns an empty vec if git is unavailable
/// or the call fails, so the caller can fall back to the tags API.
fn ls_remote_tag_versions() -> Vec<String> {
    let url = format!("https://github.com/{}.git", REPO);
    // GIT_HTTP_LOW_SPEED_* aborts a stalled transfer (proxy/firewall) so the API
    // fallback can run; GIT_TERMINAL_PROMPT=0 prevents a hang on an auth prompt.
    let output = std::process::Command::new("git")
        .args(["ls-remote", "--tags", &url])
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_HTTP_LOW_SPEED_LIMIT", "1000")
        .env("GIT_HTTP_LOW_SPEED_TIME", "15")
        .output();
    match output {
        Ok(out) if out.status.success() => {
            parse_ls_remote_versions(&String::from_utf8_lossy(&out.stdout))
        }
        _ => Vec::new(),
    }
}

async fn get_latest_stable(client: &Client) -> Result<String> {
    if let Some(ver) = latest_stable_via_redirect(client).await {
        return Ok(ver);
    }

    // Fallback: api.github.com (honors GITHUB_TOKEN) — counts against the limit.
    let url = format!("https://api.github.com/repos/{}/releases/latest", REPO);
    let resp: Value = with_github_token(
        client
            .get(&url)
            .header("User-Agent", "onchainos-cli")
            .timeout(Duration::from_secs(10)),
    )
    .send()
    .await
    .context("failed to fetch latest release from GitHub")?
    .json()
    .await
    .context("failed to parse GitHub release response")?;

    resp["tag_name"]
        .as_str()
        .map(|t| t.trim_start_matches('v').to_string())
        .context("missing tag_name in GitHub release response")
}

async fn api_tag_versions(client: &Client) -> Result<Vec<String>> {
    let url = format!("https://api.github.com/repos/{}/tags?per_page=100", REPO);
    let resp: Value = with_github_token(
        client
            .get(&url)
            .header("User-Agent", "onchainos-cli")
            .timeout(Duration::from_secs(10)),
    )
    .send()
    .await
    .context("failed to fetch tags from GitHub")?
    .json()
    .await
    .context("failed to parse GitHub tags response")?;

    let tags = resp.as_array().context("expected array from tags API")?;
    let mut versions: Vec<String> = Vec::new();
    for tag in tags {
        let name = tag["name"]
            .as_str()
            .unwrap_or("")
            .trim_start_matches('v')
            .to_string();
        if !name.is_empty() {
            versions.push(name);
        }
    }
    Ok(versions)
}

async fn get_latest_with_beta(client: &Client) -> Result<String> {
    // Primary: git ls-remote (no API limit). Fallback: tags API.
    let mut versions = ls_remote_tag_versions();
    if versions.is_empty() {
        versions = api_tag_versions(client).await?;
    }
    highest_version(&versions).context("no valid versions found in GitHub tags")
}

// ── Platform detection ──────────────────────────────────────────────────

#[allow(unreachable_code)]
fn target_triple() -> Result<&'static str> {
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    return Ok("x86_64-apple-darwin");
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    return Ok("aarch64-apple-darwin");
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    return Ok("x86_64-unknown-linux-gnu");
    #[cfg(all(target_os = "linux", target_arch = "x86"))]
    return Ok("i686-unknown-linux-gnu");
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    return Ok("aarch64-unknown-linux-gnu");
    #[cfg(all(target_os = "linux", target_arch = "arm"))]
    return Ok("armv7-unknown-linux-gnueabihf");
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    return Ok("x86_64-pc-windows-msvc");
    bail!(
        "unsupported platform — please install manually from https://github.com/{}",
        REPO
    )
}

// ── Download + verify + install ─────────────────────────────────────────

async fn download_and_install(client: &Client, version: &str) -> Result<()> {
    let triple = target_triple()?;
    let binary_name = format!("onchainos-{}", triple);
    let tag = format!("v{}", version);

    let binary_url = format!(
        "https://github.com/{}/releases/download/{}/{}",
        REPO, tag, binary_name
    );
    let checksums_url = format!(
        "https://github.com/{}/releases/download/{}/checksums.txt",
        REPO, tag
    );

    eprintln!("Fetching checksums...");
    let checksums = client
        .get(&checksums_url)
        .header("User-Agent", "onchainos-cli")
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .context("failed to download checksums.txt")?
        .text()
        .await?;

    let expected_hash = checksums
        .lines()
        .find(|l| l.contains(&binary_name))
        .and_then(|l| l.split_whitespace().next())
        .context("checksum not found for this platform in checksums.txt")?
        .to_string();

    eprintln!("Downloading {} {}...", binary_name, tag);
    let bytes = client
        .get(&binary_url)
        .header("User-Agent", "onchainos-cli")
        .timeout(Duration::from_secs(120))
        .send()
        .await
        .context("failed to download binary")?
        .bytes()
        .await
        .context("failed to read binary bytes")?;

    // SHA-256 verification
    let actual_hash = hex::encode(Sha256::digest(&bytes));
    if actual_hash != expected_hash {
        bail!(
            "checksum mismatch — binary may have been tampered with\n  expected: {}\n  actual:   {}",
            expected_hash,
            actual_hash
        );
    }
    eprintln!("Checksum verified.");

    // Atomic replace: write to <exe>.tmp then rename
    let exe_path = std::env::current_exe().context("failed to resolve current executable path")?;
    // Follow symlinks to get the real binary path
    let exe_path = exe_path.canonicalize().unwrap_or(exe_path);
    let tmp_path = exe_path.with_extension("tmp");

    std::fs::write(&tmp_path, &bytes).context("failed to write temporary binary")?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o755))
            .context("failed to set executable permission")?;
    }

    std::fs::rename(&tmp_path, &exe_path).context("failed to replace binary")?;

    Ok(())
}

// ── Command entry point ─────────────────────────────────────────────────

pub async fn execute(args: UpgradeArgs) -> Result<()> {
    let client = Client::builder().timeout(Duration::from_secs(30)).build()?;

    let current = CURRENT_VERSION;

    let latest = if args.beta {
        get_latest_with_beta(&client).await?
    } else {
        get_latest_stable(&client).await?
    };

    if args.check {
        let update_available = semver_gt(&latest, current);
        output::success(json!({
            "currentVersion": current,
            "latestVersion": latest,
            "updateAvailable": update_available,
            "channel": if args.beta { "beta" } else { "stable" },
        }));
        return Ok(());
    }

    let needs_upgrade = args.force || semver_gt(&latest, current);

    if !needs_upgrade {
        output::success(json!({
            "currentVersion": current,
            "latestVersion": latest,
            "status": "already_latest",
        }));
        return Ok(());
    }

    eprintln!("Upgrading onchainos: {} → {}", current, latest);

    download_and_install(&client, &latest).await?;

    output::success(json!({
        "previousVersion": current,
        "installedVersion": latest,
        "status": "upgraded",
    }));

    Ok(())
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::{highest_version, parse_ls_remote_versions, parse_release_tag_url, semver_gt};

    #[test]
    fn stable_newer_than_older_stable() {
        assert!(semver_gt("2.1.0", "2.0.0"));
        assert!(semver_gt("3.0.0", "2.9.9"));
        assert!(!semver_gt("2.0.0", "2.1.0"));
    }

    #[test]
    fn stable_newer_than_same_base_prerelease() {
        assert!(semver_gt("2.0.0", "2.0.0-beta.5"));
        assert!(!semver_gt("2.0.0-beta.5", "2.0.0"));
    }

    #[test]
    fn higher_prerelease_number_wins() {
        assert!(semver_gt("2.0.0-beta.1", "2.0.0-beta.0"));
        assert!(!semver_gt("2.0.0-beta.0", "2.0.0-beta.1"));
    }

    #[test]
    fn equal_versions_not_gt() {
        assert!(!semver_gt("2.0.0", "2.0.0"));
        assert!(!semver_gt("2.0.0-beta.0", "2.0.0-beta.0"));
    }

    #[test]
    fn parses_version_from_release_tag_url() {
        assert_eq!(
            parse_release_tag_url("https://github.com/owner/repo/releases/tag/v2.2.12"),
            Some("2.2.12".to_string())
        );
        assert_eq!(
            parse_release_tag_url("https://github.com/owner/repo/releases/tag/v2.0.0-beta.3"),
            Some("2.0.0-beta.3".to_string())
        );
    }

    #[test]
    fn rejects_non_tag_release_url() {
        // A HEAD that fails to redirect leaves the bare /releases/latest URL —
        // must return None so the caller falls back to the API.
        assert_eq!(
            parse_release_tag_url("https://github.com/owner/repo/releases/latest"),
            None
        );
        assert_eq!(parse_release_tag_url("https://github.com/owner/repo"), None);
        // A tag page whose tag is not a v<digit> semver is rejected too.
        assert_eq!(
            parse_release_tag_url("https://github.com/owner/repo/releases/tag/nightly"),
            None
        );
    }

    #[test]
    fn parses_and_dedupes_ls_remote_output() {
        // Peeled refs (^{}) collapse to their tag; branches and non-v tags drop.
        let stdout = "abc123\trefs/tags/v2.2.10\n\
                      abc123\trefs/tags/v2.2.10^{}\n\
                      def456\trefs/tags/v2.0.0-beta.1\n\
                      000000\trefs/tags/not-a-version\n\
                      111111\trefs/heads/main\n";
        let mut versions = parse_ls_remote_versions(stdout);
        versions.sort();
        assert_eq!(
            versions,
            vec!["2.0.0-beta.1".to_string(), "2.2.10".to_string()]
        );
    }

    #[test]
    fn picks_highest_version() {
        let versions = vec![
            "2.0.0-beta.1".to_string(),
            "2.0.0".to_string(),
            "1.9.9".to_string(),
            "2.0.0-beta.5".to_string(),
        ];
        // Stable outranks its own pre-releases.
        assert_eq!(highest_version(&versions), Some("2.0.0".to_string()));
        let empty: Vec<String> = Vec::new();
        assert_eq!(highest_version(&empty), None);
    }
}
