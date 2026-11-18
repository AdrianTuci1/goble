
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
/// `data:`, …) and for anything unparseable.
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

/// Hand a local path to the platform opener as one argument, never through a
/// shell. A path is not a URL and does not pass the web-scheme guard: it is
/// opened only because the user pressed a control that names that file.
pub(crate) fn open_local_path(path: &str) -> Result<(), String> {
    platform_open_external(path)
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
