use anyhow::{Context, Result};

use super::Store;

impl Store {
    pub(super) fn migrate(&self) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            ) STRICT;

            CREATE TABLE IF NOT EXISTS agents (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                spec TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            ) STRICT;

            CREATE TABLE IF NOT EXISTS workers (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                host TEXT,
                pairing_status TEXT NOT NULL,
                public_key TEXT,
                config TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            ) STRICT;

            CREATE TABLE IF NOT EXISTS teams (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                metadata TEXT NOT NULL,
                created_at TEXT NOT NULL
            ) STRICT;

            CREATE TABLE IF NOT EXISTS team_members (
                team_id TEXT NOT NULL,
                agent_id TEXT NOT NULL,
                PRIMARY KEY (team_id, agent_id)
            ) STRICT;

            CREATE TABLE IF NOT EXISTS chats (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                provider TEXT,
                model TEXT,
                agent_id TEXT,
                worker_id TEXT,
                workspace_routing TEXT,
                parent_chat_id TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            ) STRICT;

            CREATE TABLE IF NOT EXISTS chat_messages (
                id TEXT PRIMARY KEY,
                chat_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                tool_calls TEXT,
                created_at TEXT NOT NULL
            ) STRICT;

            CREATE TABLE IF NOT EXISTS executions (
                id TEXT PRIMARY KEY,
                agent_id TEXT,
                worker_id TEXT,
                status TEXT NOT NULL,
                trace TEXT NOT NULL,
                started_at TEXT NOT NULL,
                finished_at TEXT
            ) STRICT;

            CREATE TABLE IF NOT EXISTS mcp_servers (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                source TEXT NOT NULL,
                source_value TEXT,
                manifest TEXT NOT NULL,
                credentials_key TEXT,
                secret_ids TEXT NOT NULL DEFAULT '[]',
                enabled_tools TEXT NOT NULL DEFAULT '[]',
                installed_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            ) STRICT;

            CREATE TABLE IF NOT EXISTS vault_secrets (
                key TEXT PRIMARY KEY,
                encrypted_value BLOB NOT NULL,
                metadata TEXT NOT NULL,
                updated_at TEXT NOT NULL
            ) STRICT;

            CREATE TABLE IF NOT EXISTS principals (
                id TEXT PRIMARY KEY,
                kind TEXT NOT NULL,
                name TEXT NOT NULL,
                created_at TEXT NOT NULL
            ) STRICT;

            CREATE TABLE IF NOT EXISTS mcp_accounts (
                id TEXT PRIMARY KEY,
                server_id TEXT NOT NULL,
                principal_id TEXT NOT NULL,
                name TEXT NOT NULL,
                secret_ids TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            ) STRICT;

            CREATE TABLE IF NOT EXISTS access_grants (
                id TEXT PRIMARY KEY,
                principal_id TEXT NOT NULL,
                grant TEXT NOT NULL,
                scope TEXT,
                created_at TEXT NOT NULL
            ) STRICT;
            CREATE INDEX IF NOT EXISTS idx_access_grants_principal ON access_grants(principal_id);

            CREATE TABLE IF NOT EXISTS workflows (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT NOT NULL,
                spec TEXT NOT NULL,
                trigger TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            ) STRICT;

            CREATE INDEX IF NOT EXISTS idx_chat_messages_chat_id ON chat_messages(chat_id);
            CREATE INDEX IF NOT EXISTS idx_executions_agent_id ON executions(agent_id);
            CREATE INDEX IF NOT EXISTS idx_team_members_team_id ON team_members(team_id);
            CREATE INDEX IF NOT EXISTS idx_workflows_updated_at ON workflows(updated_at);
            CREATE INDEX IF NOT EXISTS idx_mcp_accounts_principal ON mcp_accounts(principal_id);

            CREATE TABLE IF NOT EXISTS missions (
                id TEXT PRIMARY KEY,
                chat_id TEXT NOT NULL,
                goal TEXT NOT NULL,
                status TEXT NOT NULL,
                plan TEXT,
                workflow_id TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            ) STRICT;

            CREATE INDEX IF NOT EXISTS idx_missions_chat_id ON missions(chat_id);
            CREATE INDEX IF NOT EXISTS idx_missions_status ON missions(status);

            CREATE TABLE IF NOT EXISTS reasoning_steps (
                id TEXT PRIMARY KEY,
                mission_id TEXT NOT NULL,
                step_index INTEGER NOT NULL,
                mode TEXT NOT NULL,
                content TEXT NOT NULL,
                decision TEXT,
                tool_calls TEXT,
                created_at TEXT NOT NULL
            ) STRICT;

            CREATE INDEX IF NOT EXISTS idx_reasoning_mission ON reasoning_steps(mission_id, step_index);

            CREATE TABLE IF NOT EXISTS pending_asks (
                id TEXT PRIMARY KEY,
                chat_id TEXT NOT NULL,
                mission_id TEXT,
                question TEXT NOT NULL,
                quick_replies TEXT NOT NULL,
                status TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            ) STRICT;

            CREATE INDEX IF NOT EXISTS idx_pending_asks_chat ON pending_asks(chat_id, status);

            CREATE TABLE IF NOT EXISTS pending_commands (
                call_id TEXT PRIMARY KEY,
                chat_id TEXT NOT NULL,
                message_id TEXT NOT NULL,
                candidates TEXT NOT NULL,
                cwd TEXT NOT NULL,
                status TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            ) STRICT;

            CREATE INDEX IF NOT EXISTS idx_pending_commands_chat ON pending_commands(chat_id, status);

            CREATE TABLE IF NOT EXISTS llm_settings (
                provider TEXT PRIMARY KEY,
                api_key TEXT NOT NULL,
                base_url TEXT,
                model TEXT NOT NULL,
                temperature REAL
            ) STRICT;
            CREATE TABLE IF NOT EXISTS credentials (
                name TEXT PRIMARY KEY,
                value TEXT NOT NULL
            ) STRICT;
            CREATE TABLE IF NOT EXISTS audit_log (
                id TEXT PRIMARY KEY,
                timestamp TEXT NOT NULL,
                category TEXT NOT NULL,
                actor TEXT NOT NULL,
                action TEXT NOT NULL,
                details TEXT NOT NULL
            ) STRICT;

            CREATE INDEX IF NOT EXISTS idx_audit_log_timestamp ON audit_log(timestamp DESC);

            CREATE TABLE IF NOT EXISTS agent_memory (
                agent_id TEXT PRIMARY KEY,
                version INTEGER NOT NULL,
                memory TEXT NOT NULL,
                updated_at TEXT NOT NULL
            ) STRICT;

            CREATE TABLE IF NOT EXISTS device_identities (
                id TEXT PRIMARY KEY,
                cluster_name TEXT NOT NULL,
                cert_pem TEXT NOT NULL,
                key_pem TEXT NOT NULL,
                ca_cert_pem TEXT NOT NULL,
                role TEXT NOT NULL,
                is_owner INTEGER NOT NULL DEFAULT 0,
                deployment_mode TEXT NOT NULL DEFAULT 'local',
                deployment_config TEXT NOT NULL DEFAULT '{}',
                created_at TEXT NOT NULL
            ) STRICT;
            CREATE INDEX IF NOT EXISTS idx_device_identities_owner ON device_identities(is_owner);

            CREATE TABLE IF NOT EXISTS cluster_invites (
                id TEXT PRIMARY KEY,
                cluster_name TEXT NOT NULL,
                code TEXT NOT NULL UNIQUE,
                pem_bundle TEXT NOT NULL,
                revoked INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL
            ) STRICT;
            CREATE INDEX IF NOT EXISTS idx_cluster_invites_code ON cluster_invites(code);
            CREATE INDEX IF NOT EXISTS idx_cluster_invites_cluster ON cluster_invites(cluster_name);
            "#,
        )
        .context("failed to run migrations")?;

        // `CREATE TABLE IF NOT EXISTS` does not add a column to a table that
        // already exists, so add `workspace_routing` to pre-existing `chats`
        // tables (created before the column existed).
        let has_routing: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('chats') WHERE name = 'workspace_routing'",
                [],
                |r| r.get(0),
            )
            .context("failed to check for chats.workspace_routing")?;
        if !has_routing {
            conn.execute("ALTER TABLE chats ADD COLUMN workspace_routing TEXT", [])
                .context("failed to add chats.workspace_routing")?;
        }

        // Same idiom for `parent_chat_id`: present only on a sub-agent's own
        // chats row, naming the conversation that spawned it.
        let has_parent: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('chats') WHERE name = 'parent_chat_id'",
                [],
                |r| r.get(0),
            )
            .context("failed to check for chats.parent_chat_id")?;
        if !has_parent {
            conn.execute("ALTER TABLE chats ADD COLUMN parent_chat_id TEXT", [])
                .context("failed to add chats.parent_chat_id")?;
        }

        Ok(())
    }
}
