use std::io::Read;
use std::time::Duration;

use anyhow::{Context, Result};
use semver::Version;
use serde_json::Value;

use super::version::CURRENT_VERSION;
use crate::render::sanitize_terminal_text;

pub const GITHUB_REPO: &str = "luntta/parc";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const READ_TIMEOUT: Duration = Duration::from_secs(10);
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_RELEASE_RESPONSE_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReleaseInfo {
    tag_name: String,
    version: String,
    html_url: String,
    assets: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UpdateInfo {
    current_version: String,
    latest_version: String,
    latest_tag: String,
    update_available: bool,
    release_url: String,
    assets: Vec<String>,
}

pub fn run_check(json: bool) -> Result<()> {
    let Some(latest) = fetch_latest_release()? else {
        if json {
            print_no_release_json()?;
        } else {
            print_no_release_human();
        }
        return Ok(());
    };

    let info = build_update_info(CURRENT_VERSION, latest)?;
    if json {
        print_update_json(&info)?;
    } else {
        print_update_human(&info);
    }

    Ok(())
}

fn fetch_latest_release() -> Result<Option<ReleaseInfo>> {
    let url = format!("https://api.github.com/repos/{GITHUB_REPO}/releases/latest");
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(CONNECT_TIMEOUT)
        .timeout_read(READ_TIMEOUT)
        .timeout_write(WRITE_TIMEOUT)
        .build();
    let response = match agent
        .get(&url)
        .set("Accept", "application/vnd.github+json")
        .set("User-Agent", &format!("parc/{CURRENT_VERSION}"))
        .call()
    {
        Ok(response) => response,
        Err(ureq::Error::Status(404, _)) => return Ok(None),
        Err(err) => return Err(anyhow::anyhow!("failed to query latest release: {}", err)),
    };

    let value = read_limited_json_response(response)?;
    parse_release(&value).map(Some)
}

fn read_limited_json_response(response: ureq::Response) -> Result<Value> {
    let mut body = String::new();
    let mut reader = response
        .into_reader()
        .take((MAX_RELEASE_RESPONSE_BYTES + 1) as u64);
    reader
        .read_to_string(&mut body)
        .context("failed to read GitHub release response")?;

    if body.len() > MAX_RELEASE_RESPONSE_BYTES {
        anyhow::bail!(
            "GitHub release response exceeded {} bytes",
            MAX_RELEASE_RESPONSE_BYTES
        );
    }

    serde_json::from_str(&body).context("failed to parse GitHub release response")
}

fn parse_release(value: &Value) -> Result<ReleaseInfo> {
    let tag_name = value
        .get("tag_name")
        .and_then(Value::as_str)
        .context("GitHub release response is missing tag_name")?
        .to_string();
    let version = normalize_version(&tag_name)
        .with_context(|| {
            format!(
                "latest release tag '{}' is not a semantic version",
                sanitize_for_terminal(&tag_name)
            )
        })?
        .to_string();
    let html_url = value
        .get("html_url")
        .and_then(Value::as_str)
        .unwrap_or("https://github.com/luntta/parc/releases/latest")
        .to_string();
    let assets = value
        .get("assets")
        .and_then(Value::as_array)
        .map(|assets| {
            assets
                .iter()
                .filter_map(|asset| asset.get("name").and_then(Value::as_str))
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default();

    Ok(ReleaseInfo {
        tag_name,
        version,
        html_url,
        assets,
    })
}

fn build_update_info(current: &str, latest: ReleaseInfo) -> Result<UpdateInfo> {
    let current_version = normalize_version(current)
        .with_context(|| format!("current version '{}' is not a semantic version", current))?;
    let latest_version = Version::parse(&latest.version).with_context(|| {
        format!(
            "latest version '{}' is not a semantic version",
            latest.version
        )
    })?;
    let update_available = latest_version > current_version;

    Ok(UpdateInfo {
        current_version: current_version.to_string(),
        latest_version: latest_version.to_string(),
        latest_tag: latest.tag_name,
        update_available,
        release_url: latest.html_url,
        assets: latest.assets,
    })
}

fn normalize_version(input: &str) -> Option<Version> {
    let mut candidate = input.trim();
    if let Some(stripped) = candidate.strip_prefix("parc-") {
        candidate = stripped;
    }
    candidate = candidate.trim_start_matches('v');
    Version::parse(candidate).ok()
}

fn print_update_json(info: &UpdateInfo) -> Result<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "current_version": info.current_version,
            "latest_version": info.latest_version,
            "latest_tag": info.latest_tag,
            "update_available": info.update_available,
            "release_url": info.release_url,
            "assets": info.assets,
        }))?
    );
    Ok(())
}

fn print_no_release_json() -> Result<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "current_version": CURRENT_VERSION,
            "latest_version": null,
            "latest_tag": null,
            "update_available": false,
            "release_url": null,
            "assets": [],
            "status": "no_published_release",
        }))?
    );
    Ok(())
}

fn print_update_human(info: &UpdateInfo) {
    if info.update_available {
        println!(
            "Update available: {} (current {})",
            sanitize_for_terminal(&info.latest_version),
            sanitize_for_terminal(&info.current_version)
        );
        println!("Release: {}", sanitize_for_terminal(&info.release_url));
        if info.assets.is_empty() {
            println!("No release assets are published yet.");
        } else {
            println!("Assets:");
            for asset in &info.assets {
                println!("  {}", sanitize_for_terminal(asset));
            }
        }
        println!("Automatic installation is not implemented yet. Use the release asset or your package manager.");
    } else {
        println!(
            "parc is up to date ({})",
            sanitize_for_terminal(&info.current_version)
        );
    }
}

fn print_no_release_human() {
    println!("No published parc releases found for {}.", GITHUB_REPO);
    println!("Current version: {}", CURRENT_VERSION);
}

fn sanitize_for_terminal(value: &str) -> String {
    sanitize_terminal_text(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_common_release_tags() {
        assert_eq!(normalize_version("0.2.0").unwrap().to_string(), "0.2.0");
        assert_eq!(normalize_version("v0.2.0").unwrap().to_string(), "0.2.0");
        assert_eq!(
            normalize_version("parc-v0.2.0").unwrap().to_string(),
            "0.2.0"
        );
    }

    #[test]
    fn detects_available_update() {
        let latest = ReleaseInfo {
            tag_name: "v0.2.0".to_string(),
            version: "0.2.0".to_string(),
            html_url: "https://github.com/luntta/parc/releases/v0.2.0".to_string(),
            assets: vec!["parc-x86_64-unknown-linux-gnu.tar.gz".to_string()],
        };

        let info = build_update_info("0.1.0", latest).unwrap();

        assert!(info.update_available);
        assert_eq!(info.current_version, "0.1.0");
        assert_eq!(info.latest_version, "0.2.0");
    }

    #[test]
    fn parses_release_response() {
        let release = parse_release(&serde_json::json!({
            "tag_name": "v0.2.0",
            "html_url": "https://github.com/luntta/parc/releases/tag/v0.2.0",
            "assets": [
                { "name": "parc-cli-installer.sh" },
                { "name": "parc-cli-installer.ps1" }
            ]
        }))
        .unwrap();

        assert_eq!(release.tag_name, "v0.2.0");
        assert_eq!(release.version, "0.2.0");
        assert_eq!(release.assets.len(), 2);
    }

    #[test]
    fn sanitizes_remote_text_for_human_output() {
        let sanitized = sanitize_for_terminal("asset\x1b]52;c;AAAA\x07");
        assert!(!sanitized.chars().any(char::is_control));
        assert_eq!(sanitized, "asset]52;c;AAAA");
    }
}
