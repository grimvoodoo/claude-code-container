use anyhow::{bail, Result};

/// Accepts any common GitHub repo reference and returns an authenticated HTTPS clone URL.
///
/// Handled formats:
///   owner/repo
///   github.com/owner/repo
///   https://github.com/owner/repo
///   https://github.com/owner/repo/pull/123   (PR URLs — /pull/... is stripped)
///   git@github.com:owner/repo.git            (SSH)
pub fn build_clone_url(raw: &str, token: &str) -> Result<String> {
    let raw = raw.trim();
    let (owner, repo) = parse_owner_repo(raw)?;
    Ok(format!(
        "https://x-access-token:{token}@github.com/{owner}/{repo}.git"
    ))
}

fn parse_owner_repo(raw: &str) -> Result<(String, String)> {
    if raw.starts_with("git@") {
        // git@github.com:owner/repo.git
        let after_colon = raw
            .splitn(2, ':')
            .nth(1)
            .ok_or_else(|| anyhow::anyhow!("Could not parse SSH URL: {raw}"))?;
        return split_owner_repo(after_colon, raw);
    }

    // Strip protocol and optional hostname
    let path = raw
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("github.com/");

    // Handle SSH-style without git@ prefix: github.com:owner/repo
    let path = if let Some(after) = path.strip_prefix("github.com:") {
        after
    } else {
        path
    };

    split_owner_repo(path, raw)
}

fn split_owner_repo(path: &str, original: &str) -> Result<(String, String)> {
    let path = path.trim_end_matches(".git");
    let parts: Vec<&str> = path.splitn(3, '/').collect();
    if parts.len() < 2 || parts[0].is_empty() || parts[1].is_empty() {
        bail!("Could not parse repository: \"{original}\"");
    }
    Ok((parts[0].to_string(), parts[1].to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_formats() {
        let tok = "ghp_token";

        let cases = [
            "owner/repo",
            "github.com/owner/repo",
            "https://github.com/owner/repo",
            "https://github.com/owner/repo.git",
            "https://github.com/owner/repo/pull/123",
            "git@github.com:owner/repo.git",
        ];

        for case in cases {
            let url = build_clone_url(case, tok).unwrap();
            assert!(
                url.contains("owner/repo"),
                "failed for {case}: got {url}"
            );
            assert!(url.starts_with("https://x-access-token:"), "missing token for {case}");
        }
    }
}
