use std::sync::Arc;

use goble_desktop_service::{DesktopState, LlmSetting};

pub struct LlmSettingRequest {
    pub provider: String,
    pub api_key: String,
    pub base_url: Option<String>,
    pub model: String,
    pub temperature: Option<f32>,
}

pub fn set_llm_setting(
    state: &Arc<DesktopState>,
    req: LlmSettingRequest,
) -> anyhow::Result<()> {
    state.set_llm_setting(
        &req.provider,
        &req.api_key,
        req.base_url.as_deref(),
        &req.model,
        req.temperature,
    )
}

pub fn get_llm_setting(state: &Arc<DesktopState>, provider: &str) -> Option<LlmSetting> {
    state.get_llm_setting(provider)
}
