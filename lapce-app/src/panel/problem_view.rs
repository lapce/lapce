use std::{path::PathBuf, sync::Arc};

use floem::{
    reactive::{create_memo, SignalWith},
    view::View,
    views::{label, list, stack},
    AppContext,
};
use lsp_types::DiagnosticSeverity;

use crate::{doc::EditorDiagnostic, window_tab::WindowTabData};

use super::view::panel_header;

pub fn problem_panel(cx: AppContext, window_tab_data: Arc<WindowTabData>) {}

fn problem_section(
    cx: AppContext,
    window_tab_data: Arc<WindowTabData>,
    severity: DiagnosticSeverity,
) -> impl View {
    let config = window_tab_data.common.config;
    let diagnostics = window_tab_data.main_split.diagnostics;
    let main_split = window_tab_data.main_split.clone();
    stack(cx, move |cx| {
        (
            panel_header(cx, "Errors".to_string(), config),
            list(
                cx,
                move || main_split.diagnostics_items(severity, true),
                |(p, _)| p.clone(),
                |cx, (path, diagnostics)| file_view(cx, path, diagnostics),
            ),
        )
    })
}

fn file_view(
    cx: AppContext,
    path: PathBuf,
    diagnostics: Vec<EditorDiagnostic>,
) -> impl View {
    stack(cx, move |cx| {
        (label(cx, move || path.to_str().unwrap().to_string()),)
    })
}
