//! DeepSeek Carp — AI Chat Panel for Lapce (multi-session, streaming, SDK-integrated).
//!
//! ## Capabilities
//! - **Streaming**: real-time token display via QueryEngine::stream_submit()
//! - **Agent fallback**: full tool-calling loop when streaming unavailable
//! - **Multi-session**: each chat has a unique session_id; Agent persists across messages
//! - **Memory**: auto-save conversation via MemoryManager
//! - **Plan mode**: `/plan <task>` — draft execution plan
//! - **Swarm mode**: `/swarm <task>` — decompose into parallel sub-tasks
//! - **RAG enrichment**: auto-index workspace, inject relevant code into prompts
//! - **Apply-to-Editor**: parse AI code blocks → inline diff preview → Accept/Reject
//! - **MCP**: `/mcp` status + connect external tool servers
//!
//! ## Cross-thread: sync_channel (tx→bg, rx→UI poll) + RwSignal on UI thread

use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};

use floem::{
    IntoView, View,
    reactive::{RwSignal, SignalGet, SignalUpdate, SignalWith},
    style::{CursorStyle, Display},
    views::{container, dyn_stack, label, scroll, stack, text_input, Decorators},
};
use floem::peniko::Color;
use deepseek_carp::agent::Agent;
use deepseek_carp::tools::{DiffEngine, FileEdit};

use super::position::PanelPosition;
use crate::ai;
use crate::localization::{self, Locale, t};
use crate::window_tab::WindowTabData;

// ── Types ──────────────────────────────────────────────────────

#[derive(Clone)]
struct ChatMsg {
    role: String,
    content: String,
    footer: String,
    /// Parsed file edits from this message (for Accept/Reject).
    edits: Vec<FileEdit>,
    /// Optional retrieval context (collapsible), e.g. MCP context_retrieve result.
    context: Option<String>,
}

/// Cross-thread messages from bg AI thread → UI.
#[derive(Clone)]
enum ChatResult {
    /// Streaming chunk (incremental)
    Stream { text: String, done: bool, provider: String },
    /// Final result from Agent loop
    Response { content: String, footer: String, context: Option<String> },
    /// Diff edits parsed from response for Apply-to-Editor
    Diff { edits: Vec<FileEdit> },
    /// Context retrieval only (shown collapsible)
    Context { text: String },
    /// Side-tool result (security scan / run_tests)
    Tool { name: String, output: String },
    Error(String),
}

#[derive(Clone)]
struct TypewriterState {
    buffer: String,
    typed: usize,
    interval_ms: u64,
    active: bool,
    last_tick: std::time::Instant,
}

static SESSION_COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
fn next_session_id() -> String {
    let n = SESSION_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    format!("lapce-chat-{}", n)
}

// ── Helpers ────────────────────────────────────────────────────

/// Generate a diff preview text for a FileEdit (for message display).
fn format_diff_text(edit: &FileEdit) -> String {
    let hunks = DiffEngine::generate(&edit.original, &edit.modified);
    let mut s = format!("📝 {}\n", edit.file_path.display());
    if let Some(ref desc) = edit.description {
        s.push_str(&format!("    {}\n", desc));
    }
    s.push_str("──────────────────────────────\n");
    if hunks.is_empty() {
        s.push_str("  (no changes detected)\n");
    } else {
        for hunk in &hunks {
            s.push_str(&hunk.text);
        }
    }
    s.push_str("──────────────────────────────\n");
    s
}

// ── Panel ──────────────────────────────────────────────────────

