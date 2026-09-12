use std::sync::Arc;

use goble_desktop_service::DesktopState;

pub fn get_user_profile(state: &Arc<DesktopState>) -> anyhow::Result<goble_core::user::UserProfile> {
    state
        .thread_store()
        .get_profile()
        .ok_or_else(|| anyhow::anyhow!("profile not found"))
}

pub struct UserProfileRequest {
    pub name: String,
    pub email: String,
    pub avatar_url: Option<String>,
    pub public_key_pem: Option<String>,
}

pub fn set_user_profile(
    state: &Arc<DesktopState>,
    req: UserProfileRequest,
) -> anyhow::Result<()> {
    let id = state
        .thread_store()
        .get_profile()
        .map(|p| p.id)
        .unwrap_or_else(goble_core::principal::PrincipalId::generate);
    let mut profile = goble_core::user::UserProfile::new(id, req.name, req.email);
    if let Some(url) = req.avatar_url {
        profile = profile.with_avatar_url(url);
    }
    if let Some(pem) = req.public_key_pem {
        profile = profile.with_public_key(pem);
    }
    state.thread_store().set_profile(profile)
}

pub fn list_authorized_keys(
    state: &Arc<DesktopState>,
) -> anyhow::Result<Vec<goble_core::user::AuthorizedKey>> {
    Ok(state.thread_store().list_authorized_keys())
}

pub struct AuthorizedKeyRequest {
    pub id: String,
    pub name: String,
    pub public_key_pem: String,
    pub fingerprint: String,
    pub thread_ids: Vec<String>,
}

pub fn add_authorized_key(
    state: &Arc<DesktopState>,
    req: AuthorizedKeyRequest,
) -> anyhow::Result<()> {
    let mut key =
        goble_core::user::AuthorizedKey::new(req.id, req.name, req.public_key_pem, req.fingerprint);
    key.thread_ids = req.thread_ids;
    state
        .thread_store()
        .add_authorized_key(key)
        .map_err(|e| anyhow::anyhow!("{e}"))
}

pub fn remove_authorized_key(state: &Arc<DesktopState>, id: &str) -> bool {
    state.thread_store().remove_authorized_key(id)
}
