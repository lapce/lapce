//! Session CRUD operations backed by SQLite.
//!
//! Provides create/read/list/delete for sessions and their messages.
//! Designed as a drop-in complement to the JSON-file-based `MemoryManager`.

use super::db::Database;
use crate::providers::provider::ChatMessage;
use rusqlite::params;
use serde::{Deserialize, Serialize};

/// High-level session store backed by the SQLite Database.
pub struct SessionStore {
    db: Database,
}

/// A session summary for listing (no messages — lightweight).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRow {
    pub id: String,
    pub title: String,
    pub working_dir: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub message_count: usize,
}

/// A full session with all messages loaded.
#[derive(Debug, Clone)]
pub struct FullSession {
    pub id: String,
    pub title: String,
    pub working_dir: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub messages: Vec<ChatMessage>,
}

impl SessionStore {
    /// Open the store (creates DB and runs migrations).
    pub fn open() -> rusqlite::Result<Self> {
        Ok(Self { db: Database::open()? })
    }

    // ── Write operations ──

    /// Create a new session and return its ID.
    pub fn create_session(
        &self,
        title: &str,
        working_dir: Option<&str>,
    ) -> rusqlite::Result<String> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();

        self.db.conn().execute(
            "INSERT INTO sessions (id, title, working_dir, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, title, working_dir, now, now],
        )?;
        Ok(id)
    }

    /// Add a message to a session.
    pub fn add_message(&self, session_id: &str, msg: &ChatMessage) -> rusqlite::Result<()> {
        let id = uuid::Uuid::new_v4().to_string();
        let tool_calls_json = msg.tool_calls.as_ref()
            .and_then(|tc| serde_json::to_string(tc).ok());
        let now = chrono::Utc::now().to_rfc3339();

        // Get current max seq for this session
        let max_seq: i64 = self.db.conn().query_row(
            "SELECT COALESCE(MAX(seq), -1) + 1 FROM messages WHERE session_id = ?1",
            params![session_id],
            |r| r.get(0),
        )?;

        self.db.conn().execute(
            "INSERT INTO messages (id, session_id, role, content, tool_calls, tool_call_id, seq, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                id, session_id, msg.role, msg.content,
                tool_calls_json, msg.tool_call_id, max_seq, now,
            ],
        )?;

        // Update session timestamp
        self.db.conn().execute(
            "UPDATE sessions SET updated_at = ?1 WHERE id = ?2",
            params![now, session_id],
        )?;

        // Auto-title from first user message
        let is_new: bool = self.db.conn().query_row(
            "SELECT title = 'New Session' FROM sessions WHERE id = ?1",
            params![session_id],
            |r| r.get(0),
        ).unwrap_or(false);

        if is_new && msg.role == "user" {
            let title: String = msg.content.chars().take(50).collect();
            let title = if msg.content.len() > 50 {
                format!("{}...", title)
            } else {
                title
            };
            self.db.conn().execute(
                "UPDATE sessions SET title = ?1 WHERE id = ?2",
                params![title, session_id],
            )?;
        }

        Ok(())
    }

    /// Bulk-insert messages (efficient for initial save).
    pub fn save_messages(&self, session_id: &str, messages: &[ChatMessage]) -> rusqlite::Result<()> {
        // Delete existing messages for this session (replace-all)
        self.db.conn().execute(
            "DELETE FROM messages WHERE session_id = ?1",
            params![session_id],
        )?;

        for (i, msg) in messages.iter().enumerate() {
            self.add_message_internal(session_id, msg, i as i64)?;
        }

        let now = chrono::Utc::now().to_rfc3339();
        self.db.conn().execute(
            "UPDATE sessions SET updated_at = ?1 WHERE id = ?2",
            params![now, session_id],
        )?;

        Ok(())
    }

    /// Internal: add message with explicit sequence number (for bulk insert).
    fn add_message_internal(&self, session_id: &str, msg: &ChatMessage, seq: i64) -> rusqlite::Result<()> {
        let id = uuid::Uuid::new_v4().to_string();
        let tool_calls_json = msg.tool_calls.as_ref()
            .and_then(|tc| serde_json::to_string(tc).ok());
        let now = chrono::Utc::now().to_rfc3339();

        self.db.conn().execute(
            "INSERT INTO messages (id, session_id, role, content, tool_calls, tool_call_id, seq, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![id, session_id, msg.role, msg.content, tool_calls_json, msg.tool_call_id, seq, now],
        )?;
        Ok(())
    }

    /// Record API usage statistics.
    pub fn record_usage(
        &self,
        provider: &str,
        model: &str,
        prompt_tokens: u32,
        completion_tokens: u32,
        latency_ms: u64,
        success: bool,
    ) -> rusqlite::Result<()> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();

        self.db.conn().execute(
            "INSERT INTO api_usage (id, timestamp, provider, model, prompt_tokens, completion_tokens, total_tokens, latency_ms, success)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                id, now, provider, model,
                prompt_tokens, completion_tokens,
                prompt_tokens + completion_tokens,
                latency_ms as i64,
                if success { 1i64 } else { 0i64 },
            ],
        )?;
        Ok(())
    }

    // ── Read operations ──

    /// List all sessions (without messages — lightweight).
    pub fn list_sessions(&self) -> rusqlite::Result<Vec<SessionRow>> {
        let mut stmt = self.db.conn().prepare(
            "SELECT s.id, s.title, s.working_dir, s.created_at, s.updated_at,
                    COUNT(m.id) as msg_count
             FROM sessions s
             LEFT JOIN messages m ON m.session_id = s.id
             GROUP BY s.id
             ORDER BY s.updated_at DESC"
        )?;

        let rows = stmt.query_map([], |row| {
            Ok(SessionRow {
                id: row.get(0)?,
                title: row.get(1)?,
                working_dir: row.get(2)?,
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
                message_count: row.get(5)?,
            })
        })?;

        rows.collect()
    }

    /// Load a full session with all messages.
    pub fn load_session(&self, session_id: &str) -> rusqlite::Result<Option<FullSession>> {
        // Load session metadata
        let session = self.db.conn().query_row(
            "SELECT id, title, working_dir, created_at, updated_at
             FROM sessions WHERE id = ?1",
            params![session_id],
            |row| {
                Ok(FullSession {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    working_dir: row.get(2)?,
                    created_at: row.get(3)?,
                    updated_at: row.get(4)?,
                    messages: Vec::new(),
                })
            },
        );

        let mut session = match session {
            Ok(s) => s,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
            Err(e) => return Err(e),
        };

        // Load messages ordered by seq
        let mut stmt = self.db.conn().prepare(
            "SELECT role, content, tool_calls, tool_call_id
             FROM messages WHERE session_id = ?1
             ORDER BY seq ASC"
        )?;

        let msgs = stmt.query_map(params![session_id], |row| {
            let tool_calls_json: Option<String> = row.get(2)?;
            Ok(ChatMessage {
                role: row.get(0)?,
                content: row.get(1)?,
                tool_calls: tool_calls_json
                    .and_then(|j| serde_json::from_str(&j).ok()),
                tool_call_id: row.get(3)?,
                ..Default::default()
            })
        })?;

        for msg in msgs {
            session.messages.push(msg?);
        }

        Ok(Some(session))
    }

    // ── Delete operations ──

    /// Delete a session and all its messages (cascade).
    pub fn delete_session(&self, session_id: &str) -> rusqlite::Result<()> {
        self.db.conn().execute(
            "DELETE FROM sessions WHERE id = ?1",
            params![session_id],
        )?;
        Ok(())
    }

    /// Purge sessions older than N days.
    pub fn purge_old_sessions(&self, days: i64) -> rusqlite::Result<usize> {
        let cutoff = chrono::Utc::now() - chrono::Duration::days(days);
        let cutoff_str = cutoff.to_rfc3339();
        let count = self.db.conn().execute(
            "DELETE FROM sessions WHERE updated_at < ?1",
            params![cutoff_str],
        )?;
        Ok(count)
    }

    /// Get total message count across all sessions.
    pub fn total_message_count(&self) -> rusqlite::Result<usize> {
        self.db.conn().query_row(
            "SELECT COUNT(*) FROM messages",
            [],
            |r| r.get(0),
        )
    }
}