pub fn chat_panel(
    _window_tab_data: Rc<WindowTabData>,
    _position: PanelPosition,
) -> impl View {
    let session_id = next_session_id();

    let messages: RwSignal<Vec<ChatMsg>> = RwSignal::new(vec![ChatMsg {
        role: "system".into(),
        content: format!("dscarp — AI Coding Assistant\nSession: {}\n\n/plan /execute /swarm /swarm-run /swarm-status /list-plans /compile /metrics /apply /mcp /clear /stats", session_id),
        footer: String::new(),
        edits: Vec::new(),
        context: None,
    }]);
    let input_text = RwSignal::new(String::new());
    let sending = RwSignal::new(false);
    let stream_buf: RwSignal<String> = RwSignal::new(String::new());
    let typewriter: RwSignal<String> = RwSignal::new(String::new());
    let tw_state = std::sync::Arc::new(std::sync::Mutex::new(TypewriterState {
        buffer: String::new(),
        typed: 0,
        interval_ms: 30,
        active: false,
        last_tick: std::time::Instant::now(),
    }));
    let agent_signal: RwSignal<Option<Agent>> = RwSignal::new(None);
    let last_retrieved_ctx: RwSignal<Option<String>> = RwSignal::new(None);
    let ctx_visible = RwSignal::new(false);
    let tool_busy = RwSignal::new(false);

    // ── Cross-thread channel ──
    let (tx, rx) = std::sync::mpsc::sync_channel::<ChatResult>(8);
    let rx = std::sync::Arc::new(std::sync::Mutex::new(rx));

    // ── Clones ──
    let rx_for_render = rx.clone();
    let input_for_send = input_text.clone();
    let messages_for_response = messages.clone();
    let sending_for_response = sending.clone();
    let agent_for_async = agent_signal.clone();
    let sid_for_async = session_id.clone();
    let tw_state_for_render = tw_state.clone();
    let tw_state_for_enter = tw_state.clone();

    stack((
        // ── Message list ──
        scroll({
            let id = std::sync::atomic::AtomicU64::new(0);
            let rx = rx_for_render.clone();
            stack((
                dyn_stack(
                    move || {
                        // ── Poll cross-thread channel on every UI frame ──
                        if let Ok(lock) = rx.lock() {
                            let mut msgs = messages.get();
                            let mut changed = false;
                            while let Ok(result) = lock.try_recv() {
                                changed = true;
                                match result {
                                    ChatResult::Stream { text, done: true, provider } => {
                                        typewriter.set(text.clone());
                                        stream_buf.set(String::new());
                                        let edits = ai::parse_edits(&text);
                                        if !edits.is_empty() {
                                            let diff_text = edits.iter()
                                                .map(|e| format_diff_text(e))
                                                .collect::<Vec<_>>()
                                                .join("\n");
                                            let full_content = format!("{}\n\n## Code Changes\n{}", text, diff_text);
                                            msgs.push(ChatMsg {
                                                role: "assistant".into(),
                                                content: full_content,
                                                footer: format!("stream • {}", provider),
                                                edits,
                                                context: None,
                                            });
                                        } else {
                                            msgs.push(ChatMsg {
                                                role: "assistant".into(),
                                                content: text,
                                                footer: format!("stream • {}", provider),
                                                edits: Vec::new(),
                                                context: None,
                                            });
                                        }
                                        typewriter.set(String::new());
                                        {
                                            let mut st = tw_state_for_render.lock().unwrap();
                                            st.buffer.clear();
                                            st.typed = 0;
                                            st.active = false;
                                        }
                                        sending.set(false);
                                    }
                                    ChatResult::Stream { text, .. } => {
                                        stream_buf.set(text.clone());
                                        let mut st = tw_state_for_render.lock().unwrap();
                                        if st.buffer.len() < text.len() || (text.is_empty() && !st.buffer.is_empty()) {
                                            st.buffer = text;
                                        }
                                        if !st.active && st.typed < st.buffer.len() {
                                            st.active = true;
                                            st.last_tick = std::time::Instant::now();
                                        }
                                    }
                                    ChatResult::Response { content, footer, context } => {
                                        typewriter.set(String::new());
                                        {
                                            let mut st = tw_state_for_render.lock().unwrap();
                                            st.buffer.clear();
                                            st.typed = 0;
                                            st.active = false;
                                        }
                                        stream_buf.set(String::new());
                                        let edits = ai::parse_edits(&content);
                                        let ctx = context.clone();
                                        if !edits.is_empty() {
                                            let diff_text = edits.iter()
                                                .map(|e| format_diff_text(e))
                                                .collect::<Vec<_>>()
                                                .join("\n");
                                            let full = format!("{}\n\n## Code Changes\n{}", content, diff_text);
                                            msgs.push(ChatMsg {
                                                role: "assistant".into(),
                                                content: full,
                                                footer,
                                                edits,
                                                context: ctx,
                                            });
                                        } else {
                                            msgs.push(ChatMsg {
                                                role: "assistant".into(),
                                                content,
                                                footer,
                                                edits: Vec::new(),
                                                context: ctx,
                                            });
                                        }
                                        if let Some(c) = context { last_retrieved_ctx.set(Some(c)); }
                                        sending.set(false);
                                    }
                                    ChatResult::Diff { edits } => {
                                        let summary = edits.iter()
                                            .map(|e| format!(
                                                "{}: {}→{} lines",
                                                e.file_path.display(),
                                                e.original.lines().count(),
                                                e.modified.lines().count(),
                                            ))
                                            .collect::<Vec<_>>()
                                            .join(", ");
                                        msgs.push(ChatMsg {
                                            role: "diff".into(),
                                            content: format!("Applied {} edit(s): {}", edits.len(), summary),
                                            footer: String::new(),
                                            edits,
                                            context: None,
                                        });
                                    }
                                    ChatResult::Context { text } => {
                                        last_retrieved_ctx.set(Some(text.clone()));
                                        ctx_visible.set(true);
                                        msgs.push(ChatMsg {
                                            role: "context".into(),
                                            content: format!("## Retrieved Context\n\n{text}"),
                                            footer: "MCP context_retrieve".into(),
                                            edits: Vec::new(),
                                            context: None,
                                        });
                                    }
                                    ChatResult::Tool { name, output } => {
                                        tool_busy.set(false);
                                        msgs.push(ChatMsg {
                                            role: "tool".into(),
                                            content: format!("## {name}\n\n{output}"),
                                            footer: "MCP tool".into(),
                                            edits: Vec::new(),
                                            context: None,
                                        });
                                    }
                                    ChatResult::Error(err) => {
                                        typewriter.set(String::new());
                                        {
                                            let mut st = tw_state_for_render.lock().unwrap();
                                            st.buffer.clear();
                                            st.typed = 0;
                                            st.active = false;
                                        }
                                        stream_buf.set(String::new());
                                        msgs.push(ChatMsg {
                                            role: "error".into(),
                                            content: err,
                                            footer: String::new(),
                                            edits: Vec::new(),
                                            context: None,
                                        });
                                        sending.set(false);
                                    }
                                }
                            }
                            if changed {
                                messages.set(msgs);
                            }
                        }
                        {
                            let mut st = tw_state_for_render.lock().unwrap();
                            if st.active && st.typed < st.buffer.len() {
                                let now = std::time::Instant::now();
                                if now.duration_since(st.last_tick).as_millis() as u64 >= st.interval_ms {
                                    let next = st.buffer[st.typed..].chars().next();
                                    if let Some(c) = next {
                                        let char_len = c.len_utf8();
                                        st.typed += char_len;
                                        st.last_tick = now;
                                        typewriter.set(st.buffer[..st.typed].to_string());
                                    }
                                }
                            }
                        }
                        messages.get().into_iter().enumerate()
                    },
                    move |(i, _)| { let n = *i as u64; id.store(n, std::sync::atomic::Ordering::Relaxed); n },
                    move |(_, msg)| {
                        let role_color = match msg.role.as_str() {
                            "user" => Color::from_rgb8(100, 180, 255),
                            "error" => Color::from_rgb8(255, 80, 80),
                            "system" => Color::from_rgb8(100, 200, 100),
                            "plan" => Color::from_rgb8(200, 180, 100),
                            "diff" => Color::from_rgb8(180, 200, 100),
                            "context" => Color::from_rgb8(160, 120, 220),
                            "tool" => Color::from_rgb8(120, 220, 200),
                            _ => Color::from_rgb8(200, 200, 200),
                        };
                        let mut content = msg.content.clone();
                        if !msg.footer.is_empty() {
                            content.push_str(&format!("\n\n— {}", msg.footer));
                        }
                        let has_edits = !msg.edits.is_empty();
                        let has_ctx = msg.context.is_some();
                        let ctx_text = msg.context.clone();
                        let edits = msg.edits.clone();
                        let has_mcp_btns = msg.role == "assistant" && !content.is_empty() && !msg.content.is_empty();
                        let apply_edits = msg.edits.clone();
                        let tx_for_btns = tx.clone();
                        stack((
                            label(move || content.clone())
                                .style(move |s| s.font_size(14.0).padding(4.0).width_pct(100.0).color(role_color))
                                .into_any(),
                            container(move || {
                                let ctx = ctx_text.clone();
                                if has_ctx {
                                    let c = ctx.unwrap_or_default();
                                    container(label(move || format!("\u{25B6} RAG Context ({} chars) — click to expand/collapse", c.len()))
                                        .style(|s| s.font_size(11.0).color(Color::from_rgb8(180,160,220)).padding(2.0))
                                        .on_click_stop({ let t = c.clone(); let t2 = c.clone(); move |ev| {
                                            // Toggle visibility simply by inlining full text below toggle (approx).
                                            let _ = t; let _ = t2; let _ = ev;
                                        }})
                                        .into_any()
                                } else {
                                    container(()).into_any()
                                }
                            }).style(|s| s.width_pct(100.0)).into_any(),
                            container(
                                stack((
                                    container(
                                        label(move || t(localization::MsgId::BtnAccept).to_string())
                                            .style(|s| s.font_size(12.0).color(Color::from_rgb8(100, 255, 100)))
                                            .into_any()
                                    )
                                    .style(|s| s.padding_horiz(10.0).padding_vert(4.0)
                                        .background(Color::from_rgb8(30, 80, 30))
                                        .border_radius(4.0).cursor(CursorStyle::Pointer))
                                    .on_click_stop({
                                        let edits = apply_edits.clone();
                                        move |_| {
                                            for edit in &edits {
                                                let result = ai::apply_edit(edit);
                                                tracing::info!(?result, "Edit applied");
                                            }
                                        }
                                    }),
                                    container(
                                        label(move || t(localization::MsgId::BtnReject).to_string())
                                            .style(|s| s.font_size(12.0).color(Color::from_rgb8(255, 100, 100)))
                                            .into_any()
                                    )
                                    .style(|s| s.padding_horiz(10.0).padding_vert(4.0)
                                        .background(Color::from_rgb8(80, 30, 30))
                                        .border_radius(4.0).margin_left(6.0).cursor(CursorStyle::Pointer)),
                                ))
                                .style(|s| s.flex_row().padding(4.0).gap(4.0)),
                            )
                            .style(move |s| {
                                if has_edits {
                                    s.width_pct(100.0).flex_col()
                                } else {
                                    s.width_pct(100.0).flex_col().display(Display::None)
                                }
                            }),
                            container(
                                stack((
                                    container(
                                        label(|| "Scan Security".to_string())
                                            .style(|s| s.font_size(12.0).color(Color::from_rgb8(255, 220, 100)))
                                            .into_any()
                                    )
                                    .style(|s| s.padding_horiz(10.0).padding_vert(4.0)
                                        .background(Color::from_rgb8(80, 60, 30))
                                        .border_radius(4.0).cursor(CursorStyle::Pointer))
                                    .on_click_stop({
                                        let tx = tx_for_btns.clone();
                                        move |_| {
                                            let _ = edits;
                                            let tx = tx.clone();
                                            let busyb = tool_busy.clone();
                                            busyb.set(true);
                                            std::thread::spawn(move || {
                                                let rt = tokio::runtime::Builder::new_current_thread()
                                                    .enable_time().build().ok();
                                                if let Some(rt) = rt {
                                                    let report = rt.block_on(ai::mcp_security_scan("."));
                                                    let _ = tx.send(ChatResult::Tool {
                                                        name: "Security Scan".into(),
                                                        output: report.unwrap_or_else(|| ai::security_scan(".").to_string()),
                                                    });
                                                }
                                            });
                                        }
                                    }),
                                    container(
                                        label(|| "Run Tests".to_string())
                                            .style(|s| s.font_size(12.0).color(Color::from_rgb8(100, 220, 255)))
                                            .into_any()
                                    )
                                    .style(|s| s.padding_horiz(10.0).padding_vert(4.0)
                                        .background(Color::from_rgb8(30, 60, 80))
                                        .border_radius(4.0).margin_left(6.0).cursor(CursorStyle::Pointer))
                                    .on_click_stop({
                                        let tx = tx_for_btns.clone();
                                        move |_| {
                                            let _ = edits;
                                            let tx = tx.clone();
                                            let busyb = tool_busy.clone();
                                            busyb.set(true);
                                            std::thread::spawn(move || {
                                                let rt = tokio::runtime::Builder::new_current_thread()
                                                    .enable_time().build().ok();
                                                if let Some(rt) = rt {
                                                    let report = rt.block_on(ai::mcp_run_tests());
                                                    let _ = tx.send(ChatResult::Tool {
                                                        name: "Tests".into(),
                                                        output: report.unwrap_or_else(|| ai::cargo_check().to_string()),
                                                    });
                                                }
                                            });
                                        }
                                    }),
                                    container(
                                        label(|| "Apply via Carp".to_string())
                                            .style(|s| s.font_size(12.0).color(Color::from_rgb8(100, 255, 200)))
                                            .into_any()
                                    )
                                    .style(|s| s.padding_horiz(10.0).padding_vert(4.0)
                                        .background(Color::from_rgb8(30, 80, 60))
                                        .border_radius(4.0).margin_left(6.0).cursor(CursorStyle::Pointer))
                                    .on_click_stop({
                                        let edits = apply_edits.clone();
                                        let tx = tx_for_btns.clone();
                                        move |_| {
                                            for edit in &edits {
                                                let target = edit.file_path.to_string_lossy().to_string();
                                                let report = ai::precise_edit(&target, &edit.original, &edit.modified);
                                                let _ = report;
                                                let _ = tx.send(ChatResult::Diff { edits: edits.clone() });
                                            }
                                        }
                                    }),
                                ))
                                .style(|s| s.flex_row().padding(4.0).gap(4.0)),
                            )
                            .style(move |s| {
                                if has_mcp_btns {
                                    s.width_pct(100.0).flex_col()
                                } else {
                                    s.width_pct(100.0).flex_col().display(Display::None)
                                }
                            }),
                        ))
                        .style(|s| s.width_pct(100.0).flex_col())
                        .into_any()
                    },
                ).style(|s| s.width_pct(100.0)),
            // Streaming indicator
            container(
                label(move || {
                    if !sending.get() { return String::new(); }
                    let buf = typewriter.get();
                    if buf.is_empty() { t(localization::MsgId::StatusThinking).to_string() }
                    else { format!("{}▌", buf) }
                })
                .style(|s| s.font_size(14.0).padding(4.0).width_pct(100.0).color(Color::from_rgb8(150,150,150)))
                .into_any()
            ).style(|s| s.width_pct(100.0)),
        ))
        .style(|s| s.flex_col().width_pct(100.0).flex_grow(1.0))
    }),

        // ── Input bar ──
        stack((
            text_input(input_text)
                .placeholder(t(localization::MsgId::PlaceholderInput).to_string())
                .style(|s| s.width_pct(83.0).min_height(36.0).padding(6.0).border(1.0).border_color(Color::from_rgb8(60,100,180)))
                .on_event(floem::event::EventListener::KeyUp, {
                    let input_text = input_for_send.clone();
                    let messages = messages_for_response.clone();
                    let sending = sending_for_response.clone();
                    let stream_buf = stream_buf.clone();
                    let agent_sig = agent_for_async.clone();
                    let sid = sid_for_async.clone();
                    let tx = tx.clone();
                    let typewriter = typewriter.clone();
                    let tw_state_for_enter = tw_state_for_enter.clone();
                    move |ev| {
                        if let floem::event::Event::KeyUp(ke) = ev {
                            // ── Tab: slash command autocomplete ──
                            if ke.key.logical_key == floem::keyboard::Key::Named(floem::keyboard::NamedKey::Tab) {
                                let current = input_text.get();
                                if current.starts_with('/') {
                                    if let Some(completed) = tab_complete_slash(&current) {
                                        if completed != current {
                                            input_text.set(completed);
                                        }
                                    }
                                }
                                return floem::event::EventPropagation::Stop;
                            }
                            if ke.key.logical_key == floem::keyboard::Key::Named(floem::keyboard::NamedKey::Enter)
                                && !ke.modifiers.contains(floem::keyboard::Modifiers::SHIFT)
                            {
                                let text = input_text.get().trim().to_string();
                                if text.is_empty() || sending.get() {
                                    return floem::event::EventPropagation::Continue;
                                }
                                input_text.set(String::new());
                                sending.set(true);
                                typewriter.set(String::new());
                                {
                                    let mut st = tw_state_for_enter.lock().unwrap();
                                    st.buffer.clear();
                                    st.typed = 0;
                                    st.active = false;
                                }
                                stream_buf.set(String::new());

                                // ── /clear ──
                                if text == "/clear" {
                                    let mut msgs = messages.get();
                                    msgs.push(ChatMsg { role: "system".into(), content: t(localization::MsgId::StatusSessionCleared).into(), footer: String::new(), edits: Vec::new() });
                                    messages.set(msgs);
                                    agent_sig.set(None);
                                    sending.set(false);
                                    return floem::event::EventPropagation::Stop;
                                }
                                // ── /language <zh|en> ──
                                if text.starts_with("/language ") {
                                    let lang = text.strip_prefix("/language ").unwrap_or("").trim();
                                    let (locale, name) = match lang {
                                        "zh" | "中文" => (Locale::ZhCN, "简体中文"),
                                        "en" | "english" => (Locale::En, "English"),
                                        _ => {
                                            let mut msgs = messages.get();
                                            msgs.push(ChatMsg { role: "system".into(), content: format!("Usage: /language zh|en (current: {:?})", localization::locale()), footer: String::new(), edits: Vec::new() });
                                            messages.set(msgs);
                                            sending.set(false);
                                            return floem::event::EventPropagation::Stop;
                                        }
                                    };
                                    localization::set_locale(locale);
                                    let mut msgs = messages.get();
                                    msgs.push(ChatMsg { role: "system".into(), content: format!("Language: {} ({:?})", name, locale), footer: String::new(), edits: Vec::new() });
                                    messages.set(msgs);
                                    sending.set(false);
                                    return floem::event::EventPropagation::Stop;
                                }
                                // ── /help ──
                                if text == "/help" {
                                    let mut content = String::from("## Available Commands\n\n");
                                    for cmd in SLASH_COMMANDS {
                                        let desc = if localization::locale() == Locale::ZhCN { cmd.description_zh } else { cmd.description_en };
                                        content.push_str(&format!("`{}` — {}\n", cmd.name, desc));
                                    }
                                    content.push_str("\n**Tip**: Type `/` to see completions, press **Tab** to autocomplete.");
                                    let mut msgs = messages.get();
                                    msgs.push(ChatMsg { role: "system".into(), content, footer: String::new(), edits: Vec::new() });
                                    messages.set(msgs);
                                    sending.set(false);
                                    return floem::event::EventPropagation::Stop;
                                }
                                // ── /stats ──
                                if text == "/stats" {
                                    let rag_info = {
                                        let h = ai::hub();
                                        let rag = h.rag.read().unwrap();
                                        rag.as_ref().map(|r| format!(", RAG chunks: {}", r.code_index().chunk_count())).unwrap_or_default()
                                    };
                                    let stats = format!("Session: {}\nAgent: {}\nMsgs: {}{}",
                                        sid, agent_sig.with_untracked(|a| a.is_some()), messages.with_untracked(|m| m.len()), rag_info);
                                    let mut msgs = messages.get();
                                    msgs.push(ChatMsg { role: "system".into(), content: stats, footer: String::new(), edits: Vec::new() });
                                    messages.set(msgs);
                                    sending.set(false);
                                    return floem::event::EventPropagation::Stop;
                                }
                                // ── /plan ──
                                if text.starts_with("/plan ") {
                                    let task = text.strip_prefix("/plan ").unwrap_or(&text).to_string();
                                    let hub = ai::hub();
                                    let prompt = ai::plan_mode_prompt(&task);
                                    match hub.plan.create(&task, &prompt) {
                                        Ok(plan) => {
                                            let tasks = hub.plan.extract_tasks(&plan.content);
                                            let tl: String = tasks.iter().enumerate().map(|(i,t)| format!("  {}. {}", i+1, t)).collect::<Vec<_>>().join("\n");
                                            let mut msgs = messages.get();
                                            msgs.push(ChatMsg { role: "plan".into(), content: format!("\u{1F4CB} Plan: {}\n{}\n\nSteps:\n{}", plan.title, plan.content, tl), footer: format!("slug: {}", plan.slug), edits: Vec::new() });
                                            messages.set(msgs);
                                        }
                                        Err(e) => { let mut msgs = messages.get(); msgs.push(ChatMsg { role: "error".into(), content: format!("Plan: {}", e), footer: String::new(), edits: Vec::new() }); messages.set(msgs); }
                                    }
                                    sending.set(false);
                                    return floem::event::EventPropagation::Stop;
                                }
                                // ── /swarm ──
                                if text.starts_with("/swarm ") {
                                    let task = text.strip_prefix("/swarm ").unwrap_or(&text).to_string();
                                    let hub = ai::hub();
                                    let st = hub.swarm.decompose(&task, &["backend","frontend","reviewer","tester"]);
                                    let tl: String = st.iter().map(|t| format!("  \u{2022} [{}] {} (c:{})", t.required_role.as_deref().unwrap_or("any"), t.description, t.complexity)).collect::<Vec<_>>().join("\n");
                                    let mut msgs = messages.get();
                                    msgs.push(ChatMsg { role: "system".into(), content: format!("\u{1F41D} {} sub-tasks:\n{}\n\nUse /swarm-run to execute.", st.len(), tl), footer: String::new(), edits: Vec::new() });
                                    messages.set(msgs);
                                    sending.set(false);
                                    return floem::event::EventPropagation::Stop;
                                }
                                // ── /swarm-run <task> ──
                                if text.starts_with("/swarm-run ") {
                                    let task = text.strip_prefix("/swarm-run ").unwrap_or(&text).trim().to_string();
                                    let mut msgs = messages.get();
                                    msgs.push(ChatMsg { role: "system".into(), content: format!("Running swarm on: {}", task), footer: String::new(), edits: Vec::new() });
                                    messages.set(msgs);

                                    let tx2 = tx.clone();
                                    std::thread::spawn(move || {
                                        let rt = tokio::runtime::Builder::new_current_thread()
                                            .enable_time().build().expect("tokio");
                                        rt.block_on(async {
                                            match ai::swarm_execute(&task).await {
                                                Ok(report) => {
                                                    let _ = tx2.send(ChatResult::Response {
                                                        content: report,
                                                        footer: "swarm execution".into(),
                                                    });
                                                }
                                                Err(e) => {
                                                    let _ = tx2.send(ChatResult::Error(format!("Swarm: {}", e)));
                                                }
                                            }
                                        });
                                    });
                                    return floem::event::EventPropagation::Stop;
                                }
                                // ── /swarm-status ──
                                if text == "/swarm-status" {
                                    let status = ai::swarm_status();
                                    let mut msgs = messages.get();
                                    msgs.push(ChatMsg { role: "system".into(), content: status, footer: String::new(), edits: Vec::new() });
                                    messages.set(msgs);
                                    sending.set(false);
                                    return floem::event::EventPropagation::Stop;
                                }
                                // ── /metrics ──
                                if text == "/metrics" {
                                    let report = ai::metrics_report();
                                    let mut msgs = messages.get();
                                    msgs.push(ChatMsg { role: "system".into(), content: report, footer: String::new(), edits: Vec::new() });
                                    messages.set(msgs);
                                    sending.set(false);
                                    return floem::event::EventPropagation::Stop;
                                }
                                // ── /mcp ──
                                if text == "/mcp" {
                                    let status = ai::mcp_status();
                                    let mut msgs = messages.get();
                                    msgs.push(ChatMsg { role: "system".into(), content: status, footer: String::new(), edits: Vec::new() });
                                    messages.set(msgs);
                                    sending.set(false);
                                    return floem::event::EventPropagation::Stop;
                                }
                                // ── /apply ──
                                if text == "/apply" {
                                    // Find the last assistant message with edits and apply all
                                    let msgs = messages.get();
                                    let tx2 = tx.clone();
                                    if let Some(last) = msgs.iter().rev().find(|m| m.role == "assistant" && !m.edits.is_empty()) {
                                        let edits = last.edits.clone();
                                        std::thread::spawn(move || {
                                            for edit in &edits {
                                                let result = ai::apply_edit(edit);
                                                tracing::info!(?result, "Edit applied via /apply");
                                            }
                                            let _ = tx2.send(ChatResult::Diff { edits });
                                        });
                                        sending.set(false);
                                    } else {
                                        let mut msgs = messages.get();
                                        msgs.push(ChatMsg { role: "system".into(), content: "No pending edits to apply.".into(), footer: String::new(), edits: Vec::new() });
                                        messages.set(msgs);
                                        sending.set(false);
                                    }
                                    return floem::event::EventPropagation::Stop;
                                }

                                // ── /execute <slug> ──
                                if text.starts_with("/execute ") {
                                    let slug = text.strip_prefix("/execute ").unwrap_or(&text).trim().to_string();
                                    if slug.is_empty() {
                                        let mut msgs = messages.get();
                                        msgs.push(ChatMsg { role: "system".into(), content: "Usage: /execute <plan-slug>".into(), footer: String::new(), edits: Vec::new() });
                                        messages.set(msgs);
                                        sending.set(false);
                                        return floem::event::EventPropagation::Stop;
                                    }
                                    let mut msgs = messages.get();
                                    msgs.push(ChatMsg { role: "system".into(), content: format!("Executing plan '{}'...", slug), footer: String::new(), edits: Vec::new() });
                                    messages.set(msgs);

                                    let sid2 = sid.to_string();
                                    let tx2 = tx.clone();
                                    let slug2 = slug.clone();
                                    std::thread::spawn(move || {
                                        let rt = tokio::runtime::Builder::new_current_thread()
                                            .enable_time().build().expect("tokio");
                                        rt.block_on(async {
                                            match ai::execute_plan_by_slug(&slug2, &sid2).await {
                                                Ok(report) => {
                                                    let _ = tx2.send(ChatResult::Response {
                                                        content: report,
                                                        footer: format!("plan: {}", slug2),
                                                    });
                                                }
                                                Err(e) => {
                                                    let _ = tx2.send(ChatResult::Error(format!("Execute: {}", e)));
                                                }
                                            }
                                        });
                                    });
                                    return floem::event::EventPropagation::Stop;
                                }
                                // ── /list-plans ──
                                if text == "/list-plans" {
                                    let plans = ai::list_plans();
                                    let content = if plans.is_empty() {
                                        "No saved plans. Use /plan <task> to create one.".into()
                                    } else {
                                        format!("Saved plans:\n{}", plans.iter().map(|s| format!("  • {}", s)).collect::<Vec<_>>().join("\n"))
                                    };
                                    let mut msgs = messages.get();
                                    msgs.push(ChatMsg { role: "system".into(), content, footer: String::new(), edits: Vec::new() });
                                    messages.set(msgs);
                                    sending.set(false);
                                    return floem::event::EventPropagation::Stop;
                                }
                                // ── /constitution ──
                                if text == "/constitution" {
                                    let content = ai::constitution();
                                    let mut msgs = messages.get();
                                    msgs.push(ChatMsg { role: "system".into(), content, footer: String::new(), edits: Vec::new() });
                                    messages.set(msgs);
                                    sending.set(false);
                                    return floem::event::EventPropagation::Stop;
                                }
                                // ── /permission <mode> ──
                                if text.starts_with("/permission ") {
                                    let mode_str = text.strip_prefix("/permission ").unwrap_or("").trim();
                                    let content = match mode_str {
                                        "auto" | "accept" => { ai::set_permission_mode(deepseek_carp::agent::PermissionMode::AutoAccept); "Permission: Auto-accept all".into() }
                                        "plan" => { ai::set_permission_mode(deepseek_carp::agent::PermissionMode::Plan); "Permission: Plan first, then execute".into() }
                                        "strict" => { ai::set_permission_mode(deepseek_carp::agent::PermissionMode::Strict); "Permission: Strict — ask for everything".into() }
                                        "default" => { ai::set_permission_mode(deepseek_carp::agent::PermissionMode::Default); "Permission: Default — ask for destructive".into() }
                                        _ => format!("Unknown mode '{}'. Use: default, auto, plan, strict", mode_str),
                                    };
                                    let mut msgs = messages.get();
                                    msgs.push(ChatMsg { role: "system".into(), content, footer: String::new(), edits: Vec::new() });
                                    messages.set(msgs);
                                    sending.set(false);
                                    return floem::event::EventPropagation::Stop;
                                }
                                if text == "/permission" {
                                    let mode = ai::permission_mode();
                                    let mut msgs = messages.get();
                                    msgs.push(ChatMsg { role: "system".into(), content: format!("Current permission mode: {:?}", mode), footer: String::new(), edits: Vec::new() });
                                    messages.set(msgs);
                                    sending.set(false);
                                    return floem::event::EventPropagation::Stop;
                                }
                                // ── /snapshot <label> ──
                                if text.starts_with("/snapshot ") {
                                    let label = text.strip_prefix("/snapshot ").unwrap_or("").trim();
                                    let content = match ai::git_snapshot(label) {
                                        Ok(msg) => msg,
                                        Err(e) => format!("Snapshot failed: {}", e),
                                    };
                                    let mut msgs = messages.get();
                                    msgs.push(ChatMsg { role: "system".into(), content, footer: String::new(), edits: Vec::new() });
                                    messages.set(msgs);
                                    sending.set(false);
                                    return floem::event::EventPropagation::Stop;
                                }
                                // ── /restore <turn> ──
                                if text.starts_with("/restore ") {
                                    let turn_str = text.strip_prefix("/restore ").unwrap_or("").trim();
                                    let content = match turn_str.parse::<u32>() {
                                        Ok(turn) => match ai::git_restore(turn) {
                                            Ok(msg) => msg,
                                            Err(e) => format!("Restore failed: {}", e),
                                        },
                                        Err(_) => "Usage: /restore <turn_number>".into(),
                                    };
                                    let mut msgs = messages.get();
                                    msgs.push(ChatMsg { role: "system".into(), content, footer: String::new(), edits: Vec::new() });
                                    messages.set(msgs);
                                    sending.set(false);
                                    return floem::event::EventPropagation::Stop;
                                }
                                // ── /checkpoint <file> ──
                                if text.starts_with("/checkpoint ") {
                                    let file = text.strip_prefix("/checkpoint ").unwrap_or("").trim();
                                    let content = match ai::checkpoint_save(file) {
                                        Ok(msg) => msg,
                                        Err(e) => format!("Checkpoint failed: {}", e),
                                    };
                                    let mut msgs = messages.get();
                                    msgs.push(ChatMsg { role: "system".into(), content, footer: String::new(), edits: Vec::new() });
                                    messages.set(msgs);
                                    sending.set(false);
                                    return floem::event::EventPropagation::Stop;
                                }
                                // ── /seam ──
                                if text == "/seam" {
                                    let status = ai::seam_status();
                                    let mut msgs = messages.get();
                                    msgs.push(ChatMsg { role: "system".into(), content: status, footer: String::new(), edits: Vec::new() });
                                    messages.set(msgs);
                                    sending.set(false);
                                    return floem::event::EventPropagation::Stop;
                                }
                                // ── /cost ──
                                if text == "/cost" {
                                    let breakdown = ai::cost_breakdown();
                                    let mut msgs = messages.get();
                                    msgs.push(ChatMsg { role: "system".into(), content: breakdown, footer: String::new(), edits: Vec::new() });
                                    messages.set(msgs);
                                    sending.set(false);
                                    return floem::event::EventPropagation::Stop;
                                }
                                // ── /diff-review ──
                                if text == "/diff-review" {
                                    // Review the last AI response for code edits
                                    let msgs = messages.get();
                                    let last_ai = msgs.iter().rev().find(|m| m.role == "assistant");
                                    let content = match last_ai {
                                        Some(msg) => {
                                            let session = ai::diff_session_from_response(&msg.content);
                                            if session.active {
                                                format!(
                                                    "📝 Diff Review: {} edit(s) found.\n\
                                                     Current ({}/{}): {}\n\n\
                                                     Use /accept or /reject to process edits.",
                                                    session.remaining() + session.accepted.len() + session.rejected_count,
                                                    session.selected + 1,
                                                    session.remaining(),
                                                    session.current_edit()
                                                        .map(|e| e.file_path.display().to_string())
                                                        .unwrap_or_else(|| "none".into())
                                                )
                                            } else {
                                                "No code edits found in the last AI response.".into()
                                            }
                                        }
                                        None => "No AI messages to review yet.".into(),
                                    };
                                    let mut msgs = messages.get();
                                    msgs.push(ChatMsg { role: "system".into(), content, footer: String::new(), edits: Vec::new() });
                                    messages.set(msgs);
                                    sending.set(false);
                                    return floem::event::EventPropagation::Stop;
                                }
                                // ── /session ──
                                if text == "/session" {
                                    let stats = ai::session_stats_display();
                                    let cost = ai::cost_breakdown();
                                    let content = format!("Session Info:\n{}\n{}", stats, cost);
                                    let mut msgs = messages.get();
                                    msgs.push(ChatMsg { role: "system".into(), content, footer: String::new(), edits: Vec::new() });
                                    messages.set(msgs);
                                    sending.set(false);
                                    return floem::event::EventPropagation::Stop;
                                }
                                // ── /compile ──
                                if text == "/compile" {
                                    let mut msgs = messages.get();
                                    msgs.push(ChatMsg { role: "system".into(), content: "Running cargo check with auto-fix (3 attempts)...".into(), footer: String::new(), edits: Vec::new() });
                                    messages.set(msgs);

                                    let sid2 = sid.to_string();
                                    let tx2 = tx.clone();
                                    std::thread::spawn(move || {
                                        let rt = tokio::runtime::Builder::new_current_thread()
                                            .enable_time().build().expect("tokio");
                                        rt.block_on(async {
                                            match ai::cargo_compile_auto_fix(&sid2).await {
                                                Ok(report) => {
                                                    let _ = tx2.send(ChatResult::Response {
                                                        content: report,
                                                        footer: "cargo check (auto-fix)".into(),
                                                    });
                                                }
                                                Err(e) => {
                                                    let _ = tx2.send(ChatResult::Error(format!("Compile: {}", e)));
                                                }
                                            }
                                        });
                                    });
                                    return floem::event::EventPropagation::Stop;
                                }
                                // ── /browser <url> ──
                                if text.starts_with("/browser ") {
                                    let url = text.strip_prefix("/browser ").unwrap_or("").trim();
                                    let content = match ai::browser_fetch(url) {
                                        Ok(text) => text,
                                        Err(e) => format!("Browser fetch failed: {}", e),
                                    };
                                    let mut msgs = messages.get();
                                    msgs.push(ChatMsg { role: "system".into(), content, footer: format!("url: {}", url), edits: Vec::new() });
                                    messages.set(msgs);
                                    sending.set(false);
                                    return floem::event::EventPropagation::Stop;
                                }

                                // ── Normal message (RAG-enriched + MCP parallel retrieval) ──
                                let mut msgs = messages.get();
                                msgs.push(ChatMsg { role: "user".into(), content: text.clone(), footer: String::new(), edits: Vec::new(), context: None });
                                messages.set(msgs);

                                let mut prompt = ai::enrich_prompt(&text);
                                let mentions = extract_file_mentions(&text);
                                if !mentions.is_empty() {
                                    prompt.push_str(&read_mentioned_files(&mentions));
                                }
                                let sid2 = sid.to_string();
                                let needs_init = agent_sig.with_untracked(|a| a.is_none());
                                let tx2 = tx.clone();
                                let user_query = text.clone();

                                std::thread::spawn(move || {
                                    let rt = tokio::runtime::Builder::new_current_thread()
                                        .enable_time().build().expect("tokio");

                                    // ── Parallel MCP context_retrieve ──
                                    let (ctx_tx, ctx_rx) = std::sync::mpsc::channel::<Option<String>>();
                                    std::thread::spawn(move || {
                                        let local_rt = tokio::runtime::Builder::new_current_thread()
                                            .enable_time().build().ok();
                                        if let Some(rt) = local_rt {
                                            let _ = ctx_tx.send(rt.block_on(ai::mcp_context_retrieve(&user_query)));
                                        } else { let _ = ctx_tx.send(None); }
                                    });

                                    let hub = ai::hub();
                                    let mcp_ctx = match ctx_rx.recv_timeout(std::time::Duration::from_millis(800)) {
                                        Ok(Some(c)) => {
                                            let _ = tx2.send(ChatResult::Context { text: c.clone() });
                                            Some(c)
                                        }
                                        _ => None,
                                    };

                                    let mut final_prompt = prompt.clone();
                                    if let Some(ref c) = mcp_ctx {
                                        final_prompt.push_str(&format!("\n\n## MCP Retrieved Context\n\n{c}\n\n"));
                                    }

                                    // ── Phase 1: try streaming ──
                                    match hub.query.stream_submit(&final_prompt) {
                                        Ok(mut rx) => {
                                            let mut full = String::new();
                                            let mut provider = String::from("?");
                                            while let Some(chunk) = rx.recv().await {
                                                full.push_str(&chunk.content);
                                                if !chunk.provider.is_empty() {
                                                    provider = chunk.provider.clone();
                                                }
                                                let done = chunk.is_done;
                                                let _ = tx2.send(ChatResult::Stream {
                                                    text: full.clone(),
                                                    done,
                                                    provider: provider.clone(),
                                                });
                                                if done {
                                                    let _ = tx2.send(ChatResult::Response {
                                                        content: full, footer: format!("stream • {}", provider),
                                                        context: mcp_ctx,
                                                    });
                                                    return;
                                                }
                                            }
                                        }
                                        Err(_e) => {}
                                    }

                                    // ── Phase 2: agent fallback ──
                                    if needs_init {
                                        if let Err(e) = ai::spawn_session(&sid2).await {
                                            let _ = tx2.send(ChatResult::Error(format!("Init: {}", e)));
                                            return;
                                        }
                                    }
                                    let hub = ai::hub();
                                    match hub.coordinator.spawn_agent(Default::default(), Some(&sid2)).await {
                                        Ok(mut agent) => match agent.process(&final_prompt).await {
                                            Ok(r) => {
                                                let mut footer = format!("{} • {} tokens", r.provider, r.total_tokens);
                                                if !r.tools_used.is_empty() {
                                                    footer.push_str(&format!(" • [{}]", r.tools_used.join(", ")));
                                                }
                                                ai::save_session(&sid2, agent.history());
                                                let _ = tx2.send(ChatResult::Response {
                                                    content: r.content, footer, context: mcp_ctx,
                                                });
                                            }
                                            Err(e) => { let _ = tx2.send(ChatResult::Error(format!("Agent: {}", e))); }
                                        },
                                        Err(e) => { let _ = tx2.send(ChatResult::Error(format!("Session: {}", e))); }
                                    }
                                });
                                return floem::event::EventPropagation::Stop;
                            }
                        }
                        floem::event::EventPropagation::Continue
                    }
                }),
            container(label(move || if sending.get() { t(localization::MsgId::StatusSending).to_string() } else { t(localization::MsgId::BtnSend).to_string() }).style(|s| s.color(Color::WHITE).font_size(14.0)))
                .style(|s| s.width_pct(15.0).min_height(36.0).items_center().justify_center()
                    .background(Color::from_rgb8(30,100,200)).border_radius(4.0).margin_left(6.0).cursor(CursorStyle::Pointer)),
        ))
        .style(|s| s.flex_row().width_pct(100.0).padding(8.0).items_center().border_top(1.0).border_color(Color::from_rgb8(60,60,60))),
        // ── Slash command completion hint ──
        container(
            label(move || {
                let text = input_text.get();
                let completions = slash_completions(&text);
                if completions.is_empty() {
                    String::new()
                } else {
                    completions.iter()
                        .map(|c| format!("{} — {}", c.name, {
                            if localization::locale() == Locale::ZhCN { c.description_zh } else { c.description_en }
                        }))
                        .collect::<Vec<_>>()
                        .join("  |  ")
                }
            })
            .style(|s| s.font_size(11.0).padding_horiz(8.0).padding_vert(2.0)
                .color(Color::from_rgb8(120, 160, 200)).width_pct(100.0))
        )
        .style(|s| s.width_pct(100.0)),
    ))
    .style(|s| s.size_pct(100.0, 100.0).flex_col())
    .debug_name(format!("dscarp chat {}", session_id))
}

// ── Slash Command Registry ───────────────────────────────────────

/// A registered slash command with metadata for autocomplete.
struct SlashCmd {
    name: &'static str,
    args: &'static str,      // e.g. "<slug>" or "" for no-arg commands
    description_en: &'static str,
    description_zh: &'static str,
}

/// All registered slash commands with descriptions for autocomplete.
static SLASH_COMMANDS: &[SlashCmd] = &[
    SlashCmd { name: "/clear",        args: "",       description_en: "Clear session",                         description_zh: "清除会话" },
    SlashCmd { name: "/language",     args: "<zh|en>",description_en: "Switch language (zh/en)",               description_zh: "切换语言 (zh/en)" },
    SlashCmd { name: "/stats",        args: "",       description_en: "Show session stats",                    description_zh: "会话统计" },
    SlashCmd { name: "/plan",         args: "<task>", description_en: "Create execution plan",                 description_zh: "创建执行计划" },
    SlashCmd { name: "/execute",      args: "<slug>", description_en: "Execute a plan by slug",                description_zh: "按计划执行" },
    SlashCmd { name: "/list-plans",   args: "",       description_en: "List saved plans",                      description_zh: "列出计划" },
    SlashCmd { name: "/swarm",        args: "<task>", description_en: "Decompose into sub-tasks",              description_zh: "分解子任务" },
    SlashCmd { name: "/swarm-run",    args: "<task>", description_en: "Execute swarm in parallel",             description_zh: "并行执行" },
    SlashCmd { name: "/swarm-status", args: "",       description_en: "Show swarm agent status",               description_zh: "Swarm状态" },
    SlashCmd { name: "/compile",      args: "",       description_en: "Cargo check + auto-fix (3 attempts)",   description_zh: "编译+自动修复" },
    SlashCmd { name: "/metrics",      args: "",       description_en: "Show AI metrics report",                description_zh: "AI指标" },
    SlashCmd { name: "/constitution", args: "",       description_en: "Show AI Constitution",                  description_zh: "AI准则" },
    SlashCmd { name: "/permission",   args: "<mode>", description_en: "Set permission (default/auto/plan/strict)", description_zh: "权限模式" },
    SlashCmd { name: "/snapshot",     args: "<label>",description_en: "Create git snapshot",                   description_zh: "Git快照" },
    SlashCmd { name: "/restore",      args: "<turn>", description_en: "Restore git snapshot by turn number",   description_zh: "恢复快照" },
    SlashCmd { name: "/checkpoint",   args: "<file>", description_en: "Save SHA256 file checkpoint",           description_zh: "文件检查点" },
    SlashCmd { name: "/seam",         args: "",       description_en: "Show layered context status",           description_zh: "分层上下文" },
    SlashCmd { name: "/browser",      args: "<url>",  description_en: "Fetch URL content",                     description_zh: "获取网页" },
    SlashCmd { name: "/apply",        args: "",       description_en: "Apply pending code edits",              description_zh: "应用编辑" },
    SlashCmd { name: "/mcp",          args: "",       description_en: "Show MCP connection status",            description_zh: "MCP状态" },
    SlashCmd { name: "/cost",         args: "",       description_en: "Show API cost breakdown (USD)",         description_zh: "API成本" },
    SlashCmd { name: "/diff-review",  args: "",       description_en: "Review AI edits (accept/reject each)",   description_zh: "审查AI编辑" },
    SlashCmd { name: "/session",      args: "",       description_en: "Show session stats (messages/tokens)",   description_zh: "会话统计" },
];

/// Get completions for a partial slash command input.
/// Returns matching commands sorted by relevance (exact prefix match first).
pub fn slash_completions(partial: &str) -> Vec<&'static SlashCmd> {
    let input = partial.trim_start();
    if input.is_empty() || !input.starts_with('/') {
        return Vec::new();
    }

    // Find prefix matches and substring matches
    let mut exact: Vec<&SlashCmd> = Vec::new();
    let mut substring: Vec<&SlashCmd> = Vec::new();

    for cmd in SLASH_COMMANDS {
        if cmd.name == input {
            // Exact match — no need for completion
            return Vec::new();
        }
        if cmd.name.starts_with(input) {
            exact.push(cmd);
        } else if cmd.name.contains(input) || cmd.description_en.to_lowercase().contains(&input[1..].to_lowercase())
            || cmd.description_zh.contains(&input[1..])
        {
            substring.push(cmd);
        }
    }

    exact.extend(substring);
    exact.truncate(8); // Max 8 suggestions
    exact
}

