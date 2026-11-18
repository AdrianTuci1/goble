//! Settings → Secrets: the encrypted vault, and the honest gaps.
//!
//! Vault secrets are not settings-file data — they live in the service's
//! encrypted store — but the page belongs in the settings IA because it is where
//! a user looks for a credential. Values are write-only (the service never
//! returns one), so the page shows key names and an unlock/add form.

use super::super::model::{ActionModel, PageModel, RowModel, SectionModel};
use super::super::state::{SettingsAction, SettingsUiState, StatusKind};
use super::unavailable_row;

pub fn page(state: &SettingsUiState) -> PageModel {
    let mut sections = Vec::new();
    let vault = &state.vault;
    let mut vault_section = SectionModel::new("vault", "Vault").with_description(
        "Secrets are encrypted with a passphrase you supply. Values are write-only: the page shows key names, never values.",
    );
    if vault.unlocked {
        vault_section =
            vault_section.with_header_action(ActionModel::new("Lock", SettingsAction::VaultLock));
        for (key, updated) in &vault.secrets {
            let mut row = RowModel::new(
                format!("vault.{key}"),
                key.clone(),
                if updated.is_empty() {
                    "Updated".to_string()
                } else {
                    format!("Updated {updated}")
                },
            )
            .buttons(vec![ActionModel::new(
                "Delete",
                SettingsAction::VaultDelete(key.clone()),
            )
            .with_icon("trash-02")
            .destructive()]);
            row = row.with_status(StatusKind::Success, "Unlocked · value configured");
            vault_section = vault_section.with_row(row);
        }
        if vault.secrets.is_empty() {
            vault_section = vault_section.with_row(
                RowModel::new(
                    "vault.empty",
                    "No secrets yet",
                    "Add one below; the value is encrypted before it is stored.",
                )
                .with_status(StatusKind::Muted, "The vault is unlocked."),
            );
        }
        vault_section = vault_section
            .with_row(
                RowModel::new(
                    "vault.new_key",
                    "Secret name",
                    "The key a config row refers to.",
                )
                .text(
                    "vault.new_key",
                    state.draft("vault.new_key", &vault.new_key),
                    "deploy-token",
                ),
            )
            .with_row(
                RowModel::new(
                    "vault.new_value",
                    "Secret value",
                    "Write-only: encrypted on save and never shown again.",
                )
                .secret(
                    "vault.new_value",
                    state.draft("vault.new_value", &vault.new_value),
                    false,
                ),
            )
            .with_row(
                RowModel::new(
                    "vault.add",
                    "Add secret",
                    "Store the name/value pair in the vault.",
                )
                .buttons(vec![ActionModel::new(
                    "Add secret",
                    SettingsAction::VaultAdd,
                )
                .with_icon("plus")
                .primary()]),
            );
    } else {
        vault_section = vault_section
            .with_row(
                RowModel::new(
                    "vault.locked",
                    "Vault locked",
                    "Unlock with the passphrase you set; nothing on this page can read a secret until it is open.",
                )
                .with_status(StatusKind::Muted, "Locked"),
            )
            .with_row(
                RowModel::new(
                    "vault.passphrase",
                    "Passphrase",
                    "The passphrase is not stored: it opens the vault for this session.",
                )
                .secret(
                    "vault.passphrase",
                    state.draft("vault.passphrase", &vault.passphrase),
                    false,
                ),
            )
            .with_row(
                RowModel::new("vault.unlock", "Unlock", "Open the vault for this session.")
                    .buttons(vec![ActionModel::new("Unlock", SettingsAction::VaultUnlock)
                        .with_icon("key-arrow-right")
                        .primary()]),
            );
    }
    sections.push(vault_section);
    sections.push(
        SectionModel::new("secrets-unavailable", "Not in this build")
            .with_row(unavailable_row(
                "onepassword-secrets",
                "1Password",
                "Read secrets from a 1Password vault.",
                "no 1Password integration",
            ))
            .with_row(unavailable_row(
                "keychain-secrets",
                "OS keychain",
                "Store secrets in the operating system's keychain.",
                "the vault already encrypts them with your passphrase",
            )),
    );
    sections.push(
        SectionModel::new("secrets-note", "How secrets reach a session")
            .with_description(
                "An environment group's `VAR = { vault = \"key\" }` value names a vault entry; the value is resolved when the session starts. A model's inline key in config.toml is written to that 0600 file, not the vault.",
            )
            .with_row(
                RowModel::new(
                    "vault.relation",
                    "Environment groups",
                    "Vault-backed variables are declared in environment.toml.",
                )
                .read_only("environment.toml"),
            ),
    );
    PageModel::new(sections)
}