// ── Deserialize ChatMessage for SQLite reads ──
// ChatMessage already derives Deserialize for JSON, but we read
// individual columns here, so no custom deserialization needed.

#[cfg(test)]
mod tests {
    use super::*;

    fn test_msg(role: &str, content: &str) -> ChatMessage {
        ChatMessage {
            role: role.to_string(),
            content: content.to_string(),
            tool_calls: None,
            tool_call_id: None,
            ..Default::default()
        }
    }

    #[test]
    fn test_create_and_load_session() {
        let db = Database::open().unwrap();
        db.conn().execute_batch(
            "CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY, title TEXT, working_dir TEXT,
                created_at TEXT, updated_at TEXT
            );
            CREATE TABLE IF NOT EXISTS messages (
                id TEXT PRIMARY KEY, session_id TEXT, role TEXT,
                content TEXT, tool_calls TEXT, tool_call_id TEXT,
                seq INTEGER, created_at TEXT
            );"
        ).unwrap();

        let store = SessionStore { db };

        // Create
        let sid = store.create_session("Test Session", Some("/tmp")).unwrap();
        assert!(!sid.is_empty());

        // Add messages
        store.add_message(&sid, &test_msg("user", "Hello")).unwrap();
        store.add_message(&sid, &test_msg("assistant", "Hi!")).unwrap();

        // Load
        let full = store.load_session(&sid).unwrap().unwrap();
        assert_eq!(full.title, "Hello");
        assert_eq!(full.messages.len(), 2);
        assert_eq!(full.messages[0].role, "user");
        assert_eq!(full.messages[1].role, "assistant");

        // List
        let list = store.list_sessions().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].message_count, 2);

        // Delete
        store.delete_session(&sid).unwrap();
        assert!(store.load_session(&sid).unwrap().is_none());
    }
}
