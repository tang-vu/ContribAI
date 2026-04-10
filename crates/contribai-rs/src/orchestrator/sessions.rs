//! Session management for multi-session support.
//!
//! Each session has its own context, circuit breaker state, and memory.
//! Sessions can be created, listed, attached to, killed, and forked.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

/// Session status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStatus {
    Running,
    Paused,
    Finished,
}

/// A single pipeline session.
#[derive(Debug, Clone)]
pub struct Session {
    pub id: String,
    pub name: String,
    pub status: SessionStatus,
    pub created_at: String,
    pub mode: String, // "plan" or "build"
}

/// Session manager — stores and manages sessions.
pub struct SessionManager {
    sessions: Arc<Mutex<HashMap<String, Session>>>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Create a new session.
    pub fn create(&self, name: &str, mode: &str) -> Session {
        let id = Uuid::new_v4().to_string();
        let session = Session {
            id: id.clone(),
            name: name.to_string(),
            status: SessionStatus::Running,
            created_at: chrono::Utc::now().to_rfc3339(),
            mode: mode.to_string(),
        };
        self.sessions.lock().unwrap().insert(id, session.clone());
        session
    }

    /// List all sessions.
    pub fn list(&self) -> Vec<Session> {
        self.sessions.lock().unwrap().values().cloned().collect()
    }

    /// Get a session by ID.
    pub fn get(&self, id: &str) -> Option<Session> {
        self.sessions.lock().unwrap().get(id).cloned()
    }

    /// Kill a session.
    pub fn kill(&self, id: &str) -> bool {
        if let Some(session) = self.sessions.lock().unwrap().get_mut(id) {
            session.status = SessionStatus::Finished;
            true
        } else {
            false
        }
    }

    /// Fork a session (create a copy with a new ID).
    pub fn fork(&self, id: &str, new_name: &str) -> Option<Session> {
        let sessions = self.sessions.lock().unwrap();
        if let Some(original) = sessions.get(id) {
            let new_id = Uuid::new_v4().to_string();
            let forked = Session {
                id: new_id.clone(),
                name: new_name.to_string(),
                status: SessionStatus::Running,
                created_at: chrono::Utc::now().to_rfc3339(),
                mode: original.mode.clone(),
            };
            drop(sessions);
            self.sessions.lock().unwrap().insert(new_id, forked.clone());
            Some(forked)
        } else {
            None
        }
    }

    /// Count active sessions.
    pub fn active_count(&self) -> usize {
        self.sessions
            .lock()
            .unwrap()
            .values()
            .filter(|s| s.status == SessionStatus::Running)
            .count()
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_and_list() {
        let mgr = SessionManager::new();
        mgr.create("test-session", "build");
        let sessions = mgr.list();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].name, "test-session");
        assert_eq!(sessions[0].mode, "build");
    }

    #[test]
    fn test_kill_session() {
        let mgr = SessionManager::new();
        let session = mgr.create("kill-test", "build");
        assert_eq!(session.status, SessionStatus::Running);
        assert!(mgr.kill(&session.id));
        let updated = mgr.get(&session.id).unwrap();
        assert_eq!(updated.status, SessionStatus::Finished);
    }

    #[test]
    fn test_fork_session() {
        let mgr = SessionManager::new();
        let original = mgr.create("original", "plan");
        let forked = mgr.fork(&original.id, "forked").unwrap();
        assert_ne!(forked.id, original.id);
        assert_eq!(forked.name, "forked");
        assert_eq!(forked.mode, original.mode);
        assert_eq!(forked.status, SessionStatus::Running);
        assert_eq!(mgr.list().len(), 2);
    }

    #[test]
    fn test_active_count() {
        let mgr = SessionManager::new();
        mgr.create("s1", "build");
        mgr.create("s2", "plan");
        let s3 = mgr.create("s3", "build");
        mgr.kill(&s3.id);
        assert_eq!(mgr.active_count(), 2);
    }
}
