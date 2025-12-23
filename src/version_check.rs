use anyhow::{Context, Result};
use serde::Deserialize;
use std::time::Duration;

const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const GITHUB_API_URL: &str = "https://api.github.com/repos/isomerc/nicotine/releases/latest";
const TIMEOUT_SECS: u64 = 5;

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    html_url: String,
}

/// Checks GitHub for a newer release version
/// Returns Ok(Some((new_version, url))) if an update is available
/// Returns Ok(None) if current version is up to date or on error
pub fn check_for_updates() -> Result<Option<(String, String)>> {
    // Build HTTP client with timeout
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(TIMEOUT_SECS))
        .user_agent("nicotine") // GitHub API requires a user agent
        .build()
        .context("Failed to build HTTP client")?;

    // Fetch latest release info from GitHub
    let response = client
        .get(GITHUB_API_URL)
        .send()
        .context("Failed to fetch latest release from GitHub")?;

    if !response.status().is_success() {
        return Ok(None); // Silently fail on HTTP errors
    }

    let release: GithubRelease = response
        .json()
        .context("Failed to parse GitHub API response")?;

    // Extract version from tag (e.g., "v0.2.1" -> "0.2.1")
    let latest_version = release.tag_name.trim_start_matches('v');

    // Compare versions
    if is_newer_version(latest_version, CURRENT_VERSION)? {
        Ok(Some((latest_version.to_string(), release.html_url)))
    } else {
        Ok(None)
    }
}

/// Compares two semantic versions (e.g., "0.2.1" vs "0.2.0")
/// Returns true if `latest` is newer than `current`
fn is_newer_version(latest: &str, current: &str) -> Result<bool> {
    let latest_parts = parse_version(latest)?;
    let current_parts = parse_version(current)?;

    Ok(latest_parts > current_parts)
}

/// Parses a version string like "0.2.1" into (major, minor, patch)
fn parse_version(version: &str) -> Result<(u32, u32, u32)> {
    let parts: Vec<&str> = version.split('.').collect();

    if parts.len() != 3 {
        anyhow::bail!("Invalid version format: {}", version);
    }

    let major = parts[0]
        .parse::<u32>()
        .context("Failed to parse major version")?;
    let minor = parts[1]
        .parse::<u32>()
        .context("Failed to parse minor version")?;
    let patch = parts[2]
        .parse::<u32>()
        .context("Failed to parse patch version")?;

    Ok((major, minor, patch))
}

/// Prints an update notification to the user
pub fn print_update_notification(new_version: &str, url: &str) {
    println!();
    println!("─────────────────────────────────────────────────────────────────────────");
    println!();
    println!("🚬 A new version of nicotine is available!");
    println!();
    println!("Current version: {}", CURRENT_VERSION);
    println!("Latest version:  {}", new_version);
    println!();
    println!("Update with:");
    println!("curl -sSL https://raw.githubusercontent.com/isomerc/nicotine/refs/heads/master/install-github.sh | bash");
    println!();
    println!("Release notes: {}", url);
    println!();
    println!("─────────────────────────────────────────────────────────────────────────");
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_version() {
        assert_eq!(parse_version("0.2.1").unwrap(), (0, 2, 1));
        assert_eq!(parse_version("1.0.0").unwrap(), (1, 0, 0));
        assert_eq!(parse_version("10.20.30").unwrap(), (10, 20, 30));
    }

    #[test]
    fn test_is_newer_version() {
        assert!(is_newer_version("0.2.2", "0.2.1").unwrap());
        assert!(is_newer_version("0.3.0", "0.2.9").unwrap());
        assert!(is_newer_version("1.0.0", "0.9.9").unwrap());

        assert!(!is_newer_version("0.2.1", "0.2.1").unwrap());
        assert!(!is_newer_version("0.2.0", "0.2.1").unwrap());
        assert!(!is_newer_version("0.1.9", "0.2.0").unwrap());
    }
}
