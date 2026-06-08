use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

use floem::{
    IntoView, View,
    reactive::{RwSignal, SignalGet, SignalUpdate, SignalWith},
    style::{CursorStyle, Display},
    views::{container, dyn_stack, label, scroll, stack, text_input, Decorators},
};
use floem::peniko::Color;

use super::position::PanelPosition;
use crate::ai;
use crate::localization::{self, Locale, t};
use crate::window_tab::WindowTabData;

#[derive(Clone)]
struct CopilotMsg {
    role: String,
    content: String,
}

#[derive(Clone)]
enum CopilotResult {
    Stream { text: String, done: bool, provider: String },
    Response { content: String, footer: String },
    Error(String),
}

#[derive(Clone, Copy, PartialEq)]
enum CopilotStatus {
    Connected,
    Thinking,
    Error,
}

#[derive(Clone)]
struct TypewriterState {
    buffer: String,
    typed: usize,
    interval_ms: u64,
    active: bool,
    last_tick: std::time::Instant,
}

static COPILOT_SESSION_COUNTER: AtomicUsize = AtomicUsize::new(0);
fn next_copilot_session_id() -> String {
    let n = COPILOT_SESSION_COUNTER.fetch_add(1, Ordering::SeqCst);
    format!("copilot-{}", n)
}

struct QuickAction {
    label: &'static str,
    icon: &'static str,
    prompt_fn: fn(&str) -> String,
}

static QUICK_ACTIONS: &[QuickAction] = &[
    QuickAction { label: "Explain", icon: "\u{1F4DD}", prompt_fn: |sel| format!("Explain this code:\n```\n{}\n```", sel) },
    QuickAction { label: "Fix", icon: "\u{1F41E}", prompt_fn: |_| "Fix all errors in the current file. Show what changed and why.".into() },
    QuickAction { label: "Refactor", icon: "\u{1F504}", prompt_fn: |sel| format!("Refactor this selection for clarity and performance:\n```\n{}\n```", sel) },
    QuickAction { label: "Test", icon: "\u{1F4A0}", prompt_fn: |sel| format!("Generate unit tests for this code:\n```\n{}\n```", sel) },
];

static COPILLOT_SLASH_CMDS: &[(&str, &str, &str)] = &[
    ("/plan",   "Create execution plan",     "创建执行计划"),
    ("/swarm",  "Decompose into sub-tasks",  "分解子任务"),
    ("/debug",  "Debug current issue",       "调试当前问题"),
];

