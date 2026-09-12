use goble_core::principal::PrincipalId;
use goble_core::thread::{Participant, ThreadError, ThreadId, UserId};
use goble_core::user::{AuthorizedKey, UserError, UserProfile};
use sha2::{Digest, Sha256};

use super::ThreadStore;

impl ThreadStore {
    pub fn get_profile(&self) -> Option<UserProfile> {
        self.profile.lock().clone()
    }

    pub fn set_profile(&self, profile: UserProfile) -> anyhow::Result<()> {
        *self.profile.lock() = Some(profile);
        self.save()
    }

    pub fn list_authorized_keys(&self) -> Vec<AuthorizedKey> {
        self.keys.lock().clone()
    }

    pub fn add_authorized_key(&self, key: AuthorizedKey) -> Result<(), UserError> {
        let mut keys = self.keys.lock();
        if keys.iter().any(|k| k.fingerprint == key.fingerprint) {
            return Err(UserError::DuplicateKeyFingerprint(key.fingerprint));
        }
        keys.push(key);
        drop(keys);
        self.save().map_err(|_| UserError::ProfileNotFound)?;
        Ok(())
    }

    pub fn remove_authorized_key(&self, id: &str) -> bool {
        let before = self.keys.lock().len();
        self.keys.lock().retain(|k| k.id != id);
        let removed = self.keys.lock().len() < before;
        if removed {
            let _ = self.save();
        }
        removed
    }

    pub fn invite_user_by_public_key(
        &self,
        thread_id: &ThreadId,
        public_key_pem: impl Into<String>,
        name: impl Into<String>,
    ) -> Result<Participant, ThreadError> {
        let participant = self
            .resolve_user_by_public_key(public_key_pem, name)
            .map_err(|_| ThreadError::Unauthorized)?;
        self.add_participant(thread_id, participant.clone())?;
        Ok(participant)
    }

    pub fn resolve_user_by_public_key(
        &self,
        public_key_pem: impl Into<String>,
        name: impl Into<String>,
    ) -> Result<Participant, UserError> {
        let pem = public_key_pem.into();
        let fingerprint = fingerprint(&pem);
        let id = PrincipalId(fingerprint.clone());
        let key = AuthorizedKey::new(&fingerprint, name, &pem, &fingerprint);
        self.add_authorized_key(key)?;
        Ok(Participant::User(UserId::from_principal(id)))
    }
}

fn fingerprint(pem: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(pem.trim().as_bytes());
    hex::encode(hasher.finalize())
}
