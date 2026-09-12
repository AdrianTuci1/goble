use crate::store::Store;
use anyhow::Result;

/// Substitute `{{credential:<name>}}` placeholders with the stored secret value
/// at execution time. Only the placeholder (a name) is ever exposed to the
/// model; the value is resolved here and passed to the process argv, so it never
/// appears in the transcript or a tool result.
pub(super) fn expand_credential_refs(store: &Store, s: &str) -> Result<String> {
    const OPEN: &str = "{{credential:";
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    loop {
        match rest.find(OPEN) {
            None => {
                out.push_str(rest);
                break;
            }
            Some(pos) => {
                out.push_str(&rest[..pos]);
                let tail = &rest[pos + OPEN.len()..];
                match tail.find("}}") {
                    None => {
                        out.push_str(OPEN);
                        out.push_str(tail);
                        break;
                    }
                    Some(end) => {
                        let name = tail[..end].trim();
                        match store.get_credential(name)? {
                            Some(value) => out.push_str(&value),
                            None => anyhow::bail!(
                                "unknown credential `{name}`; use the `credentials` tool to list stored credentials"
                            ),
                        }
                        rest = &tail[end + 2..];
                    }
                }
            }
        }
    }
    Ok(out)
}

pub(super) fn list_credentials(store: &Store) -> Result<String> {
    let names = store.list_credential_names()?;
    if names.is_empty() {
        return Ok("no credentials stored".to_string());
    }
    Ok(format!("stored credentials: {}", names.join(", ")))
}

pub(super) fn list_principals(store: &Store) -> Result<String> {
    let principals = store.list_principals()?;
    if principals.is_empty() {
        return Ok("no principals".to_string());
    }
    let mut lines = Vec::new();
    for (id, kind, name, _created) in principals {
        let grants = store.list_access(&id)?;
        let grants_s = if grants.is_empty() {
            "no grants".to_string()
        } else {
            grants
                .iter()
                .map(|(g, s, _)| {
                    if s.is_empty() {
                        g.clone()
                    } else {
                        format!("{g}:{s}")
                    }
                })
                .collect::<Vec<_>>()
                .join(", ")
        };
        lines.push(format!(
            "principal {id} ({kind}, {name}) grants=[{grants_s}]"
        ));
    }
    Ok(lines.join("\n"))
}
