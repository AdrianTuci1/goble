use serde::{Deserialize, Serialize};

use crate::agent::{AgentId, AgentSpec};
use crate::worker::WorkerId;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DesktopMessage {
    PairRequest {
        worker_id: WorkerId,
        pairing_code_hash: String,
    },
    RunAgent {
        trace_id: String,
        agent_id: AgentId,
        spec: AgentSpec,
    },
    ScheduleAgent {
        agent_id: AgentId,
        trigger: crate::agent::Trigger,
    },
    PushSecrets {
        secrets: Vec<crate::secret::Secret>,
    },
    Ping,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkerMessage {
    Paired,
    AgentStarted {
        trace_id: String,
        agent_id: AgentId,
    },
    AgentLog {
        trace_id: String,
        step_id: String,
        level: crate::execution::LogLevel,
        message: String,
    },
    AgentFinished {
        trace_id: String,
        status: crate::execution::ExecutionStatus,
    },
    StatusReport {
        worker_id: WorkerId,
        status: crate::worker::WorkerStatus,
        load: u8,
    },
    Pong,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope {
    pub version: u32,
    pub payload: Vec<u8>,
    pub signature: Option<Vec<u8>>,
}

impl Envelope {
    pub fn new(payload: Vec<u8>) -> Self {
        Self {
            version: 1,
            payload,
            signature: None,
        }
    }

    pub fn serialize(&self) -> anyhow::Result<Vec<u8>> {
        serde_json::to_vec(self).map_err(anyhow::Error::from)
    }

    pub fn deserialize(bytes: &[u8]) -> anyhow::Result<Self> {
        serde_json::from_slice(bytes).map_err(anyhow::Error::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::AgentSpec;

    #[test]
    fn test_roundtrip_desktop_message() {
        let msg = DesktopMessage::RunAgent {
            trace_id: "t1".to_string(),
            agent_id: AgentId::generate(),
            spec: AgentSpec::new("demo", "do nothing"),
        };
        let bytes = serde_json::to_vec(&msg).unwrap();
        let decoded: DesktopMessage = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn test_roundtrip_worker_message() {
        let msg = WorkerMessage::Paired;
        let bytes = serde_json::to_vec(&msg).unwrap();
        let decoded: WorkerMessage = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn test_envelope_roundtrip() {
        let env = Envelope::new(vec![1, 2, 3]);
        let bytes = env.serialize().unwrap();
        let decoded = Envelope::deserialize(&bytes).unwrap();
        assert_eq!(decoded.payload, vec![1, 2, 3]);
    }
}
