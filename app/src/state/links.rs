
/// Detect a BYOH desktop-handoff URI in `text` (`rdp://…` or a
/// `goble://desktop?…` link). Returns the trimmed URI substring when found, so
/// a harness that surfaces its handoff as a hyperlink can be launched by the
/// user even without a structured handoff event.
pub(crate) fn detect_screen_link(text: &str) -> Option<String> {
    let start = text.find("rdp://").or_else(|| text.find("goble://desktop"))?;
    let rest = &text[start..];
    let end = rest
        .find(char::is_whitespace)
        .map(|i| start + i)
        .unwrap_or(text.len());
    if end <= start {
        return None;
    }
    let uri = &text[start..end];
    // A trailing punctuation mark is not part of the URI.
    let uri = uri.trim_end_matches(|c: char| c == ')' || c == ']' || c == '}' || c == '.' || c == ',' || c == ';');
    if uri.is_empty() {
        None
    } else {
        Some(uri.to_string())
    }
}

/// Derive a screen-registry source id from a detected handoff URI, best-effort.
/// Remote sources are typically identified by their `user@host:port`; when that
/// cannot be parsed the host (or the whole URI) is used.
pub(crate) fn screen_source_from_link(link: &str) -> Option<String> {
    let mut candidate: Option<String> = None;
    // goble://desktop?user=&host=&port=
    if let Some(q) = link.split_once('?').map(|(_, q)| q) {
        let mut user = String::new();
        let mut host = String::new();
        let mut port = String::new();
        for pair in q.split('&') {
            let mut kv = pair.splitn(2, '=');
            let (k, v) = (kv.next().unwrap_or(""), kv.next().unwrap_or(""));
            match k {
                "user" => user = v.to_string(),
                "host" => host = v.to_string(),
                "port" => port = v.to_string(),
                _ => {}
            }
        }
        if !host.is_empty() {
            candidate = Some(if user.is_empty() {
                if port.is_empty() { host.clone() } else { format!("{host}:{port}") }
            } else if port.is_empty() {
                format!("{user}@{host}")
            } else {
                format!("{user}@{host}:{port}")
            });
        }
    }
    // rdp://[user@]host[:port] — strip scheme.
    if candidate.is_none() {
        if let Some(rest) = link.strip_prefix("rdp://") {
            let rest = rest.split_whitespace().next().unwrap_or(rest);
            let rest = rest.trim_end_matches('/');
            let host = rest.rsplit('@').next().unwrap_or(rest);
            candidate = Some(host.to_string());
        }
    }
    candidate
}

/// The web URL schemes the app is willing to hand to the OS opener. A link
/// that arrived in a transcript is untrusted input, so only these two are
/// accepted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WebScheme {
    Http,
    Https,
}

/// The scheme of `url` when it is a well-formed `http`/`https` URL carrying a
/// non-empty host; `None` for every other scheme (`file:`, `javascript:`,
/// `data:`, `rdp:`, `goble:`, …) and for anything unparseable.
pub(crate) fn web_scheme(url: &str) -> Option<WebScheme> {
    let (scheme, rest) = url.split_once(':')?;
    let scheme = if scheme.eq_ignore_ascii_case("http") {
        WebScheme::Http
    } else if scheme.eq_ignore_ascii_case("https") {
        WebScheme::Https
    } else {
        return None;
    };
    // A web URL carries an authority; reject `http:`/`https:` with no host.
    let host = rest.strip_prefix("//")?;
    let host = host.split(['/', '?', '#']).next().unwrap_or("");
    (!host.is_empty()).then_some(scheme)
}

/// Open a transcript link through the platform opener, but only after the
/// scheme guard: a non-http(s) URL is refused before the opener is reached, so
/// an arbitrary string can never become an OS launch.
pub(crate) fn open_external_url(url: &str) -> Result<(), String> {
    open_external_url_with(url, &platform_open_external)
}

/// The guarded opener with the platform launcher injected, so the guard is
/// testable without starting a browser.
pub(crate) fn open_external_url_with(
    url: &str,
    launch: &dyn Fn(&str) -> Result<(), String>,
) -> Result<(), String> {
    if web_scheme(url).is_none() {
        return Err(format!("refusing to open non-http(s) URL: {url}"));
    }
    launch(url)
}

/// Hand `url` to the OS opener as a single argument (never through a shell).
#[cfg(target_os = "macos")]
pub(crate) fn platform_open_external(url: &str) -> Result<(), String> {
    std::process::Command::new("open")
        .arg(url)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("could not launch `open`: {e}"))
}

#[cfg(target_os = "linux")]
pub(crate) fn platform_open_external(url: &str) -> Result<(), String> {
    std::process::Command::new("xdg-open")
        .arg(url)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("could not launch `xdg-open`: {e}"))
}

#[cfg(target_os = "windows")]
pub(crate) fn platform_open_external(url: &str) -> Result<(), String> {
    // `explorer` opens an http(s) URL in the default browser; it is launched
    // directly, without an intermediate shell.
    std::process::Command::new("explorer")
        .arg(url)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("could not launch `explorer`: {e}"))
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
pub(crate) fn platform_open_external(url: &str) -> Result<(), String> {
    Err(format!("no external opener on this platform: {url}"))
}
