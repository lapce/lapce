//! Plan artifacts — OpenSpec-style visual/planning document generators.
//!
//! When the Agent is in Plan mode (see `plan_mode_prompt`), it should not
//! only produce a markdown task list but also a set of artifacts that a
//! human reviewer (or IDE panel) can inspect visually:
//!
//! ```text
//! Plan (Agent analyzes requirement)
//!  │
//!  ├── Architecture Diagram   (Mermaid Flowchart)      → architecture.mmd
//!  ├── API Schema             (OpenAPI 3 JSON)         → api.openapi.json
//!  ├── Sequence Diagram       (Mermaid Sequence)        → sequence.mmd
//!  └── UI Prototype           (single-HTML wireframe)   → prototype.html
//! ```
//!
//! These mirrors the OpenSpec / HagiCode "planning direction" system
//! (github.com/Fission-AI/OpenSpec), but lives natively in
//! deepseek-carp so the Lapce glue layer can render them directly.

use serde::{Serialize, Deserialize};

/// An artifact type produced alongside a plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    ArchitectureFlowchart,
    OpenApiSchema,
    SequenceDiagram,
    UiPrototype,
}

impl ArtifactKind {
    pub fn file_ext(self) -> &'static str {
        match self {
            ArtifactKind::ArchitectureFlowchart => "mmd",
            ArtifactKind::OpenApiSchema        => "openapi.json",
            ArtifactKind::SequenceDiagram      => "mmd",
            ArtifactKind::UiPrototype          => "html",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ArtifactKind::ArchitectureFlowchart => "Architecture Diagram (Mermaid Flowchart)",
            ArtifactKind::OpenApiSchema        => "API Schema (OpenAPI 3 JSON)",
            ArtifactKind::SequenceDiagram      => "Sequence Diagram (Mermaid)",
            ArtifactKind::UiPrototype          => "UI Prototype (HTML wireframe)",
        }
    }

    pub fn filename(self, slug: &str) -> String {
        match self {
            ArtifactKind::ArchitectureFlowchart => format!("{slug}.architecture.mmd"),
            ArtifactKind::OpenApiSchema        => format!("{slug}.api.openapi.json"),
            ArtifactKind::SequenceDiagram      => format!("{slug}.sequence.mmd"),
            ArtifactKind::UiPrototype          => format!("{slug}.prototype.html"),
        }
    }
}

/// An artifact produced by the plan phase.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanArtifact {
    pub kind: ArtifactKind,
    pub title: String,
    pub body: String,
}

impl PlanArtifact {
    pub fn new(kind: ArtifactKind, title: impl Into<String>, body: impl Into<String>) -> Self {
        Self { kind, title: title.into(), body: body.into() }
    }

    pub fn save(&self, dir: &std::path::Path, slug: &str) -> std::io::Result<std::path::PathBuf> {
        std::fs::create_dir_all(dir).ok();
        let path = dir.join(self.kind.filename(slug));
        std::fs::write(&path, &self.body)?;
        Ok(path)
    }
}

/// ────────────────────────────────────────────────────────────────────────
/// 1. Mermaid Flowchart builder
/// ────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct FlowchartNode {
    pub id: &'static str,
    pub label: String,
    pub shape: FlowchartShape,
    pub subgraph: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FlowchartShape {
    #[default]
    Box,
    Round,
    Stadium,
    Rhombus,
    Cylinder,
    Subroutine,
}

impl FlowchartShape {
    fn open(self) -> &'static str {
        match self {
            FlowchartShape::Box        => "[",
            FlowchartShape::Round      => "(",
            FlowchartShape::Stadium    => "([",
            FlowchartShape::Rhombus    => "{",
            FlowchartShape::Cylinder   => "[(",
            FlowchartShape::Subroutine => "[[",
        }
    }
    fn close(self) -> &'static str {
        match self {
            FlowchartShape::Box        => "]",
            FlowchartShape::Round      => ")",
            FlowchartShape::Stadium    => "])",
            FlowchartShape::Rhombus    => "}",
            FlowchartShape::Cylinder   => ")]",
            FlowchartShape::Subroutine => "]]",
        }
    }
}

