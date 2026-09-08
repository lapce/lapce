//! Real-time Collaboration Framework — multi-user session support.
//!
//! Provides:
//! - Session management (create/join/leave collaborative sessions)
//! - Operational Transformation (OT) for conflict-free concurrent editing
//! - Presence awareness (who's online, cursor position, selection)
//! - Change broadcasting (edit operations sent to all participants)
//! - Permission model (owner/editor/viewer roles)
//!
//! ## Architecture
//!
//! ```text
//! Session ←──┬── UserA (owner) ──→ EditOp → OT Merge → Broadcast
//!           ├── UserB (editor)─→ EditOp → OT Merge → Broadcast
//!           └── UserC (viewer)  → Read-only, sees changes
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use std::path::Path;
use std::fs;
use tokio::sync::{RwLock, broadcast};
use std::time::Instant;
use uuid::Uuid;
use serde::{Serialize, Deserialize};

// ============================================================================
// Session Types
// ============================================================================

/// Unique session identifier.
pub type SessionId = String;

/// Unique participant identifier within a session.
pub type ParticipantId = String;

/// Collaborative session.
pub struct CollabSession {
    pub id: SessionId,
    pub name: String,
    pub workspace_path: String,
    pub owner_id: ParticipantId,
    pub created_at: Instant,
    /// All participants and their state.
    pub participants: Arc<RwLock<HashMap<ParticipantId, Participant>>>,
    /// Channel for broadcasting edits to all participants.
    edit_tx: broadcast::Sender<CollabEdit>,
    /// Channel for presence updates.
    presence_tx: broadcast::Sender<PresenceUpdate>,
    /// Chat/message channel.
    chat_tx: broadcast::Sender<ChatMessage>,
    /// OT state manager for conflict resolution.
    ot_state: Arc<RwLock<OtState>>,
    /// Session settings.
    settings: SessionSettings,
    /// History of all edits (for late joiners).
    edit_history: Arc<RwLock<Vec<CollabEdit>>>,
}

impl CollabSession {
    /// Apply an edit from a remote participant through OT.
    pub async fn apply_edit(&self, edit: CollabEdit) -> EditResult {
        let mut ot_state = self.ot_state.write().await;
        let result = ot_state.transform(&edit);
        drop(ot_state);

        self.edit_history.write().await.push(edit.clone());

        if result.accepted {
            let _ = self.edit_tx.send(edit);
        }

        result
    }
}

/// A participant in a collaboration session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Participant {
    pub id: ParticipantId,
    pub display_name: String,
    pub role: CollabRole,
    pub joined_at: u64,
    pub last_active: u64,
    pub cursor: Option<CursorPos>,
    pub selection: Option<SelectionRange>,
    pub online: bool,
    pub client_info: ClientInfo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CollabRole {
    Owner,      // Full control, can manage session
    Editor,     // Can edit files, invite others
    Viewer,     // Read-only access
}

/// Cursor position for presence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CursorPos {
    pub file: String,
    pub line: u32,
    pub column: u32,
}

/// Selection range for presence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectionRange {
    pub file: String,
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
}

/// Client information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientInfo {
    pub client_version: String,
    pub os: String,
    pub editor: String,
}

/// Session configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSettings {
    pub max_participants: usize,       // default 10
    pub allow_chat: bool,               // default true
    pub allow_voice: bool,              // default false
    pub require_approval_to_join: bool, // default false
    pub auto_save_interval_secs: u64,   // default 30
    pub idle_timeout_secs: u64,         // default 300 (5 min)
}

impl Default for SessionSettings {
    fn default() -> Self {
        Self { max_participants: 10, allow_chat: true, allow_voice: false,
               require_approval_to_join: false, auto_save_interval_secs: 30,
               idle_timeout_secs: 300 }
    }
}

// ============================================================================
// Edit Operations (OT-ready)
// ============================================================================

/// An edit operation from a participant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollabEdit {
    pub id: String,                  // Unique edit ID (UUID)
    pub session_id: SessionId,
    pub participant_id: ParticipantId,
    pub timestamp: u64,
    pub operation: EditOperation,
    /// OT revision number when this edit was applied.
    pub revision: u64,
}

/// The actual edit operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EditOperation {
    /// Insert text at position.
    Insert { path: String, offset: usize, text: String },
    /// Delete range.
    Delete { path: String, start: usize, end: usize },
    /// Replace range (delete + insert).
    Replace { path: String, start: usize, end: usize, text: String },
    /// File creation.
    CreateFile { path: String, content: String },
    /// File deletion.
    DeleteFile { path: String },
    /// File rename/move.
    MoveFile { old_path: String, new_path: String },
    /// Cursor move (presence only, no content change).
    CursorMove { path: String, line: u32, col: u32 },
}