pub fn copilot_sidebar(
    _window_tab_data: Rc<WindowTabData>,
    _position: PanelPosition,
) -> impl View {
    let session_id = next_copilot_session_id();
    let status: RwSignal<CopilotStatus> = RwSignal::new(CopilotStatus::Connected);
    let messages: RwSignal<Vec<CopilotMsg>> = RwSignal::new(vec![CopilotMsg {
        role: "system".into(),
        content: if localization::locale() == Locale::ZhCN {
            "\u{1F916} DeepSeek Carp — AI 编程助手\n\n选择代码后点击快捷操作，或直接输入问题。"
        } else {
            "\u{1F916} DeepSeek Carp — AI Coding Assistant\n\nSelect code and use quick actions, or type a question."
        }
        .to_string(),
    }]);
    let input_text = RwSignal::new(String::new());
    let sending = RwSignal::new(false);
    let stream_buf: RwSignal<String> = RwSignal::new(String::new());
    let typewriter: RwSignal<String> = RwSignal::new(String::new());
    let tw_state = std::sync::Arc::new(std::sync::Mutex::new(TypewriterState {
        buffer: String::new(),
        typed: 0,
        interval_ms: 20,
        active: false,
        last_tick: std::time::Instant::now(),
    }));
    let model_name: RwSignal<String> = RwSignal::new("deepseek-chat".into());
    let token_count: RwSignal<u64> = RwSignal::new(0);
    let local_pct: RwSignal<f64> = RwSignal::new(0.0);

    let (tx, rx) = std::sync::mpsc::sync_channel::<CopilotResult>(8);
    let rx = std::sync::Arc::new(std::sync::Mutex::new(rx));

    let rx_for_render = rx.clone();
    let input_for_send = input_text.clone();
    let messages_for_response = messages.clone();
    let sending_for_response = sending.clone();
    let tw_state_for_render = tw_state.clone();
    let tw_state_for_enter = tw_state.clone();
    let status_for_ui = status.clone();

    stack((
        // ── Header ──
        container(
            stack((
                container(
                    label(move || {
                        let s = status_for_ui.get();
                        let dot = match s {
                            CopilotStatus::Connected => "\u{1F7E2}",
                            CopilotStatus::Thinking => "\u{1F7E1}",
                            CopilotStatus::Error => "\u{1F534}",
                        };
                        format!("{} DeepSeek Carp", dot)
                    })
                    .style(|s| s.font_size(15.0).font_weight(floem::peniko::FontWeight::BOLD)
                        .color(Color::from_rgb8(220, 220, 230)))
                    .into_any()
                )
                .style(|s| s.flex_grow(1.0).items_center()),
                container(
                    label(|| "\u{2699}".to_string())
                    .style(|s| s.font_size(16.0).color(Color::from_rgb8(140, 140, 160))
                        .cursor(CursorStyle::Pointer))
                    .into_any()
                )
                .style(|s| s.padding(4.0).cursor(CursorStyle::Pointer)),
            ))
            .style(|s| s.flex_row().width_pct(100.0).items_center().padding_horiz(8.0).padding_vert(6.0)
                .border_bottom(1.0).border_color(Color::from_rgb8(45, 45, 55)))
        )
        .style(|s| s.width_pct(100.0)),

        // ── Quick Actions ──
        container(
            stack((
                QUICK_ACTIONS.iter().map(|action| {
                    let action_label = action.label.to_string();
                    let action_icon = action.icon.to_string();
                    let prompt_fn = action.prompt_fn;
                    let msgs = messages.clone();
                    let send = sending.clone();
                    let stat = status.clone();
                    let inp = input_text.clone();
                    let tx_action = tx.clone();
                    let sid_action = session_id.clone();
                    container(
                        label(move || format!("{} {}", action_icon, action_label))
                            .style(|s| s.font_size(12.0).color(Color::from_rgb8(180, 190, 210))
                                .padding_horiz(10.0).padding_vert(5.0))
                            .into_any()
                    )
                    .style(|s| s.background(Color::from_rgb8(50, 52, 65)).border_radius(6.0)
                        .cursor(CursorStyle::Pointer).flex_grow(1.0).items_center().justify_center())
                    .on_click_stop(move |_| {
                        if send.get() { return; }
                        let prompt = prompt_fn("");
                        let mut m = msgs.get();
                        m.push(CopilotMsg { role: "user".into(), content: prompt.clone() });
                        msgs.set(m);
                        send.set(true);
                        stat.set(CopilotStatus::Thinking);
                        inp.set(String::new());
                        let tx2 = tx_action.clone();
                        let sid2 = sid_action.clone();
                        std::thread::spawn(move || {
                            let rt = tokio::runtime::Builder::new_current_thread()
                                .enable_time().build().expect("tokio");
                            rt.block_on(async {
                                let hub = ai::hub();
                                let enriched = ai::enrich_prompt(&prompt);
                                match hub.query.stream_submit(&enriched) {
                                    Ok(mut stream_rx) => {
                                        let mut full = String::new();
                                        let mut provider = String::from("?");
                                        while let Some(chunk) = stream_rx.recv().await {
                                            full.push_str(&chunk.content);
                                            if !chunk.provider.is_empty() { provider = chunk.provider.clone(); }
                                            let done = chunk.is_done;
                                            let _ = tx2.send(CopilotResult::Stream { text: full.clone(), done, provider: provider.clone() });
                                            if done {
                                                let _ = tx2.send(CopilotResult::Response { content: full, footer: format!("stream \u2022 {}", provider) });
                                                return;
                                            }
                                        }
                                    }
                                    Err(_) => {}
                                }
                                match hub.coordinator.spawn_agent(Default::default(), Some(&sid2)).await {
                                    Ok(mut agent) => match agent.process(&enriched).await {
                                        Ok(r) => {
                                            let _ = tx2.send(CopilotResult::Response { content: r.content, footer: format!("{} \u2022 {} tokens", r.provider, r.total_tokens) });
                                        }
                                        Err(e) => { let _ = tx2.send(CopilotResult::Error(format!("{}", e))); }
                                    },
                                    Err(e) => { let _ = tx2.send(CopilotResult::Error(format!("{}", e))); }
                                }
                            });
                        });
                    })
                    .into_any()
                }).collect::<Vec<_>>()
            ))
            .style(|s| s.flex_row().width_pct(100.0).gap(6.0).padding(6.0))
        )
        .style(|s| s.width_pct(100.0).border_bottom(1.0).border_color(Color::from_rgb8(45, 45, 55))),

        // ── Chat Area ──
        scroll({
            let rx_chat = rx_for_render.clone();
            stack((
                dyn_stack(
                    move || {
                        if let Ok(lock) = rx_chat.lock() {
                            let mut msgs = messages.get();
                            let mut changed = false;
                            while let Ok(result) = lock.try_recv() {
                                changed = true;
                                match result {
                                    CopilotResult::Stream { text, done: true, provider } => {
                                        typewriter.set(text.clone());
                                        stream_buf.set(String::new());
                                        msgs.push(CopilotMsg { role: "assistant".into(), content: text, });
                                        typewriter.set(String::new());
                                        {
                                            let mut st = tw_state_for_render.lock().unwrap();
                                            st.buffer.clear(); st.typed = 0; st.active = false;
                                        }
                                        sending.set(false);
                                        status.set(CopilotStatus::Connected);
                                        let _ = provider;
                                    }
                                    CopilotResult::Stream { text, .. } => {
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
                                    CopilotResult::Response { content, footer } => {
                                        typewriter.set(String::new());
                                        { let mut st = tw_state_for_render.lock().unwrap(); st.buffer.clear(); st.typed = 0; st.active = false; }
                                        stream_buf.set(String::new());
                                        let mut display = content.clone();
                                        if !footer.is_empty() { display.push_str(&format!("\n\u2014 {}", footer)); }
                                        msgs.push(CopilotMsg { role: "assistant".into(), content: display });
                                        sending.set(false);
                                        status.set(CopilotStatus::Connected);
                                        let _ = footer;
                                    }
                                    CopilotResult::Error(err) => {
                                        typewriter.set(String::new());
                                        { let mut st = tw_state_for_render.lock().unwrap(); st.buffer.clear(); st.typed = 0; st.active = false; }
                                        stream_buf.set(String::new());
                                        msgs.push(CopilotMsg { role: "error".into(), content: err });
                                        sending.set(false);
                                        status.set(CopilotStatus::Error);
                                    }
                                }
                            }
                            if changed { messages.set(msgs); }
                        }
                        {
                            let mut st = tw_state_for_render.lock().unwrap();
                            if st.active && st.typed < st.buffer.len() {
                                let now = std::time::Instant::now();
                                if now.duration_since(st.last_tick).as_millis() as u64 >= st.interval_ms {
                                    if let Some(c) = st.buffer[st.typed..].chars().next() {
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
                    |(i, _)| *i as u64,
                    move |(_, msg)| {
                        let is_user = msg.role == "user";
                        let is_error = msg.role == "error";
                        let is_system = msg.role == "system";
                        let bg = if is_user { Color::from_rgb8(30, 60, 110) }
                            else if is_error { Color::from_rgb8(80, 30, 30) }
                            else if is_system { Color::from_rgb8(35, 45, 35) }
                            else { Color::from_rgb8(42, 44, 54) };
                        let text_color = if is_user { Color::from_rgb8(160, 200, 255) }
                            else if is_error { Color::from_rgb8(255, 120, 120) }
                            else if is_system { Color::from_rgb8(140, 200, 140) }
                            else { Color::from_rgb8(200, 200, 210) };
                        let align_items = if is_user { floem::reactive::style::AlignItems::FlexEnd } else { floem::reactive::style::AlignItems::FlexStart };
                        let max_width = if is_user { 85.0 } else { 95.0 };
                        container(
                            label(move || msg.content.clone())
                                .style(move |s| s.font_size(13.0).padding(8.0).width_pct(max_width)
                                    .color(text_color).line_height(1.5))
                                .into_any()
                        )
                        .style(move |s| s.background(bg).border_radius(8.0)
                            .align_items(align_items).margin_vert(2.0)
                            .padding_horiz(if is_user { 12.0 } else { 8.0 }))
                        .into_any()
                    },
                ).style(|s| s.width_pct(100.0)),
                container(
                    label(move || {
                        if !sending.get() { return String::new(); }
                        let buf = typewriter.get();
                        if buf.is_empty() { "\u25CF".to_string() } else { format!("{}\u258C", buf) }
                    })
                    .style(|s| s.font_size(13.0).padding(6.0).width_pct(92.0)
                        .color(Color::from_rgb8(160, 160, 180)))
                    .into_any()
                ).style(|s| s.width_pct(100.0)),
            ))
            .style(|s| s.flex_col().width_pct(100.0).flex_grow(1.0).padding(6.0).gap(4.0))
        })
        .style(|s| s.flex_col().width_pct(100.0).flex_grow(1.0)),

        // ── Slash Command Hint ──
        container(
            label(move || {
                let text = input_text.get();
                if !text.starts_with('/') { return String::new(); }
                COPILLOT_SLASH_CMDS.iter()
                    .filter(|(cmd, _, _)| cmd.starts_with(&text) || text.starts_with(cmd))
                    .map(|(cmd, en, zh)| {
                        if localization::locale() == Locale::ZhCN { format!("{} — {}", cmd, zh) }
                        else { format!("{} — {}", cmd, en) }
                    })
                    .collect::<Vec<_>>()
                    .join("   ")
            })
            .style(|s| s.font_size(11.0).padding_horiz(8.0).padding_vert(2.0)
                .color(Color::from_rgb8(120, 150, 200)).width_pct(100.0))
        )
        .style(|s| s.width_pct(100.0).min_height(18.0)),

        // ── Input Bar ──
        stack((
            text_input(input_text)
                .placeholder(if localization::locale() == Locale::ZhCN { "输入问题或 /plan /swarm /debug ..." } else { "Ask or /plan /swarm /debug ..." })
                .style(|s| s.width_pct(82.0).min_height(32.0).padding(6.0).border(1.0)
                    .border_color(Color::from_rgb8(55, 85, 155)).border_radius(6.0))
                .on_event(floem::event::EventListener::KeyUp, {
                    let input_text = input_for_send.clone();
                    let messages = messages_for_response.clone();
                    let sending = sending_for_response.clone();
                    let stream_buf = stream_buf.clone();
                    let typewriter = typewriter.clone();
                    let tw_state = tw_state_for_enter.clone();
                    let stat = status.clone();
                    let tx_input = tx.clone();
                    let sid_input = session_id.clone();
                    let mdl = model_name.clone();
                    let tkn = token_count.clone();
                    move |ev| {
                        if let floem::event::Event::KeyUp(ke) = ev {
                            if ke.key.logical_key == floem::keyboard::Key::Named(floem::keyboard::NamedKey::Enter)
                                && !ke.modifiers.contains(floem::keyboard::Modifiers::SHIFT)
                            {
                                let text = input_text.get().trim().to_string();
                                if text.is_empty() || sending.get() {
                                    return floem::event::EventPropagation::Continue;
                                }
                                input_text.set(String::new());
                                sending.set(true);
                                stat.set(CopilotStatus::Thinking);
                                typewriter.set(String::new());
                                { let mut st = tw_state.lock().unwrap(); st.buffer.clear(); st.typed = 0; st.active = false; }
                                stream_buf.set(String::new());

                                let mut msgs = messages.get();
                                msgs.push(CopilotMsg { role: "user".into(), content: text.clone() });
                                messages.set(msgs);

                                let enriched = ai::enrich_prompt(&text);
                                let sid2 = sid_input.clone();
                                let tx2 = tx_input.clone();
                                let mdl2 = mdl.clone();
                                let tkn2 = tkn.clone();
                                std::thread::spawn(move || {
                                    let rt = tokio::runtime::Builder::new_current_thread()
                                        .enable_time().build().expect("tokio");
                                    rt.block_on(async {
                                        let hub = ai::hub();
                                        mdl2.set(format!("cloud:{}", "deepseek-chat"));
                                        match hub.query.stream_submit(&enriched) {
                                            Ok(mut stream_rx) => {
                                                let mut full = String::new();
                                                let mut provider = String::from("?");
                                                while let Some(chunk) = stream_rx.recv().await {
                                                    full.push_str(&chunk.content);
                                                    if !chunk.provider.is_empty() { provider = chunk.provider.clone(); }
                                                    let done = chunk.is_done;
                                                    let _ = tx2.send(CopilotResult::Stream { text: full.clone(), done, provider: provider.clone() });
                                                    if done {
                                                        tkn2.set(full.split_whitespace().count() as u64 * 4 / 3);
                                                        let _ = tx2.send(CopilotResult::Response { content: full, footer: format!("stream \u2022 {}", provider) });
                                                        return;
                                                    }
                                                }
                                            }
                                            Err(_) => {}
                                        }
                                        match hub.coordinator.spawn_agent(Default::default(), Some(&sid2)).await {
                                            Ok(mut agent) => match agent.process(&enriched).await {
                                                Ok(r) => {
                                                    tkn2.set(r.total_tokens);
                                                    let _ = tx2.send(CopilotResult::Response { content: r.content, footer: format!("{} \u2022 {} tokens", r.provider, r.total_tokens) });
                                                }
                                                Err(e) => { let _ = tx2.send(CopilotResult::Error(format!("{}", e))); }
                                            },
                                            Err(e) => { let _ = tx2.send(CopilotResult::Error(format!("{}", e))); }
                                        }
                                    });
                                });
                                return floem::event::EventPropagation::Stop;
                            }
                        }
                        floem::event::EventPropagation::Continue
                    }
                }),
            container(
                label(move || if sending.get() { "\u23F3".to_string() } else { "\u27A4".to_string() })
                    .style(|s| s.color(Color::WHITE).font_size(14.0))
                    .into_any()
            )
            .style(|s| s.width_pct(16.0).min_height(32.0).items_center().justify_center()
                .background(Color::from_rgb8(40, 90, 180)).border_radius(6.0).margin_left(6.0)
                .cursor(CursorStyle::Pointer)),
        ))
        .style(|s| s.flex_row().width_pct(100.0).padding(6.0).items_center()
            .border_top(1.0).border_color(Color::from_rgb8(45, 45, 55))),

        // ── Status Footer ──
        container(
            label(move || {
                let mdl = model_name.get();
                let tkn = token_count.get();
                let lcl = local_pct.get();
                if localization::locale() == Locale::ZhCN {
                    format!("{} | {} tokens | 本地 {:.0}%", mdl, tkn, lcl * 100.0)
                } else {
                    format!("{} | {} tokens | local {:.0}%", mdl, tkn, lcl * 100.0)
                }
            })
            .style(|s| s.font_size(10.0).color(Color::from_rgb8(100, 105, 125))
                .padding_horiz(8.0).padding_vert(2.0))
        )
        .style(|s| s.width_pct(100.0)),
    ))
    .style(|s| s.size_pct(100.0, 100.0).flex_col().background(Color::from_rgb8(28, 29, 36)))
    .debug_name(format!("dscarp copilot sidebar {}", session_id))
}