/// Try to tab-complete the current input to the longest common prefix.
/// Returns Some(completed_text) if completion succeeded.
pub fn tab_complete_slash(input: &str) -> Option<String> {
    let completions = slash_completions(input);
    if completions.is_empty() {
        return None;
    }
    if completions.len() == 1 {
        // Single match — complete fully
        let cmd = completions[0];
        Some(if cmd.args.is_empty() {
            cmd.name.to_string()
        } else {
            format!("{} ", cmd.name)
        })
    } else {
        // Multiple matches — extend to longest common prefix
        let mut lcp = completions[0].name.to_string();
        for cmd in &completions[1..] {
            lcp = longest_common_prefix(&lcp, cmd.name);
        }
        if lcp.len() > input.len() {
            Some(if lcp == input { lcp } else { lcp })
        } else {
            None
        }
    }
}

fn longest_common_prefix(a: &str, b: &str) -> String {
    a.chars().zip(b.chars())
        .take_while(|(ac, bc)| ac == bc)
        .map(|(ac, _)| ac)
        .collect()
}

// ── @mention File Injection ──────────────────────────────────────

/// Extract @mentioned file paths from a message.
/// Supports patterns: @path/to/file.rs, @"path with spaces/file.rs"
pub fn extract_file_mentions(text: &str) -> Vec<String> {
    let mut mentions = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '@' && (i == 0 || chars[i - 1].is_whitespace()
            || ['(', '[', '{', '<', '"', '\''].contains(&chars[i - 1]))
        {
            let start = i + 1;
            let mut end = start;

            // Handle quoted paths: @"path/to/file.rs"
            if start < chars.len() && chars[start] == '"' {
                let quote_start = start + 1;
                let mut quote_end = quote_start;
                while quote_end < chars.len() && chars[quote_end] != '"' {
                    quote_end += 1;
                }
                if quote_end > quote_start {
                    let path: String = chars[quote_start..quote_end].iter().collect();
                    if !path.is_empty() && path.chars().any(|c| c == '/' || c == '\\' || c == '.') {
                        mentions.push(path);
                    }
                }
                i = quote_end + 1;
                continue;
            }

            // Unquoted path: stop at whitespace or punctuation
            while end < chars.len() && !chars[end].is_whitespace()
                && ![')', ']', '}', '>', '"', '\'', ',', ';'].contains(&chars[end])
            {
                end += 1;
            }
            if end > start {
                let path: String = chars[start..end].iter().collect();
                if path.chars().any(|c| c == '/' || c == '\\' || c == '.') {
                    mentions.push(path);
                }
            }
            i = end;
        } else {
            i += 1;
        }
    }
    mentions
}

/// Read contents of @mentioned files and format as context block.
/// Capped at 16KB per file, max 5 files.
pub fn read_mentioned_files(mentions: &[String]) -> String {
    if mentions.is_empty() {
        return String::new();
    }

    let mut context = String::from("\n\n## @mentioned files\n");
    let max_files = 5;
    let max_size: usize = 16 * 1024; // 16KB per file

    for (i, path) in mentions.iter().take(max_files).enumerate() {
        match std::fs::read_to_string(path) {
            Ok(content) => {
                let truncated = if content.len() > max_size {
                    format!("{}...\n[truncated: {} bytes]", &content[..max_size], content.len())
                } else {
                    content
                };
                context.push_str(&format!("\n### {}\n```\n{}\n```\n", path, truncated));
            }
            Err(e) => {
                context.push_str(&format!("\n### {} (error: {})\n", path, e));
            }
        }

        if i < mentions.len().min(max_files) - 1 {
            context.push('\n');
        }
    }

    if mentions.len() > max_files {
        context.push_str(&format!("\n... and {} more files\n", mentions.len() - max_files));
    }

    context
}