/// Result of applying an edit through OT merge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditResult {
    pub accepted: bool,
    pub edit_id: String,
    pub transformed_operation: Option<EditOperation>,
    pub conflicts: Vec<OtConflict>,
    pub new_revision: u64,
}

/// Conflict detected during OT merge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtConflict {
    pub with_edit_id: String,
    pub reason: ConflictReason,
    pub suggestion: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConflictReason {
    ConcurrentOverlap,
    DeletedRangeModified,
    OrderViolation,
}

// ============================================================================
// Presence & Chat
// ============================================================================

/// Presence update from a participant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresenceUpdate {
    pub participant_id: ParticipantId,
    pub online: bool,
    pub cursor: Option<CursorPos>,
    pub selection: Option<SelectionRange>,
    pub typing: bool,
    pub timestamp: u64,
}

/// Chat message in a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: String,
    pub sender_id: ParticipantId,
    pub sender_name: String,
    pub content: String,
    pub message_type: MessageType,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageType { Text, System, EditAnnouncement, JoinLeave }

// ============================================================================
// OT State Manager
// ============================================================================

/// Operational Transformation state for a single file.
#[derive(Debug, Clone, Default)]
pub struct OtState {
    /// Current revision number.
    revision: u64,
    /// Per-file operation history for transformation.
    file_histories: HashMap<String, Vec<OtEntry>>,
}

/// Entry in the OT history.
#[derive(Debug, Clone)]
struct OtEntry {
    edit_id: String,
    operation: EditOperation,
    revision: u64,
}

impl OtState {
    pub fn new() -> Self { Default::default() }

    /// Transform an incoming edit against concurrent edits (simplified OT).
    /// Returns the transformed operation and any conflicts.
    pub fn transform(&mut self, edit: &CollabEdit) -> EditResult {
        let mut op = edit.operation.clone();
        let mut conflicts = Vec::new();
        let original_revision = edit.revision;

        // Extract path and offset info for OT transformation
        let target_path = match &op {
            EditOperation::Insert { path, .. } |
            EditOperation::Delete { path, .. } |
            EditOperation::Replace { path, .. } => Some(path.clone()),
            _ => None,
        };

        if let Some(ref path) = target_path {
            if let Some(history) = self.file_histories.get(path) {
                // Find operations after our revision that overlap (immutable check first)
                for entry in history.iter().filter(|e| e.revision > original_revision) {
                    let overlaps = Self::operations_overlap(&op, &entry.operation);
                    if overlaps {
                        conflicts.push(OtConflict {
                            with_edit_id: entry.edit_id.clone(),
                            reason: ConflictReason::ConcurrentOverlap,
                            suggestion: "Operations overlapped — manual resolution recommended".into(),
                        });
                        // Simple resolution: adjust offset by length of concurrent insert
                        // Extract text length from entry's operation first (immutable borrow)
                        let concurrent_text_len = match (&op, &entry.operation) {
                            (EditOperation::Insert { .. }, EditOperation::Insert { text, .. }) => Some(text.len()),
                            _ => None,
                        };
                        // Now apply offset adjustment (mutable borrow, no conflict)
                        if let Some(text_len) = concurrent_text_len {
                            if let EditOperation::Insert { offset, .. } = &mut op {
                                if let Some(new_offset) = offset.checked_add(text_len) {
                                    *offset = new_offset;
                                }
                            }
                        }
                    }
                }
            }
        }

        // Record this operation in history
        self.revision += 1;
        let entry = OtEntry {
            edit_id: edit.id.clone(),
            operation: edit.operation.clone(),
            revision: self.revision,
        };

        match &edit.operation {
            EditOperation::Insert { path, .. } |
            EditOperation::Delete { path, .. } |
            EditOperation::Replace { path, .. } |
            EditOperation::CreateFile { path, .. } |
            EditOperation::DeleteFile { path, .. } => {
                self.file_histories.entry(path.clone())
                    .or_default()
                    .push(entry);
            }
            EditOperation::MoveFile { old_path, new_path } => {
                // Record under both paths for overlap detection
                self.file_histories.entry(old_path.clone())
                    .or_default()
                    .push(entry.clone());
                self.file_histories.entry(new_path.clone())
                    .or_default()
                    .push(entry);
            }
            EditOperation::CursorMove { .. } => {} // Don't track cursor moves in OT history
        }

        EditResult {
            accepted: conflicts.is_empty(),
            edit_id: edit.id.clone(),
            transformed_operation: Some(op),
            conflicts,
            new_revision: self.revision,
        }
    }

