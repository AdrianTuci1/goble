//! The desktop-native state API: the thin wrappers the shell calls on the
//! shared desktop state.
//!
//! One module per surface the app reads: workers ([`workers`]), cluster
//! identity and its installs ([`cluster`]), chats ([`chat`]), agents
//! ([`agents`]), workflows ([`workflows`]), teams ([`teams`]), execution
//! traces ([`executions`]), the credential vault ([`vault`]), LLM settings
//! ([`llm`]), MCP servers ([`mcp`]), the harness turn ([`harness`]), threads
//! and their messages ([`threads`]) and the user profile with its authorized
//! keys ([`profile`]). Every function and request struct keeps its original
//! `state_api::` path through the re-exports below.

mod agents;
mod chat;
mod cluster;
mod executions;
mod harness;
mod llm;
mod mcp;
mod profile;
mod teams;
mod threads;
mod vault;
mod workers;
mod workflows;

pub use agents::{
    classify_intent, create_agent, delete_agent, list_agents, run_agent,
    run_agent_for_thread_reply, schedule_agent, update_agent, CreateAgentRequest,
    RunAgentForThreadReplyRequest, RunAgentRequest, RuntimeTarget, ScheduleAgentRequest,
    UpdateAgentRequest,
};
pub use chat::{
    add_chat_message, chat_messages, create_chat, list_chats, set_chat_model, SetChatModelRequest,
};
pub use cluster::{
    cluster_helm_install, create_cluster, export_cluster_backup, export_cluster_key,
    export_identity_wallet, get_cluster_identity, has_cluster_identity, import_cluster_key,
    import_identity_wallet, unlock_cluster_identity, ClusterHelmInstallRequest,
    CreateClusterRequest, ExportIdentityRequest, ImportClusterKeyRequest, ImportIdentityRequest,
};
pub use executions::{get_execution_trace, list_executions};
pub use harness::{cancel_harness, list_harness_tools, run_harness, RunHarnessRequest};
pub use llm::{get_llm_setting, set_llm_setting, LlmSettingRequest};
pub use mcp::{
    delete_mcp_server, discover_mcp_tools, install_mcp_server, list_mcp_servers,
    search_mcp_servers, test_call_mcp_tool, update_mcp_server, update_mcp_server_meta,
    InstallMcpRequest, TestCallMcpRequest, UpdateMcpMetaRequest, UpdateMcpRequest,
};
pub use profile::{
    add_authorized_key, get_user_profile, list_authorized_keys, remove_authorized_key,
    set_user_profile, AuthorizedKeyRequest, UserProfileRequest,
};
pub use teams::{create_team, list_teams, CreateTeamRequest};
pub use threads::{
    add_thread_participant, add_thread_reaction, create_thread, delete_thread,
    delete_thread_message, get_thread_messages, get_thread_participants, invite_user_by_public_key,
    list_threads, mark_thread_read, migrate_legacy_chats_to_threads, post_thread_message,
    remove_thread_participant, remove_thread_reaction, update_thread_message,
    AddThreadParticipantRequest, CreateThreadRequest, DeleteThreadMessageRequest,
    InviteUserByPublicKeyRequest, PostMessageRequest, ReactionRequest,
    RemoveThreadParticipantRequest, UpdateThreadMessageRequest,
};
pub use vault::{
    list_vault_secrets, set_vault_secret, unlock_vault, UnlockVaultRequest, VaultSecretRequest,
};
pub use workers::{
    add_worker, generate_worker_invite, install_worker, list_workers, pair_worker, ping_worker,
    remove_worker, tag_worker, worker_logs, AddWorkerRequest, InstallWorkerRequest,
    PairWorkerRequest,
};
pub use workflows::{create_workflow, delete_workflow, list_workflows, CreateWorkflowRequest};
