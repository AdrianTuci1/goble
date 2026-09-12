use std::collections::HashSet;
use std::time::Duration;

use anyhow::{Context as _, Result};

use super::WebSearchConfig;

pub(super) async fn web_search(
    args: &serde_json::Value,
    config: &WebSearchConfig,
) -> Result<String> {
    let query = args["query"].as_str().context("query is required")?;
    let advanced = args["advanced"].as_bool().unwrap_or(false);
    let caller_max = args["max_results"].as_u64().unwrap_or(0) as usize;
    // A deeper search defaults to more results; callers can still cap it.
    let default = if advanced { 30 } else { 10 };
    let max_results = if caller_max > 0 {
        caller_max.clamp(1, 30)
    } else {
        default.clamp(1, 30)
    };
    // Hosted backend (e.g. xAI) when configured; otherwise DuckDuckGo. The
    // `advanced` flag drives a deeper, paged DDG search when no backend is set.
    if !config.api_key.is_empty() && !config.base_url.is_empty() {
        return hosted_web_search(
            &config.base_url,
            &config.api_key,
            query,
            max_results,
            advanced,
        )
        .await;
    }
    ddg_web_search(query, max_results, advanced).await
}

async fn hosted_web_search(
    endpoint: &str,
    api_key: &str,
    query: &str,
    max_results: usize,
    advanced: bool,
) -> Result<String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()?;
    let resp = client
        .post(endpoint)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({ "query": query, "max_results": max_results, "advanced": advanced }))
        .send()
        .await
        .context("hosted web_search request failed")?
        .text()
        .await
        .context("hosted web_search failed to read body")?;
    let parsed: serde_json::Value =
        serde_json::from_str(&resp).context("hosted web_search returned invalid JSON")?;
    let empty: Vec<serde_json::Value> = Vec::new();
    let results = parsed
        .get("results")
        .and_then(|r| r.as_array())
        .unwrap_or(&empty);
    let mut out = Vec::new();
    for result in results.iter().take(max_results) {
        let title = result.get("title").and_then(|v| v.as_str()).unwrap_or("");
        let url = result.get("url").and_then(|v| v.as_str()).unwrap_or("");
        let snippet = result.get("snippet").and_then(|v| v.as_str()).unwrap_or("");
        if title.is_empty() && snippet.is_empty() {
            continue;
        }
        out.push(format!(
            "TITLE: {}\nURL: {}\nSNIPPET: {}\n",
            title, url, snippet
        ));
    }
    Ok(format!("{} results\n{}", out.len(), out.join("\n")))
}

/// Decode a DuckDuckGo redirect `//duckduckgo.com/l/?uddg=<encoded>&rut=...` to
/// the real destination URL, falling back to the original href when it is not a
/// DDG redirect.
pub(super) fn decode_ddg_url(href: &str) -> String {
    let idx = href.find("uddg=").map(|i| i + 5).unwrap_or(usize::MAX);
    if idx < href.len() {
        let rest = &href[idx..];
        let encoded = rest.split('&').next().unwrap_or(rest);
        if let Ok(decoded) = urlencoding::decode(encoded) {
            let decoded = decoded.into_owned();
            if decoded.starts_with("http://") || decoded.starts_with("https://") {
                return decoded;
            }
        }
    }
    href.to_string()
}