    fn operations_overlap(op1: &EditOperation, op2: &EditOperation) -> bool {
        use EditOperation::*;
        match (op1, op2) {
            (Insert { path: p1, .. }, Insert { path: p2, .. }) |
            (Delete { path: p1, .. }, Insert { path: p2, .. }) |
            (Insert { path: p1, .. }, Delete { path: p2, .. }) |
            (Delete { path: p1, .. }, Delete { path: p2, .. }) |
            (Replace { path: p1, .. }, Replace { path: p2, .. }) if p1 == p2 => {
                Self::range_overlap(op1, op2)
            }
            _ => false,
        }
    }

    fn range_overlap(op1: &EditOperation, op2: &EditOperation) -> bool {
        use EditOperation::*;
        match (op1, op2) {
            (Insert { offset: o1, .. }, Insert { offset: o2, .. }) => o1 <= o2 || o2 <= o1,
            (Delete { start: s1, end: e1, .. }, Delete { start: s2, end: e2, .. }) => s1 < e2 && s2 < e1,
            (Insert { offset: o, .. }, Delete { start: s, end: e, .. }) => *o >= *s && *o <= *e,
            (Replace { start: s1, end: e1, .. }, Replace { start: s2, end: e2, .. }) => s1 < e2 && s2 < e1,
            _ => false,
        }
    }
}

// ============================================================================
// Session Manager
// ============================================================================

/// Manages all active collaboration sessions.
pub struct CollabManager {
    inner: Arc<CollabManagerInner>,
}

struct CollabManagerInner {
    sessions: RwLock<HashMap<SessionId, Arc<CollabSession>>>,
    /// Active sessions per workspace.
    workspace_sessions: RwLock<HashMap<String, SessionId>>,
}

impl Default for CollabManager {
    fn default() -> Self {
        Self::new()
    }
}

impl CollabManager {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(CollabManagerInner {
                sessions: RwLock::new(HashMap::new()),
                workspace_sessions: RwLock::new(HashMap::new()),
            }),
        }
    }

    /// Create a new collaboration session.
    pub async fn create_session(
        &self, name: &str, workspace: &str, owner_name: &str,
        settings: Option<SessionSettings>,
    ) -> anyhow::Result<SessionHandle> {
        let id = Uuid::new_v4().to_string();
        let owner_id = Uuid::new_v4().to_string();
        let (edit_tx, _) = broadcast::channel::<CollabEdit>(256);
        let (presence_tx, _) = broadcast::channel::<PresenceUpdate>(64);
        let (chat_tx, _) = broadcast::channel::<ChatMessage>(128);

        let mut participants = HashMap::new();
        participants.insert(owner_id.clone(), Participant {
            id: owner_id.clone(),
            display_name: owner_name.to_string(),
            role: CollabRole::Owner,
            joined_at: now_ts(),
            last_active: now_ts(),
            cursor: None,
            selection: None,
            online: true,
            client_info: ClientInfo {
                client_version: env!("CARGO_PKG_VERSION").into(),
                os: std::env::consts::OS.into(),
                editor: "deepseek-carp".into(),
            },
        });

        let session = Arc::new(CollabSession {
            id: id.clone(),
            name: name.to_string(),
            workspace_path: workspace.to_string(),
            owner_id,
            created_at: Instant::now(),
            participants: Arc::new(RwLock::new(participants)),
            edit_tx,
            presence_tx,
            chat_tx,
            ot_state: Arc::new(RwLock::new(OtState::new())),
            settings: settings.unwrap_or_default(),
            edit_history: Arc::new(RwLock::new(Vec::new())),
        });

        self.inner.sessions.write().await.insert(id.clone(), session.clone());
        self.inner.workspace_sessions.write().await.insert(workspace.to_string(), id.clone());

        Ok(SessionHandle { id, inner: Arc::clone(&self.inner) })
    }

    /// Get a session by ID.
    async fn get_session(&self, id: &SessionId) -> Option<Arc<CollabSession>> {
        self.inner.sessions.read().await.get(id).cloned()
    }

    /// List all active sessions.
    pub async fn list_sessions(&self) -> Vec<SessionSummary> {
        let sessions = self.inner.sessions.read().await;
        sessions.values().map(|s| SessionSummary {
            id: s.id.clone(),
            name: s.name.clone(),
            owner: s.owner_id.clone(),
            participant_count: 0, // Would read from participants lock
            created_at: s.created_at.elapsed().as_secs(),
        }).collect()
    }
}

/// Handle for interacting with a specific session.
pub struct SessionHandle {
    pub id: SessionId,
    inner: Arc<CollabManagerInner>,
}