#[derive(Debug, Clone)]
pub struct FlowchartEdge {
    pub from: &'static str,
    pub to:   &'static str,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct MermaidFlowchart {
    pub direction: FlowchartDir,
    pub nodes: Vec<FlowchartNode>,
    pub edges: Vec<FlowchartEdge>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FlowchartDir {
    #[default] TD, LR, BT, RL,
}

impl MermaidFlowchart {
    pub fn render(&self) -> String {
        let dir = match self.direction {
            FlowchartDir::TD => "TD",
            FlowchartDir::LR => "LR",
            FlowchartDir::BT => "BT",
            FlowchartDir::RL => "RL",
        };

        let mut out = String::from("```mermaid\n");
        out.push_str(&format!("flowchart {dir}\n"));

        let mut subgraphs: Vec<&str> = Vec::new();
        for n in &self.nodes {
            if let Some(sg) = n.subgraph {
                if !subgraphs.contains(&sg) { subgraphs.push(sg); }
            }
        }
        for sg in subgraphs {
            out.push_str(&format!("subgraph {sg}\n"));
            for n in &self.nodes {
                if n.subgraph == Some(sg) {
                    let (o, c) = (n.shape.open(), n.shape.close());
                    out.push_str(&format!("  {} {}{}{}{}\n", n.id, o, n.label.replace(['"', '\n'], ""), c,
                        if matches!(n.shape, FlowchartShape::Rhombus) { "" } else { "" }));
                }
            }
            out.push_str("end\n");
        }
        for n in &self.nodes {
            if n.subgraph.is_none() {
                let (o, c) = (n.shape.open(), n.shape.close());
                out.push_str(&format!("{} {}{}{}\n", n.id, o, n.label.replace(['"', '\n'], ""), c));
            }
        }

        for e in &self.edges {
            match &e.label {
                Some(l) => out.push_str(&format!("{} -->|\"{}\"| {}\n", e.from, l.replace(['|'], "│"), e.to)),
                None    => out.push_str(&format!("{} --> {}\n", e.from, e.to)),
            }
        }
        out.push_str("```\n");
        out
    }
}

/// ────────────────────────────────────────────────────────────────────────
/// 2. OpenAPI 3.0 JSON skeleton (lightweight — we emit literal JSON)
/// ────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct OpenApiPath {
    pub method: &'static str,
    pub path: String,
    pub summary: String,
    pub description: String,
    pub request_body: Option<&'static str>,
    pub responses: Vec<(u16, &'static str)>,
}

#[derive(Debug, Clone, Default)]
pub struct OpenApiSchema {
    pub title: String,
    pub version: String,
    pub base_url: String,
    pub paths: Vec<OpenApiPath>,
    pub components: Vec<(&'static str, &'static str)>,
}

impl OpenApiSchema {
    pub fn render_json(&self) -> String {
        let mut out = String::from("{\n");
        out.push_str("  \"openapi\": \"3.0.3\",\n");
        out.push_str("  \"info\": {\n");
        out.push_str(&format!("    \"title\": \"{}\",\n", self.title.replace('"', "\\\"")));
        out.push_str(&format!("    \"version\": \"{}\"\n", self.version));
        out.push_str("  },\n");
        if !self.base_url.is_empty() {
            out.push_str("  \"servers\": [{ \"url\": \"");
            out.push_str(&self.base_url.replace('"', "\\\""));
            out.push_str("\" }],\n");
        }
        out.push_str("  \"paths\": {\n");
        for (i, p) in self.paths.iter().enumerate() {
            let comma = if i + 1 == self.paths.len() { "" } else { "," };
            out.push_str(&format!("    \"{}\": {{\n", p.path.replace('"', "\\\"")));
            out.push_str(&format!("      \"{}\": {{\n", p.method.to_lowercase()));
            out.push_str(&format!("        \"summary\": \"{}\",\n", p.summary.replace(['"', '\n'], "")));
            out.push_str(&format!("        \"description\": \"{}\",\n", p.description.replace(['"', '\n'], " ")));
            if let Some(rb) = p.request_body {
                out.push_str("        \"requestBody\": {\n");
                out.push_str("          \"required\": true,\n");
                out.push_str("          \"content\": { \"application/json\": { ");
                out.push_str(&format!("\"schema\": {{ \"$ref\": \"#/components/schemas/{}\" }}", rb));
                out.push_str(" } }\n        },\n");
            }
            out.push_str("        \"responses\": {\n");
            for (ri, (code, desc)) in p.responses.iter().enumerate() {
                let rcomma = if ri + 1 == p.responses.len() { "" } else { "," };
                out.push_str(&format!("          \"{}\": {{ \"description\": \"{}\" }}{}\n",
                    code, desc.replace(['"', '\n'], ""), rcomma));
            }
            out.push_str("        }\n");
            out.push_str(&format!("      }}\n    }}{}\n", comma));
        }
        out.push_str("  },\n");
        out.push_str("  \"components\": { \"schemas\": {\n");
        for (i, (name, schema_json)) in self.components.iter().enumerate() {
            let comma = if i + 1 == self.components.len() { "" } else { "," };
            out.push_str(&format!("    \"{name}\": {schema_json}{comma}\n"));
        }
        out.push_str("  } }\n}\n");
        out
    }
}

/// ────────────────────────────────────────────────────────────────────────
/// 3. Mermaid Sequence Diagram
/// ────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct SeqParticipant {
    pub alias: &'static str,
    pub label: String,
    pub kind: SeqParticipantKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeqParticipantKind {
    Actor, Participant, Database,
}

#[derive(Debug, Clone)]
pub struct SeqMessage {
    pub from: &'static str,
    pub to:   &'static str,
    pub arrow: SeqArrow,
    pub label: String,
    pub activate_to: bool,
    pub deactivate_from: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeqArrow {
    Sync,   // ->>
    Async,  // ->
    Return, // -->>
    Note,   // autonote
}

#[derive(Debug, Clone, Default)]
pub struct MermaidSequence {
    pub participants: Vec<SeqParticipant>,
    pub messages: Vec<SeqMessage>,
}

impl MermaidSequence {
    pub fn render(&self) -> String {
        let mut out = String::from("```mermaid\nsequenceDiagram\n");
        for p in &self.participants {
            let kw = match p.kind {
                SeqParticipantKind::Actor      => "actor",
                SeqParticipantKind::Participant=> "participant",
                SeqParticipantKind::Database   => "database",
            };
            out.push_str(&format!("  {kw} {}:{}\n", p.alias, p.label.replace(':', "_")));
        }
        for m in &self.messages {
            let arrow = match m.arrow {
                SeqArrow::Sync   => "->>",
                SeqArrow::Async  => "->",
                SeqArrow::Return => "-->>",
                SeqArrow::Note   => {
                    out.push_str(&format!("  autonote over {}: {}\n",
                        m.from, m.label.replace(['\n'], " ")));
                    continue;
                }
            };
            out.push_str(&format!("  {} {from}{arrow}{to}: {label}",
                if m.activate_to { "activate " } else { "" },
                from = m.from, arrow = arrow, to = m.to,
                label = m.label.replace(['"', '\n'], " ")));
            if m.activate_to {
                out.push_str(&format!("\\nactivate {} ", m.to));
            }
            if m.deactivate_from {
                out.push_str(&format!("\n  deactivate {}", m.from));
            }
            out.push('\n');
        }
        out.push_str("```\n");
        out
    }
}

/// ────────────────────────────────────────────────────────────────────────
/// 4. UI prototype (single-file HTML wireframe)
/// ────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct UiWidget {
    pub widget: &'static str,
    pub label: String,
    pub placeholder: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct UiScreen {
    pub title: String,
    pub layout: String,
    pub widgets: Vec<UiWidget>,
}

#[derive(Debug, Clone, Default)]
pub struct HtmlPrototype {
    pub app_title: String,
    pub screens: Vec<UiScreen>,
}

impl HtmlPrototype {
    pub fn render(&self) -> String {
        let mut html = String::from(
"<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n  <meta charset=\"utf-8\">\n");
        html.push_str(&format!("  <title>{}</title>\n", self.app_title.replace('"', "&quot;")));
        html.push_str(
"  <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\n");
        html.push_str(
"<style>\n");
        html.push_str(":root{bg:#fafafa;fg:#222;border:#d0d0d0;accent:#3b82f6;code:monospace}\n");
        html.push_str("*{box-sizing:border-box;margin:0;padding:0;font-family:ui-sans-serif,system-ui}\n");
        html.push_str("body{background:var(--bg);color:var(--fg);padding:24px}\n");
        html.push_str("h1{font-size:22px;font-weight:600;margin-bottom:8px}\n");
        html.push_str("h2{font-size:16px;font-weight:500;margin:16px 0 8px;color:#555}\n");
        html.push_str(".screen{background:#fff;border:1px solid var(--border);border-radius:10px;padding:20px;margin-bottom:20px;box-shadow:0 1px 2px rgba(0,0,0,.04)}\n");
        html.push_str(".layout{display:flex;gap:16px;flex-wrap:wrap}\n");
        html.push_str(".layout.stack{flex-direction:column;align-items:stretch}\n");
        html.push_str(".widget{border:1px dashed var(--border);padding:10px 12px;border-radius:6px;font-size:13px;color:#444;background:#fcfcfc;min-width:160px}\n");
        html.push_str(".widget.btn{background:var(--accent);color:#fff;border-style:solid;border-color:var(--accent);font-weight:500}\n");
        html.push_str(".widget.input{min-width:220px;background:#fff;border-style:solid}\n");
        html.push_str(".widget.label{border:none;background:transparent;color:#888;font-size:12px;padding:2px}\n");
        html.push_str(".widget.sidebar{flex:1;min-width:200px;min-height:260px;background:#f3f4f6}\n");
        html.push_str(".widget.main  {flex:3;min-width:320px;min-height:260px;background:#fff;border:1px solid var(--border)}\n");
        html.push_str(".tag{display:inline-block;background:#eef2ff;color:#3730a3;font-size:10px;padding:2px 6px;border-radius:4px;margin-right:6px}\n");
        html.push_str("</style>\n</head>\n<body>\n");
        html.push_str(&format!("  <h1>{}</h1>\n", self.app_title.replace('<',"&lt;").replace('>',"&gt;")));
        html.push_str("  <div style=\"margin-bottom:12px;color:#888;font-size:12px\">");
        html.push_str("  ⚠️  Wireframe prototype — not final UI. Rendered from plan artifacts.\n");
        html.push_str("  </div>\n");
        for s in &self.screens {
            let layout = if s.layout == "stack" { "layout stack" } else { "layout" };
            html.push_str(&format!("  <h2>Screen: {}</h2>\n", s.title.replace('<',"&lt;")));
            html.push_str("  <div class=\"screen\">\n");
            html.push_str(&format!("    <div class=\"{}\">\n", layout));
            for w in &s.widgets {
                let cls = match w.widget {
                    "button" | "btn"     => "widget btn",
                    "input" | "textbox"  => "widget input",
                    "label"              => "widget label",
                    "sidebar"            => "widget sidebar",
                    "main"               => "widget main",
                    _                    => "widget",
                };
                html.push_str(&format!(
                    "      <div class=\"{}\">{}{}</div>\n",
                    cls,
                    if w.label.is_empty() { String::new() } else { format!("<span class=\"tag\">{}</span>", w.label) },
                    match w.widget {
                        "input" | "textbox" => format!("<div style=\"color:#aaa;font-size:12px\">{}</div>",
                            w.placeholder.clone().unwrap_or_else(|| "text input".into())),
                        "button" | "btn"    => w.placeholder.clone().unwrap_or_else(|| "Action".into()),
                        "label"              => w.placeholder.clone().unwrap_or_default(),
                        "sidebar"            => "<div style=\"color:#888\">sidebar pane</div>".into(),
                        "main"               => "<div style=\"color:#888\">main content area</div>".into(),
                        _                    => format!("<div style=\"color:#aaa;font-size:12px\">{}</div>",
                            w.placeholder.clone().unwrap_or_default()),
                    }
                ));
            }
            html.push_str("    </div>\n  </div>\n");
        }
        html.push_str("</body>\n</html>\n");
        html
    }
}

/// ────────────────────────────────────────────────────────────────────────
/// 5. All-in-one planner — takes a free-form request and produces 4 artifacts
/// ────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct PlanArtifactBundle {
    pub flowchart: MermaidFlowchart,
    pub api:       OpenApiSchema,
    pub sequence:  MermaidSequence,
    pub prototype: HtmlPrototype,
}

impl PlanArtifactBundle {
    pub fn save_all(&self, dir: &std::path::Path, slug: &str) -> std::io::Result<Vec<std::path::PathBuf>> {
        std::fs::create_dir_all(dir).ok();
        let mut out = Vec::new();

        let flow_path = dir.join(ArtifactKind::ArchitectureFlowchart.filename(slug));
        std::fs::write(&flow_path, self.flowchart.render())?;
        out.push(flow_path);

        let api_path = dir.join(ArtifactKind::OpenApiSchema.filename(slug));
        std::fs::write(&api_path, self.api.render_json())?;
        out.push(api_path);

        let seq_path = dir.join(ArtifactKind::SequenceDiagram.filename(slug));
        std::fs::write(&seq_path, self.sequence.render())?;
        out.push(seq_path);

        let html_path = dir.join(ArtifactKind::UiPrototype.filename(slug));
        std::fs::write(&html_path, self.prototype.render())?;
        out.push(html_path);

        Ok(out)
    }
}

/// Build the prompt that instructs the AI to emit all 4 artifacts in addition
/// to the markdown plan. This replaces the plain prompt in `plan_mode_prompt`.
pub fn openspec_plan_prompt(user_request: &str) -> String {
    format!(
"You are in PLAN MODE with OpenSpec artifact generation.\n\
\n\
User request: {user_request}\n\
\n\
You MUST produce FIVE artifacts and save them to the plan directory:\n\
\n\
1) `ARCHITECTURE.md`  — markdown with a Mermaid **flowchart TD** in a ````mermaid` block.\n\
   Nodes: components/services. Subgraphs: layers (api, service, data, infra).\n\
\n\
2) `API.openapi.json`  — valid OpenAPI 3.0 JSON describing every REST/WebSocket endpoint\n\
   mentioned in the plan. Include paths, methods, request/response schemas.\n\
\n\
3) `SEQUENCE.md`  — markdown with a Mermaid **sequenceDiagram** in a ````mermaid` block\n\
   showing the end-to-end interaction from user input to final result.\n\
\n\
4) `PROTOTYPE.html`  — single-file HTML wireframe (tailwind-style inline CSS is OK)\n\
   showing each screen the user would see: layout, key widgets, states.\n\
\n\
5) `PLAN.md`  — the traditional step-by-step checkbox list (`- [ ] step`).\n\
\n\
Constraints:\n\
- Mermaid blocks MUST render on GitHub, GitLab, Obsidian and Obsidian-like editors.\n\
- OpenAPI JSON MUST parse cleanly with `json.loads` in Python or `serde_json::from_str` in Rust.\n\
- Prototype HTML MUST be self-contained (no external deps) and look correct in a browser.\n\
- If the plan has NO user-facing UI, emit a minimal 2-screen prototype (landing + empty).\n\
- If the plan has NO HTTP API, emit an OpenAPI doc with zero paths but correct structure.\n\
\n\
Save all five files, then print a short summary of what each artifact contains.\n\
Wait for the user to approve before executing.",
        user_request = user_request,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flowchart_renders_mermaid() {
        let f = MermaidFlowchart {
            direction: FlowchartDir::TD,
            nodes: vec![
                FlowchartNode { id: "U", label: "User".into(),       shape: FlowchartShape::Round,     subgraph: None },
                FlowchartNode { id: "A", label: "API Gateway".into(), shape: FlowchartShape::Stadium,    subgraph: Some("Edge") },
                FlowchartNode { id: "S", label: "Service".into(),    shape: FlowchartShape::Box,        subgraph: Some("Core") },
                FlowchartNode { id: "D", label: "Postgres".into(),   shape: FlowchartShape::Cylinder,   subgraph: Some("Core") },
            ],
            edges: vec![
                FlowchartEdge { from: "U", to: "A", label: Some("HTTPS".into()) },
                FlowchartEdge { from: "A", to: "S", label: Some("gRPC".into())  },
                FlowchartEdge { from: "S", to: "D", label: None                 },
            ],
        };
        let rendered = f.render();
        assert!(rendered.contains("```mermaid"));
        assert!(rendered.contains("flowchart TD"));
        assert!(rendered.contains("subgraph Edge"));
        assert!(rendered.contains("subgraph Core"));
        assert!(rendered.contains("A -->"));
    }

    #[test]
    fn openapi_renders_valid_json() {
        let s = OpenApiSchema {
            title: "User Service".into(),
            version: "1.0.0".into(),
            base_url: "/api".into(),
            paths: vec![OpenApiPath {
                method: "get", path: "/users/{id}".into(),
                summary: "Fetch a user".into(),
                description: "Returns the user with the given id.".into(),
                request_body: None,
                responses: vec![(200, "User found"), (404, "Not found")],
            }],
            components: vec![("User", r#"{ "type": "object", "properties": { "id": { "type": "string" } } }"#)],
        };
        let json = s.render_json();
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("render must be valid JSON");
        assert_eq!(parsed["openapi"], "3.0.3");
        assert!(parsed["paths"]["/users/{id}"]["get"]["summary"].is_string());
    }

    #[test]
    fn sequence_renders_mermaid() {
        let mut seq = MermaidSequence::default();
        seq.participants.extend([
            SeqParticipant { alias: "U", label: "User".into(),      kind: SeqParticipantKind::Actor },
            SeqParticipant { alias: "A", label: "Agent".into(),     kind: SeqParticipantKind::Participant },
            SeqParticipant { alias: "DB", label: "Memory".into(),   kind: SeqParticipantKind::Database },
        ]);
        seq.messages.extend([
            SeqMessage { from: "U", to: "A",  arrow: SeqArrow::Sync,   label: "analyze(plan)".into(), activate_to: false, deactivate_from: false },
            SeqMessage { from: "A", to: "DB", arrow: SeqArrow::Sync,   label: "lookup context".into(), activate_to: false, deactivate_from: false },
            SeqMessage { from: "DB", to: "A", arrow: SeqArrow::Return, label: "context".into(),        activate_to: false, deactivate_from: false },
            SeqMessage { from: "A", to: "U", arrow: SeqArrow::Return, label: "plan".into(),           activate_to: false, deactivate_from: false },
        ]);
        let m = seq.render();
        assert!(m.contains("```mermaid"));
        assert!(m.contains("sequenceDiagram"));
        assert!(m.contains("actor U"));
        assert!(m.contains("database DB"));
        assert!(m.contains("->>"));
        assert!(m.contains("U") && m.contains("A"));
    }

    #[test]
    fn prototype_renders_standalone_html() {
        let mut p = HtmlPrototype { app_title: "Chat IDE".into(), ..Default::default() };
        p.screens.push(UiScreen {
            title: "Main Window".into(),
            layout: "horizontal".into(),
            widgets: vec![
                UiWidget { widget: "sidebar", label: "Sidebar".into(), placeholder: None },
                UiWidget { widget: "main",    label: "Editor".into(),  placeholder: None },
                UiWidget { widget: "input",   label: "Chat".into(),    placeholder: Some("Ask anything…".into()) },
                UiWidget { widget: "button",  label: "Send".into(),    placeholder: Some("Run".into()) },
            ],
        });
        let h = p.render();
        assert!(h.starts_with("<!DOCTYPE html>"));
        assert!(h.contains("<title>Chat IDE</title>"));
        assert!(h.contains("Chat"));
        assert!(h.contains("Run"));
        assert!(!h.contains('`'));
    }

    #[test]
    fn artifact_save_works() {
        let dir = std::env::temp_dir().join("dscarp_artifacts");
        let _ = std::fs::remove_dir_all(&dir);
        let body: String = "```mermaid\nsequenceDiagram\n  participant X\n```\n".into();
        let title: String = "Demo".into();
        let art = PlanArtifact::new(ArtifactKind::SequenceDiagram, title, body);
        let p = art.save(&dir, "demo").unwrap();
        assert!(p.exists());
        let content = std::fs::read_to_string(&p).unwrap();
        assert!(content.contains("sequenceDiagram"));
    }

    #[test]
    fn openspec_prompt_mentions_five_artifacts() {
        let p = openspec_plan_prompt("build a todo API");
        assert!(p.contains("ARCHITECTURE.md"));
        assert!(p.contains("API.openapi.json"));
        assert!(p.contains("SEQUENCE.md"));
        assert!(p.contains("PROTOTYPE.html"));
        assert!(p.contains("PLAN.md"));
    }
}
