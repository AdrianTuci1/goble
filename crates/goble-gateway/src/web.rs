use chrono::SecondsFormat;

use crate::state::WorkerSummary;

/// Minimal login page. Everything is inline: the gateway must work on a host
/// with no internet access and no static-asset directory.
pub(crate) fn login_page(error: Option<&str>) -> String {
    let mut page = String::from(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Goble Gateway</title>
<style>
  :root { color-scheme: light dark; }
  body { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; max-width: 30rem; margin: 4rem auto; padding: 0 1rem; }
  h1 { font-size: 1.3rem; }
  form { display: flex; gap: .5rem; margin-top: 1.5rem; }
  input { flex: 1; padding: .6rem; font: inherit; }
  button { padding: .6rem 1rem; font: inherit; cursor: pointer; }
  .hint { opacity: .7; font-size: .85rem; }
  .error { color: #b00020; }
</style>
</head>
<body>
<h1>Goble Gateway</h1>
<p class="hint">Enter the PIN printed by the gateway when it started.</p>
"#,
    );

    if let Some(error) = error {
        page.push_str("<p class=\"error\">");
        page.push_str(&html_escape(error));
        page.push_str("</p>\n");
    }

    page.push_str(
        r#"<form method="post" action="/login">
  <input name="pin" type="password" inputmode="numeric" autocomplete="one-time-code" placeholder="PIN" autofocus required>
  <button type="submit">Unlock</button>
</form>
</body>
</html>
"#,
    );
    page
}

/// Transport console. It relays raw JSON text frames; it is deliberately not
/// the terminal UI.
pub(crate) fn panel_page(workers: &[WorkerSummary], public_url: Option<&str>) -> String {
    let mut page = String::from(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Goble Gateway &middot; transport console</title>
<style>
  :root { color-scheme: light dark; }
  body { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; max-width: 60rem; margin: 2rem auto; padding: 0 1rem; }
  h1 { font-size: 1.3rem; margin-bottom: .25rem; }
  h2 { font-size: 1rem; margin-top: 2rem; }
  table { border-collapse: collapse; width: 100%; }
  th, td { text-align: left; padding: .4rem .6rem; border-bottom: 1px solid #8884; }
  code { word-break: break-all; }
  .hint { opacity: .7; font-size: .85rem; }
  textarea { width: 100%; box-sizing: border-box; font: inherit; min-height: 9rem; }
  .row { display: flex; gap: .5rem; align-items: center; flex-wrap: wrap; margin: .75rem 0; }
  button, select { padding: .4rem .8rem; font: inherit; cursor: pointer; }
  #status { font-weight: bold; }
</style>
</head>
<body>
<h1>Goble Gateway</h1>
<p class="hint">Transport console, not the full terminal UI. Frames are relayed opaquely as WebSocket text.</p>
"#,
    );

    if let Some(url) = public_url {
        page.push_str("<p class=\"hint\">Public URL: <code>");
        page.push_str(&html_escape(url));
        page.push_str("</code></p>\n");
    }

    page.push_str("<h2>Connected workers</h2>\n<table>\n<thead><tr><th>Worker</th><th>Connected</th><th>Last seen</th></tr></thead>\n<tbody>\n");
    if workers.is_empty() {
        page.push_str("<tr><td colspan=\"3\">No workers connected.</td></tr>\n");
    } else {
        for worker in workers {
            page.push_str("<tr><td><code>");
            page.push_str(&html_escape(&worker.id));
            page.push_str("</code></td><td>");
            page.push_str(
                &worker
                    .connected_at
                    .to_rfc3339_opts(SecondsFormat::Secs, true),
            );
            page.push_str("</td><td>");
            page.push_str(&worker.last_seen.to_rfc3339_opts(SecondsFormat::Secs, true));
            page.push_str("</td></tr>\n");
        }
    }
    page.push_str("</tbody>\n</table>\n");

    page.push_str(
        r#"<h2>Relay console</h2>
<div class="row">
  <label for="worker">Worker</label>
  <select id="worker">
"#,
    );
    for worker in workers {
        page.push_str("<option value=\"");
        page.push_str(&html_escape(&worker.id));
        page.push_str("\">");
        page.push_str(&html_escape(&worker.id));
        page.push_str("</option>\n");
    }
    page.push_str(
        r#"  </select>
  <button id="connect" type="button">Connect</button>
  <button id="disconnect" type="button">Disconnect</button>
  <span id="status">disconnected</span>
</div>
<textarea id="log" readonly placeholder="Relayed frames appear here."></textarea>
<div class="row">
  <input id="frame" style="flex:1;padding:.5rem;font:inherit" placeholder='{"kind":"ping"}'>
  <button id="send" type="button">Send</button>
</div>
<p class="hint">The send box writes one text frame exactly as typed. Nothing is parsed or rewritten.</p>
<form method="post" action="/logout"><button type="submit">Log out</button></form>
<script>
(() => {
  const select = document.getElementById('worker');
  const log = document.getElementById('log');
  const frame = document.getElementById('frame');
  const status = document.getElementById('status');
  let socket = null;

  function append(line) {
    log.value += line + '\n';
    log.scrollTop = log.scrollHeight;
  }

  function setStatus(text) {
    status.textContent = text;
  }

  function disconnect() {
    if (socket) {
      socket.close();
      socket = null;
    }
    setStatus('disconnected');
  }

  document.getElementById('connect').addEventListener('click', () => {
    disconnect();
    const worker = select.value;
    if (!worker) {
      append('no worker selected');
      return;
    }
    const scheme = location.protocol === 'https:' ? 'wss:' : 'ws:';
    const url = scheme + '//' + location.host + '/ws/client?worker=' + encodeURIComponent(worker);
    socket = new WebSocket(url);
    setStatus('connecting to ' + worker);
    socket.addEventListener('open', () => setStatus('connected to ' + worker));
    socket.addEventListener('message', (event) => {
      if (typeof event.data === 'string') {
        append('<- ' + event.data);
      } else {
        append('<- binary frame (' + event.data.size + ' bytes)');
      }
    });
    socket.addEventListener('close', () => {
      setStatus('disconnected');
      socket = null;
    });
    socket.addEventListener('error', () => append('socket error'));
  });

  document.getElementById('disconnect').addEventListener('click', disconnect);

  document.getElementById('send').addEventListener('click', () => {
    if (!socket || socket.readyState !== WebSocket.OPEN) {
      append('not connected');
      return;
    }
    const text = frame.value;
    socket.send(text);
    append('-> ' + text);
  });
})();
</script>
</body>
</html>
"#,
    );
    page
}

/// Shown when the browser client has not been built yet. It has to explain the
/// fix on a host with no network access, so it stays inline and dependency-free.
pub(crate) fn build_page(web_dir: &str) -> String {
    let mut page = String::from(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Goble Gateway &middot; client not built</title>
<style>
  :root { color-scheme: light dark; }
  body { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; max-width: 46rem; margin: 4rem auto; padding: 0 1rem; }
  h1 { font-size: 1.3rem; }
  h2 { font-size: 1rem; margin-top: 2rem; }
  code, pre { background: #8882; border-radius: .25rem; }
  code { padding: .1rem .3rem; word-break: break-all; }
  pre { padding: .75rem; overflow-x: auto; }
  .hint { opacity: .7; font-size: .85rem; }
</style>
</head>
<body>
<h1>Goble browser client is not built</h1>
<p>The gateway is running, but the terminal client it serves from <code>/app/</code> is missing.
The gateway looked in:</p>
<pre>"#,
    );
    page.push_str(&html_escape(web_dir));
    page.push_str(
        r#"</pre>

<h2>Build it</h2>
<p>From the repository root:</p>
<pre>scripts/build-web.sh</pre>
<p>Or by hand:</p>
<pre>rustup target add wasm32-unknown-unknown
cargo build -p goble-web --target wasm32-unknown-unknown --release
wasm-bindgen target/wasm32-unknown-unknown/release/goble_web.wasm \
  --out-dir crates/goble-gateway/web --target web</pre>

<h2>Point the gateway elsewhere</h2>
<p>Start it with <code>--web-dir &lt;DIR&gt;</code> or set <code>GOBLE_WEB_DIR=&lt;DIR&gt;</code>.
The directory must contain <code>index.html</code>.</p>
<p class="hint">The transport console keeps working at <code>/panel</code> without the client.</p>
</body>
</html>
"#,
    );
    page
}

pub(crate) fn html_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(character),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn html_is_escaped() {
        assert_eq!(
            html_escape("<script>\"x\" & 'y'</script>"),
            "&lt;script&gt;&quot;x&quot; &amp; &#39;y&#39;&lt;/script&gt;"
        );
    }

    #[test]
    fn panel_escapes_worker_ids() {
        let workers = vec![WorkerSummary {
            id: "<img src=x onerror=alert(1)>".to_string(),
            connected_at: Utc::now(),
            last_seen: Utc::now(),
        }];
        let page = panel_page(&workers, None);
        assert!(!page.contains("<img src=x"));
        assert!(page.contains("&lt;img src=x"));
    }

    #[test]
    fn build_page_names_the_path_and_escapes_it() {
        let page = build_page("/srv/<client>");
        assert!(page.contains("scripts/build-web.sh"));
        assert!(page.contains("goble-web --target wasm32-unknown-unknown --release"));
        assert!(page.contains("&lt;client&gt;"));
        assert!(!page.contains("<client>"));
        assert!(!page.contains("http://"));
        assert!(!page.contains("https://"));
    }

    #[test]
    fn pages_contain_no_external_assets() {
        assert!(!login_page(None).contains("http://"));
        assert!(!panel_page(&[], None).contains("http://"));
        assert!(!panel_page(&[], None).contains("https://"));
    }
}