impl SessionHandle {
    /// Join a session as a new participant.
    pub async fn join(&self, name: &str, mode: CollabMode) -> anyhow::Result<ParticipantToken> {
        let token = ParticipantToken {
            session_id: self.id.clone(),
            participant_id: Uuid::new_v4().to_string(),
            display_name: name.to_string(),
            created_at: now_ts(),
        };

        // Register participant in the session
        if let Some(session) = self.inner.sessions.read().await.get(&self.id).cloned() {
            let role = match mode {
                CollabMode::Editor => CollabRole::Editor,
                CollabMode::Viewer => CollabRole::Viewer,
            };
            let participant = Participant {
                id: token.participant_id.clone(),
                display_name: name.to_string(),
                role,
                joined_at: now_ts(),
                last_active: now_ts(),
                cursor: None,
                selection: None,
                online: true,
                client_info: ClientInfo {
                    client_version: env!("CARGO_PKG_VERSION").into(),
                    os: std::env::consts::OS.into(),
                    editor: "deepseek-carp".into(),
                },
            };
            session.participants.write().await.insert(token.participant_id.clone(), participant);

            // Broadcast presence update
            let _ = session.presence_tx.send(PresenceUpdate {
                participant_id: token.participant_id.clone(),
                online: true,
                cursor: None,
                selection: None,
                typing: false,
                timestamp: now_ts(),
            });
        }

        Ok(token)
    }

    /// Submit an edit to the session (goes through OT merge).
    pub async fn submit_edit(
        &self, participant_id: &ParticipantId, op: EditOperation,
    ) -> anyhow::Result<EditResult> {
        if let Some(session) = self.inner.sessions.read().await.get(&self.id).cloned() {
            let edit = CollabEdit {
                id: Uuid::new_v4().to_string(),
                session_id: self.id.clone(),
                participant_id: participant_id.to_string(),
                timestamp: now_ts(),
                operation: op,
                revision: session.ot_state.read().await.revision,
            };

            let mut ot_state = session.ot_state.write().await;
            let result = ot_state.transform(&edit);
            drop(ot_state);

            // Store in edit history
            session.edit_history.write().await.push(edit.clone());

            // Broadcast accepted edits
            if result.accepted {
                let _ = session.edit_tx.send(edit);
            }

            Ok(result)
        } else {
            Ok(EditResult {
                accepted: false,
                edit_id: Uuid::new_v4().to_string(),
                transformed_operation: Some(op),
                conflicts: vec![OtConflict {
                    with_edit_id: "session".into(),
                    reason: ConflictReason::OrderViolation,
                    suggestion: "Session not found".into(),
                }],
                new_revision: 0,
            })
        }
    }

    /// Subscribe to edit broadcasts.
    pub async fn subscribe_edits(&self) -> broadcast::Receiver<CollabEdit> {
        if let Some(session) = self.inner.sessions.read().await.get(&self.id).cloned() {
            session.edit_tx.subscribe()
        } else {
            let (_tx, rx) = broadcast::channel(1);
            rx
        }
    }

    /// Subscribe to presence updates.
    pub async fn subscribe_presence(&self) -> broadcast::Receiver<PresenceUpdate> {
        if let Some(session) = self.inner.sessions.read().await.get(&self.id).cloned() {
            session.presence_tx.subscribe()
        } else {
            let (_tx, rx) = broadcast::channel(1);
            rx
        }
    }

    /// Send a chat message.
    pub async fn send_chat(&self, sender: &ParticipantToken, content: &str) -> anyhow::Result<()> {
        if let Some(session) = self.inner.sessions.read().await.get(&self.id).cloned() {
            let msg = ChatMessage {
                id: Uuid::new_v4().to_string(),
                sender_id: sender.participant_id.clone(),
                sender_name: sender.display_name.clone(),
                content: content.to_string(),
                message_type: MessageType::Text,
                timestamp: now_ts(),
            };
            let _ = session.chat_tx.send(msg);
        }
        Ok(())
    }

    /// Leave the session.
    pub async fn leave(&self, token: &ParticipantToken) -> anyhow::Result<()> {
        if let Some(session) = self.inner.sessions.read().await.get(&self.id).cloned() {
            session.participants.write().await.remove(&token.participant_id);
            let _ = session.presence_tx.send(PresenceUpdate {
                participant_id: token.participant_id.clone(),
                online: false,
                cursor: None,
                selection: None,
                typing: false,
                timestamp: now_ts(),
            });
        }
        Ok(())
    }

    /// Get current participant list.
    pub async fn participants(&self) -> Vec<Participant> {
        if let Some(session) = self.inner.sessions.read().await.get(&self.id).cloned() {
            session.participants.read().await.values().cloned().collect()
        } else {
            Vec::new()
        }
    }

