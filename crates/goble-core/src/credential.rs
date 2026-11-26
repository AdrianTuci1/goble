//! One credential resolver for every integration: the composer, model
//! selector, Settings, send path and worker seeding all ask the same question,
//! in the same documented order, so a key present in any source is usable.

/// Where a resolved credential came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CredentialSource {
    Env,
    Keychain,
    Vault,
    ConfigInline,
    ConfigEnv,
    Store,
    CliLogin,
}

impl CredentialSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Env => "environment",
            Self::Keychain => "keychain",
            Self::Vault => "vault",
            Self::ConfigInline => "config",
            Self::ConfigEnv => "config env",
            Self::Store => "store",
            Self::CliLogin => "CLI login",
        }
    }
}

/// Every source, already read from its native store. The order of the fields is
/// the precedence: env > keychain > vault > config inline > config
/// `api_key_env` > app store > CLI login.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CredentialSources {
    pub env: Option<String>,
    pub keychain: Option<String>,
    pub vault: Option<String>,
    pub config_inline: Option<String>,
    pub config_env: Option<String>,
    pub store: Option<String>,
    pub cli_login: bool,
}

/// A resolved credential: the value (when there is one) and the source to show
/// in the UI. CLI login is a source but has no API key string.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResolvedCredential {
    pub value: Option<String>,
    pub source: Option<CredentialSource>,
}

pub fn resolve(sources: &CredentialSources) -> ResolvedCredential {
    let candidates = [
        (sources.env.as_deref(), CredentialSource::Env),
        (sources.keychain.as_deref(), CredentialSource::Keychain),
        (sources.vault.as_deref(), CredentialSource::Vault),
        (
            sources.config_inline.as_deref(),
            CredentialSource::ConfigInline,
        ),
        (sources.config_env.as_deref(), CredentialSource::ConfigEnv),
        (sources.store.as_deref(), CredentialSource::Store),
    ];
    for (value, source) in candidates {
        if let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) {
            return ResolvedCredential {
                value: Some(value.to_string()),
                source: Some(source),
            };
        }
    }
    if sources.cli_login {
        return ResolvedCredential {
            value: None,
            source: Some(CredentialSource::CliLogin),
        };
    }
    ResolvedCredential::default()
}

/// The environment variable a provider's key is conventionally read from.
pub fn provider_env_var(provider: &str) -> Option<&'static str> {
    match provider {
        "anthropic" | "claude" => Some("ANTHROPIC_API_KEY"),
        "openai" | "codex" => Some("OPENAI_API_KEY"),
        "google" | "gemini" => Some("GOOGLE_API_KEY"),
        "deepseek" => Some("DEEPSEEK_API_KEY"),
        "antigravity" => Some("ANTIGRAVITY_API_KEY"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sources() -> CredentialSources {
        CredentialSources::default()
    }

    #[test]
    fn env_only_makes_the_model_usable() {
        let resolved = resolve(&CredentialSources {
            env: Some("env-key".to_string()),
            ..sources()
        });
        assert_eq!(resolved.value.as_deref(), Some("env-key"));
        assert_eq!(resolved.source, Some(CredentialSource::Env));
    }

    #[test]
    fn every_source_is_recognised_and_precedence_is_documented() {
        for (source, expected) in [
            (
                CredentialSources {
                    keychain: Some("kc".to_string()),
                    ..sources()
                },
                CredentialSource::Keychain,
            ),
            (
                CredentialSources {
                    vault: Some("vault".to_string()),
                    ..sources()
                },
                CredentialSource::Vault,
            ),
            (
                CredentialSources {
                    config_inline: Some("inline".to_string()),
                    ..sources()
                },
                CredentialSource::ConfigInline,
            ),
            (
                CredentialSources {
                    config_env: Some("from-env".to_string()),
                    ..sources()
                },
                CredentialSource::ConfigEnv,
            ),
            (
                CredentialSources {
                    store: Some("store".to_string()),
                    ..sources()
                },
                CredentialSource::Store,
            ),
        ] {
            let resolved = resolve(&source);
            assert_eq!(resolved.source, Some(expected));
            assert!(resolved.value.is_some());
        }
        let env_wins = resolve(&CredentialSources {
            env: Some("env".to_string()),
            keychain: Some("kc".to_string()),
            config_inline: Some("inline".to_string()),
            ..sources()
        });
        assert_eq!(env_wins.source, Some(CredentialSource::Env));
    }

    #[test]
    fn cli_login_is_a_source_without_an_api_key() {
        let resolved = resolve(&CredentialSources {
            cli_login: true,
            ..sources()
        });
        assert_eq!(resolved.value, None);
        assert_eq!(resolved.source, Some(CredentialSource::CliLogin));
    }

    #[test]
    fn no_source_is_an_honest_missing_state() {
        let resolved = resolve(&sources());
        assert_eq!(resolved.value, None);
        assert_eq!(resolved.source, None);
    }
}
