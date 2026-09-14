use std::sync::Arc;

use goble_desktop_service::{DesktopState, VaultSecretInfo};

pub fn list_vault_secrets(state: &Arc<DesktopState>) -> Vec<VaultSecretInfo> {
    state.list_vault_secrets()
}

pub struct VaultSecretRequest {
    pub name: String,
    pub value: String,
}

pub fn set_vault_secret(state: &Arc<DesktopState>, req: VaultSecretRequest) -> anyhow::Result<()> {
    state.set_vault_secret(&req.name, &req.value)
}

pub struct UnlockVaultRequest {
    pub passphrase: String,
}

pub fn unlock_vault(
    state: Arc<DesktopState>,
    req: UnlockVaultRequest,
) -> anyhow::Result<Vec<String>> {
    let res = state.unlock_vault(req.passphrase)?;
    Arc::clone(&state).restore_clients();
    Ok(res)
}