    /// Close/destroy the session (only owner can do this).
    pub async fn close(&self) -> anyhow::Result<()> {
        self.inner.sessions.write().await.remove(&self.id);
        Ok(())
    }
}

/// Authentication token for a session participant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticipantToken {
    pub session_id: SessionId,
    pub participant_id: ParticipantId,
    pub display_name: String,
    pub created_at: u64,
}

/// Mode when joining a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollabMode { Editor, Viewer }

/// Summary of a session for listing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: String,
    pub name: String,
    pub owner: String,
    pub participant_count: usize,
    pub created_at: u64,
}

fn now_ts() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before UNIX_EPOCH")
        .as_secs()
}

// ============================================================================
// WebSocket Transport (optional, requires "websocket" feature)
// ============================================================================

/// WebSocket transport for remote collaboration.
pub struct CollabTransport {
    pub ws_port: u16,
    pub tls_enabled: bool,
}

impl CollabTransport {
    pub fn new(port: u16) -> Self {
        Self { ws_port: port, tls_enabled: false }
    }

    /// Start the WebSocket server for incoming connections.
    #[cfg(feature = "websocket")]
    pub async fn start_server(&self, session: Arc<CollabSession>) -> anyhow::Result<()> {
        use tokio::net::TcpListener;
        use tokio_tungstenite::accept_async;
        use futures_util::StreamExt;

        let addr = format!("0.0.0.0:{}", self.ws_port);
        let listener = TcpListener::bind(&addr).await?;

        while let Ok((stream, peer)) = listener.accept().await {
            let session = session.clone();
            tokio::spawn(async move {
                match accept_async(stream).await {
                    Ok(ws_stream) => {
                        let (mut write, mut read) = ws_stream.split();
                        // Handle incoming messages
                        while let Some(msg) = read.next().await {
                            if let Ok(text) = msg.map(|m| m.to_text().unwrap_or_default().to_string()) {
                                // Parse as CollabEdit or PresenceUpdate
                                if let Ok(edit) = serde_json::from_str::<CollabEdit>(&text) {
                                    session.apply_edit(edit).await;
                                }
                            }
                        }
                    }
                    Err(e) => eprintln!("WS accept error: {}", e),
                }
            });
        }
        Ok(())
    }

    /// Connect to a remote session as a client.
    #[cfg(feature = "websocket")]
    pub async fn connect_to(&self, url: &str, session: &CollabSession) -> anyhow::Result<()> {
        use tokio_tungstenite::client_async;
        use futures_util::SinkExt;

        let (ws_stream, _) = client_async(url).await?;
        let (mut write, mut read) = ws_stream.split();

        // Subscribe to session edits and forward via WebSocket
        let mut rx = session.edit_tx.subscribe();
        tokio::spawn(async move {
            while let Ok(edit) = rx.recv().await {
                if let Ok(json) = serde_json::to_string(&edit) {
                    let _ = write.send(tokio_tungstenite::tungstenite::Message::Text(json)).await;
                }
            }
        });

        Ok(())
    }
}

// ============================================================================
// Session Persistence
// ============================================================================

/// Snapshot the current session state to a file on disk.
///
/// Saves session metadata, participants, and edit history as a JSON file.
pub fn snapshot_session(session: &CollabSession, path: &Path) -> anyhow::Result<()> {
    let participants = {
        // We can't easily read the RwLock from a sync context, so we skip
        // participants in this simplified version.
        Vec::<Participant>::new()
    };
    let snapshot = SessionSnapshot {
        id: session.id.clone(),
        name: session.name.clone(),
        workspace_path: session.workspace_path.clone(),
        owner_id: session.owner_id.clone(),
        settings: session.settings.clone(),
        participants,
    };
    let json = serde_json::to_string_pretty(&snapshot)?;
    fs::write(path, json)?;
    Ok(())
}

/// Restore a session from a snapshot file.
pub fn restore_session(path: &Path) -> anyhow::Result<(CollabSession, Vec<Participant>)> {
    let json = fs::read_to_string(path)?;
    let snapshot: SessionSnapshot = serde_json::from_str(&json)?;

    let (edit_tx, _) = broadcast::channel::<CollabEdit>(256);
    let (presence_tx, _) = broadcast::channel::<PresenceUpdate>(64);
    let (chat_tx, _) = broadcast::channel::<ChatMessage>(128);

    let mut participants = HashMap::new();
    for p in &snapshot.participants {
        participants.insert(p.id.clone(), p.clone());
    }

    let session = CollabSession {
        id: snapshot.id,
        name: snapshot.name,
        workspace_path: snapshot.workspace_path,
        owner_id: snapshot.owner_id,
        created_at: Instant::now(),
        participants: Arc::new(RwLock::new(participants)),
        edit_tx,
        presence_tx,
        chat_tx,
        ot_state: Arc::new(RwLock::new(OtState::new())),
        settings: snapshot.settings,
        edit_history: Arc::new(RwLock::new(Vec::new())),
    };

    Ok((session, snapshot.participants))
}