async fn ddg_web_search(query: &str, max_results: usize, advanced: bool) -> Result<String> {
    let encoded = urlencoding::encode(query);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()?;
    let base_url = format!("https://html.duckduckgo.com/html/?q={}", encoded);
    let mut results: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut offset: Option<usize> = None;
    // `advanced` runs a deeper, best-effort paged scan to gather more distinct
    // results than the handful a single page returns; duplicates are dropped.
    let max_pages = if advanced { 3 } else { 1 };
    for _ in 0..max_pages {
        let url = match offset {
            Some(s) => format!("{base_url}&s={s}"),
            None => base_url.clone(),
        };
        let resp = client
            .get(&url)
            .header("User-Agent", "Mozilla/5.0 (compatible; Goble/1.0)")
            .send()
            .await
            .context("web_search request failed")?
            .text()
            .await
            .context("web_search failed to read body")?;
        offset = regex_lite::Regex::new(r#"name="s" value="(\d+)""#)
            .ok()
            .and_then(|re| re.captures(&resp))
            .and_then(|c| c.get(1))
            .and_then(|m| m.as_str().parse::<usize>().ok())
            .filter(|&s| s > 0);
        for result_html in resp.split(r#"class="result""#).skip(1) {
            let title = regex_lite::Regex::new(r#"class="result__a"[^>]*>(.*?)</a>"#)
                .ok()
                .and_then(|re| re.captures(result_html))
                .and_then(|c| c.get(1))
                .map(|m| html_unescape(m.as_str()))
                .unwrap_or_default();
            let snippet = regex_lite::Regex::new(r#"class="result__snippet"[^>]*>(.*?)</a>"#)
                .ok()
                .and_then(|re| re.captures(result_html))
                .and_then(|c| c.get(1))
                .map(|m| html_unescape(m.as_str()))
                .unwrap_or_default();
            let href = regex_lite::Regex::new(r#"class="result__a"[^>]*href="([^"]+)""#)
                .ok()
                .and_then(|re| re.captures(result_html))
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().to_string())
                .unwrap_or_default();
            let url = decode_ddg_url(&href);
            if title.is_empty() && snippet.is_empty() {
                continue;
            }
            if !seen.insert(url.clone()) {
                continue;
            }
            results.push(format!(
                "TITLE: {}\nURL: {}\nSNIPPET: {}\n",
                title, url, snippet
            ));
            if results.len() >= max_results {
                break;
            }
        }
        if results.len() >= max_results || !advanced {
            break;
        }
    }
    Ok(format!("{} results\n{}", results.len(), results.join("\n")))
}

pub(super) async fn read_url(args: &serde_json::Value) -> Result<String> {
    let url = args["url"].as_str().context("url is required")?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()?;
    let html = client
        .get(url)
        .header("User-Agent", "Mozilla/5.0 (compatible; Goble/1.0)")
        .send()
        .await
        .context("read_url request failed")?
        .text()
        .await
        .context("read_url failed to read body")?;
    let text = html_to_text(&html);
    Ok(text.chars().take(12000).collect())
}

pub(super) async fn execute_python_code(args: &serde_json::Value) -> Result<String> {
    let code = args["code"].as_str().context("code is required")?;
    let dir = tempfile::tempdir().context("failed to create temp dir")?;
    let file = dir.path().join("script.py");
    std::fs::write(&file, code).context("failed to write python script")?;
    let output = tokio::process::Command::new("python3")
        .arg(&file)
        .current_dir(&dir)
        .output()
        .await
        .context("failed to execute python3")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    Ok(format!(
        "exit_code: {}\nstdout:\n{}\nstderr:\n{}",
        output.status.code().unwrap_or(-1),
        stdout,
        stderr
    ))
}

fn html_to_text(html: &str) -> String {
    let mut text = String::new();
    let mut in_tag = false;
    let mut in_script = false;
    let mut tag_buffer = String::new();
    for ch in html.chars() {
        if ch == '<' && !in_script {
            in_tag = true;
            tag_buffer.clear();
            continue;
        }
        if ch == '>' && in_tag {
            in_tag = false;
            let tag = tag_buffer.trim().to_lowercase();
            if tag.starts_with("script") || tag.starts_with("style") {
                in_script = true;
            } else if tag.starts_with("/script") || tag.starts_with("/style") {
                in_script = false;
            }
            if text.ends_with('\n') || text.is_empty() {
                continue;
            }
            if tag.starts_with("br")
                || tag.starts_with("p")
                || tag.starts_with("div")
                || tag.starts_with("h")
                || tag.starts_with("li")
                || tag.starts_with("tr")
            {
                text.push('\n');
            }
            continue;
        }
        if in_tag {
            tag_buffer.push(ch);
            continue;
        }
        if !in_script {
            text.push(ch);
        }
    }
    let re = regex_lite::Regex::new(r"\n\s*\n").unwrap();
    re.replace_all(&text, "\n").into_owned()
}

fn html_unescape(s: &str) -> String {
    let mut out = s.to_string();
    let entities = [
        ("&amp;", "&"),
        ("&lt;", "<"),
        ("&gt;", ">"),
        ("&quot;", "\""),
        ("&#39;", "'"),
        ("&nbsp;", " "),
    ];
    for (enc, dec) in &entities {
        out = out.replace(enc, dec);
    }
    let tag_re = regex_lite::Regex::new(r"<[^>]+>").unwrap();
    tag_re.replace_all(&out, "").into_owned().trim().to_string()
}
