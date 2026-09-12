use std::collections::HashMap;
use std::path::Path;

use goble_core::thread::{Thread, ThreadMessage};
use parking_lot::Mutex;

use super::ThreadStore;

const THREADS_FILE: &str = "threads.json";
const MESSAGES_DIR: &str = "messages";
const USERS_FILE: &str = "users.json";
const KEYS_FILE: &str = "keys.json";
const READ_RECEIPTS_FILE: &str = "read_receipts.json";

impl ThreadStore {
    pub fn new(base_path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let base_path = base_path.as_ref().to_path_buf();
        std::fs::create_dir_all(&base_path)?;
        std::fs::create_dir_all(base_path.join(MESSAGES_DIR))?;

        let store = Self {
            threads: Mutex::new(Vec::new()),
            messages: Mutex::new(HashMap::new()),
            last_read_at: Mutex::new(HashMap::new()),
            profile: Mutex::new(None),
            keys: Mutex::new(Vec::new()),
            base_path,
        };

        store.load()?;
        Ok(store)
    }

    pub(super) fn save(&self) -> anyhow::Result<()> {
        let threads = self.threads.lock();
        let messages = self.messages.lock();
        let profile = self.profile.lock();
        let keys = self.keys.lock();

        std::fs::write(
            self.base_path.join(THREADS_FILE),
            serde_json::to_string_pretty(&*threads)?,
        )?;

        for (thread_id, list) in messages.iter() {
            std::fs::write(
                self.base_path
                    .join(MESSAGES_DIR)
                    .join(format!("{}.jsonl", thread_id)),
                list.iter()
                    .map(serde_json::to_string)
                    .collect::<Result<Vec<_>, _>>()?
                    .join("\n"),
            )?;
        }

        std::fs::write(
            self.base_path.join(USERS_FILE),
            serde_json::to_string_pretty(&*profile)?,
        )?;

        std::fs::write(
            self.base_path.join(KEYS_FILE),
            serde_json::to_string_pretty(&*keys)?,
        )?;

        let last_read = self.last_read_at.lock();
        std::fs::write(
            self.base_path.join(READ_RECEIPTS_FILE),
            serde_json::to_string_pretty(&*last_read)?,
        )?;

        Ok(())
    }

    fn load(&self) -> anyhow::Result<()> {
        let threads_path = self.base_path.join(THREADS_FILE);
        if threads_path.exists() {
            let data = std::fs::read_to_string(&threads_path)?;
            let threads: Vec<Thread> = serde_json::from_str(&data)?;
            *self.threads.lock() = threads;
        }

        let messages_dir = self.base_path.join(MESSAGES_DIR);
        if messages_dir.exists() {
            let mut map = HashMap::new();
            for entry in std::fs::read_dir(&messages_dir)? {
                let entry = entry?;
                let path = entry.path();
                if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                    let content = std::fs::read_to_string(&path)?;
                    let mut messages = Vec::new();
                    for line in content.lines() {
                        if line.trim().is_empty() {
                            continue;
                        }
                        messages.push(serde_json::from_str::<ThreadMessage>(line)?);
                    }
                    map.insert(name.to_string(), messages);
                }
            }
            *self.messages.lock() = map;
        }

        let profile_path = self.base_path.join(USERS_FILE);
        if profile_path.exists() {
            let data = std::fs::read_to_string(&profile_path)?;
            *self.profile.lock() = serde_json::from_str(&data)?;
        }

        let keys_path = self.base_path.join(KEYS_FILE);
        if keys_path.exists() {
            let data = std::fs::read_to_string(&keys_path)?;
            *self.keys.lock() = serde_json::from_str(&data)?;
        }

        let read_path = self.base_path.join(READ_RECEIPTS_FILE);
        if read_path.exists() {
            let data = std::fs::read_to_string(&read_path)?;
            *self.last_read_at.lock() = serde_json::from_str(&data)?;
        }

        Ok(())
    }
}