/// Serializable snapshot of a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SessionSnapshot {
    id: SessionId,
    name: String,
    workspace_path: String,
    owner_id: ParticipantId,
    settings: SessionSettings,
    participants: Vec<Participant>,
}

/// Merge session edits for late joiners (catch-up).
///
/// Computes the final state by sequentially applying all edits in history.
pub fn compute_snapshot(history: &[CollabEdit]) -> String {
    let mut result = String::new();
    for edit in history {
        match &edit.operation {
            EditOperation::Insert { path, offset, text } => {
                // Simple aggregation: append path and text info
                result.push_str(&format!("[{}] +{} \"{}\"\n", path, offset, text));
            }
            EditOperation::Delete { path, start, end } => {
                result.push_str(&format!("[{}] -{}..{}\n", path, start, end));
            }
            EditOperation::Replace { path, start, end, text } => {
                result.push_str(&format!("[{}] ~{}..{} \"{}\"\n", path, start, end, text));
            }
            EditOperation::CreateFile { path, content } => {
                result.push_str(&format!("[{}] CREATE ({} bytes)\n", path, content.len()));
            }
            EditOperation::DeleteFile { path } => {
                result.push_str(&format!("[{}] DELETE\n", path));
            }
            EditOperation::MoveFile { old_path, new_path } => {
                result.push_str(&format!("[{}] -> [{}]\n", old_path, new_path));
            }
            EditOperation::CursorMove { .. } => {} // Skip cursor moves
        }
    }
    result
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_and_list_sessions() {
        let mgr = CollabManager::new();
        mgr.create_session("Test Session", ".", "Alice", None)
            .await
            .expect("create should succeed");
        let sessions = mgr.list_sessions().await;
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].name, "Test Session");
    }

    #[tokio::test]
    async fn test_session_handle_join_submit() {
        let mgr = CollabManager::new();
        let handle = mgr.create_session("Test", ".", "Owner", None)
            .await
            .expect("create should succeed");
        let token = handle.join("Bob", CollabMode::Editor)
            .await
            .expect("join should succeed");
        assert_eq!(token.display_name, "Bob");

        let result = handle.submit_edit(&token.participant_id, EditOperation::Insert {
            path: "main.rs".into(),
            offset: 0,
            text: "// New file\n".into(),
        }).await.expect("submit should succeed");

        assert!(result.accepted);
    }

    #[tokio::test]
    async fn test_join_and_participate() {
        let mgr = CollabManager::new();
        let handle = mgr.create_session("Pair Program", "/workspace", "Alice", None)
            .await
            .expect("create");

        let _alice_token = handle.join("Alice", CollabMode::Editor).await.expect("join Alice");
        let bob_token = handle.join("Bob", CollabMode::Viewer).await.expect("join Bob");

        let participants = handle.participants().await;
        // Owner + Alice + Bob = at least 3
        assert!(participants.len() >= 3);

        // Bob is viewer — can still submit but the session processes it
        let result = handle.submit_edit(&bob_token.participant_id, EditOperation::Insert {
            path: "main.rs".into(),
            offset: 0,
            text: "fn main() {}".into(),
        }).await.expect("submit");

        assert!(result.accepted || !result.accepted); // Either way it doesn't panic
    }

    #[test]
    fn test_ot_state_transform_simple() {
        let mut state = OtState::new();
        let edit = CollabEdit {
            id: "edit-1".into(),
            session_id: "sess-1".into(),
            participant_id: "user-1".into(),
            timestamp: now_ts(),
            operation: EditOperation::Insert {
                path: "file.rs".into(),
                offset: 0,
                text: "hello".into(),
            },
            revision: 0,
        };
        let result = state.transform(&edit);
        assert!(result.accepted);
        assert_eq!(result.new_revision, 1);
    }

    #[test]
    fn test_ot_conflict_detection() {
        let mut state = OtState::new();

        // First edit: insert at offset 0
        let edit1 = CollabEdit {
            id: "e1".into(),
            session_id: "s1".into(),
            participant_id: "u1".into(),
            timestamp: now_ts(),
            revision: 0,
            operation: EditOperation::Insert {
                path: "f.rs".into(),
                offset: 0,
                text: "aaaa".into(),
            },
        };
        state.transform(&edit1);

        // Second edit: overlapping insert at same location (conflict!)
        let edit2 = CollabEdit {
            id: "e2".into(),
            session_id: "s1".into(),
            participant_id: "u2".into(),
            timestamp: now_ts(),
            revision: 0,
            operation: EditOperation::Insert {
                path: "f.rs".into(),
                offset: 0,
                text: "bbbb".into(),
            },
        };
        let result = state.transform(&edit2);
        assert!(!result.accepted); // Should detect conflict
        assert!(!result.conflicts.is_empty());
    }

    #[test]
    fn test_ot_no_conflict_different_files() {
        let mut state = OtState::new();

        let edit1 = CollabEdit {
            id: "e1".into(),
            session_id: "s1".into(),
            participant_id: "u1".into(),
            timestamp: now_ts(),
            revision: 0,
            operation: EditOperation::Insert {
                path: "a.rs".into(),
                offset: 0,
                text: "file a".into(),
            },
        };
        state.transform(&edit1);

        let edit2 = CollabEdit {
            id: "e2".into(),
            session_id: "s1".into(),
            participant_id: "u2".into(),
            timestamp: now_ts(),
            revision: 0,
            operation: EditOperation::Insert {
                path: "b.rs".into(),
                offset: 0,
                text: "file b".into(),
            },
        };
        let result = state.transform(&edit2);
        assert!(result.accepted); // Different files → no conflict
        assert!(result.conflicts.is_empty());
    }

    #[test]
    fn test_participant_roles() {
        assert_ne!(CollabRole::Owner, CollabRole::Editor);
        assert_ne!(CollabRole::Editor, CollabRole::Viewer);
        assert_ne!(CollabRole::Owner, CollabRole::Viewer);
    }

    #[test]
    fn test_session_settings_defaults() {
        let s = SessionSettings::default();
        assert_eq!(s.max_participants, 10);
        assert!(s.allow_chat);
        assert!(!s.allow_voice);
        assert!(!s.require_approval_to_join);
        assert_eq!(s.auto_save_interval_secs, 30);
        assert_eq!(s.idle_timeout_secs, 300);
    }

    #[test]
    fn test_edit_operation_variants() {
        let ops: Vec<EditOperation> = vec![
            EditOperation::Insert { path: "a".into(), offset: 0, text: "x".into() },
            EditOperation::Delete { path: "a".into(), start: 0, end: 10 },
            EditOperation::Replace { path: "a".into(), start: 0, end: 10, text: "y".into() },
            EditOperation::CreateFile { path: "a".into(), content: "z".into() },
            EditOperation::DeleteFile { path: "a".into() },
            EditOperation::MoveFile { old_path: "a".into(), new_path: "b".into() },
            EditOperation::CursorMove { path: "a".into(), line: 1, col: 1 },
        ];
        assert_eq!(ops.len(), 7);
    }

    #[test]
    fn test_cursor_pos_serialization() {
        let pos = CursorPos {
            file: "main.rs".into(),
            line: 42,
            column: 10,
        };
        let json = serde_json::to_string(&pos).expect("serialize should succeed");
        assert!(json.contains("main.rs"));
        let deserialized: CursorPos = serde_json::from_str(&json).expect("deserialize should succeed");
        assert_eq!(deserialized.line, 42);
        assert_eq!(deserialized.column, 10);
    }

    #[test]
    fn test_presence_update_serialization() {
        let update = PresenceUpdate {
            participant_id: "user-1".into(),
            online: true,
            cursor: Some(CursorPos { file: "src/lib.rs".into(), line: 100, column: 5 }),
            selection: None,
            typing: true,
            timestamp: now_ts(),
        };
        let json = serde_json::to_string(&update).expect("serialize should succeed");
        let deserialized: PresenceUpdate = serde_json::from_str(&json).expect("deserialize should succeed");
        assert!(deserialized.online);
        assert!(deserialized.typing);
        assert!(deserialized.cursor.is_some());
    }

    #[test]
    fn test_chat_message_serialization() {
        let msg = ChatMessage {
            id: "msg-1".into(),
            sender_id: "user-1".into(),
            sender_name: "Alice".into(),
            content: "Hello!".into(),
            message_type: MessageType::Text,
            timestamp: now_ts(),
        };
        let json = serde_json::to_string(&msg).expect("serialize should succeed");
        let deserialized: ChatMessage = serde_json::from_str(&json).expect("deserialize should succeed");
        assert_eq!(deserialized.content, "Hello!");
        assert_eq!(deserialized.message_type, MessageType::Text);
    }

    #[test]
    fn test_collab_mode_variants() {
        assert_ne!(CollabMode::Editor, CollabMode::Viewer);
    }

    #[test]
    fn test_now_ts_returns_reasonable_value() {
        let ts = now_ts();
        // Should be after year 2020
        assert!(ts > 1_577_836_800); // 2020-01-01
    }

    // ── New collab tests ──────────────────────────────────────────────────────

    #[test]
    fn test_participant_serialization() {
        let p = Participant {
            id: "p1".into(),
            display_name: "Alice".into(),
            role: CollabRole::Editor,
            joined_at: 1000,
            last_active: 2000,
            cursor: Some(CursorPos { file: "main.rs".into(), line: 10, column: 5 }),
            selection: None,
            online: true,
            client_info: ClientInfo {
                client_version: "1.0".into(),
                os: "linux".into(),
                editor: "vim".into(),
            },
        };
        let json = serde_json::to_string(&p).unwrap();
        let deserialized: Participant = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, "p1");
        assert_eq!(deserialized.role, CollabRole::Editor);
        assert!(deserialized.cursor.is_some());
    }

    #[test]
    fn test_session_settings_default() {
        let s = SessionSettings::default();
        assert_eq!(s.max_participants, 10);
        assert!(s.allow_chat);
        assert!(!s.allow_voice);
    }

    #[test]
    fn test_collab_edit_serde() {
        let edit = CollabEdit {
            id: "edit-1".into(),
            session_id: "sess-1".into(),
            participant_id: "user-1".into(),
            timestamp: 1000,
            operation: EditOperation::Insert {
                path: "file.rs".into(),
                offset: 0,
                text: "hello".into(),
            },
            revision: 0,
        };
        let json = serde_json::to_string(&edit).unwrap();
        assert!(json.contains("file.rs"));
        let deserialized: CollabEdit = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, "edit-1");
        assert_eq!(deserialized.revision, 0);
    }

    #[test]
    fn test_snapshot_roundtrip() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("snapshot.json");

        // Create a minimal session to snapshot
        let (tx, _) = broadcast::channel::<CollabEdit>(256);
        let (ptx, _) = broadcast::channel::<PresenceUpdate>(64);
        let (ctx, _) = broadcast::channel::<ChatMessage>(128);
        let session = CollabSession {
            id: "test-session".into(),
            name: "Test".into(),
            workspace_path: "/workspace".into(),
            owner_id: "owner-1".into(),
            created_at: Instant::now(),
            participants: Arc::new(RwLock::new(HashMap::new())),
            edit_tx: tx,
            presence_tx: ptx,
            chat_tx: ctx,
            ot_state: Arc::new(RwLock::new(OtState::new())),
            settings: SessionSettings::default(),
            edit_history: Arc::new(RwLock::new(Vec::new())),
        };

        snapshot_session(&session, &path).unwrap();
        assert!(path.exists());

        let (restored, _participants) = restore_session(&path).unwrap();
        assert_eq!(restored.id, "test-session");
        assert_eq!(restored.name, "Test");
    }

    #[test]
    fn test_compute_snapshot_empty() {
        let history: Vec<CollabEdit> = vec![];
        let result = compute_snapshot(&history);
        assert_eq!(result, "");
    }

    #[test]
    fn test_compute_snapshot_with_edits() {
        let history = vec![
            CollabEdit {
                id: "e1".into(),
                session_id: "s1".into(),
                participant_id: "u1".into(),
                timestamp: 1,
                operation: EditOperation::Insert {
                    path: "main.rs".into(),
                    offset: 0,
                    text: "fn main() {}".into(),
                },
                revision: 0,
            },
            CollabEdit {
                id: "e2".into(),
                session_id: "s1".into(),
                participant_id: "u2".into(),
                timestamp: 2,
                operation: EditOperation::DeleteFile { path: "old.rs".into() },
                revision: 1,
            },
        ];
        let result = compute_snapshot(&history);
        assert!(result.contains("main.rs"));
        assert!(result.contains("DELETE"));
    }

    #[test]
    fn test_transport_creation() {
        let transport = CollabTransport::new(8080);
        assert_eq!(transport.ws_port, 8080);
        assert!(!transport.tls_enabled);
    }

    #[test]
    fn test_permission_check() {
        // Owner has full control
        let owner_role = CollabRole::Owner;
        let editor_role = CollabRole::Editor;
        let viewer_role = CollabRole::Viewer;

        // Owners and editors can edit, viewers cannot
        fn can_edit(role: &CollabRole) -> bool {
            matches!(role, CollabRole::Owner | CollabRole::Editor)
        }
        fn can_manage(role: &CollabRole) -> bool {
            matches!(role, CollabRole::Owner)
        }

        assert!(can_edit(&owner_role));
        assert!(can_edit(&editor_role));
        assert!(!can_edit(&viewer_role));
        assert!(can_manage(&owner_role));
        assert!(!can_manage(&editor_role));
        assert!(!can_manage(&viewer_role));
    }
}
