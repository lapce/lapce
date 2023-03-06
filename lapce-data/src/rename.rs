use std::{path::PathBuf, sync::Arc};

use druid::{Command, EventCtx, ExtEventSink, Target, WidgetId};
use lapce_core::buffer::Buffer;
use lapce_xi_rope::Rope;
use lsp_types::{Position, PrepareRenameResponse};

use crate::{
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    data::LapceMainSplitData,
    document::LocalBufferKind,
};

#[derive(Clone)]
pub struct RenameData {
    pub view_id: WidgetId,
    pub editor_id: WidgetId,
    pub active: bool,
    pub rev: u64,
    /// The path of the file in which the symbol is currently being renamed
    pub path: PathBuf,
    /// The offset that is currently being handled
    pub offset: usize,
    pub position: Position,
    pub from_editor: WidgetId,

    pub start: usize,
    pub end: usize,
    pub placeholder: String,
    pub mouse_within: bool,
}

impl RenameData {
    pub fn new() -> Self {
        Self {
            view_id: WidgetId::next(),
            editor_id: WidgetId::next(),
            active: false,
            rev: 0,
            path: PathBuf::new(),
            offset: 0,
            position: Position::new(0, 0),
            from_editor: WidgetId::next(),
            start: 0,
            end: 0,
            placeholder: "".to_string(),
            mouse_within: false,
        }
    }

    pub fn update(
        &mut self,
        path: PathBuf,
        rev: u64,
        offset: usize,
        position: Position,
        from_editor: WidgetId,
    ) {
        self.active = false;
        self.path = path;
        self.rev = rev;
        self.offset = offset;
        self.from_editor = from_editor;
        self.position = position;
        self.mouse_within = false;
    }

    pub fn cancel(&mut self) {
        self.active = false;
    }

    #[allow(clippy::too_many_arguments)]
    pub fn handle_prepare_rename(
        &mut self,
        ctx: &mut EventCtx,
        main_split: &mut LapceMainSplitData,
        path: PathBuf,
        rev: u64,
        offset: usize,
        start: usize,
        end: usize,
        placeholder: String,
    ) {
        if self.path != path || self.rev != rev || self.offset != offset {
            return;
        }
        self.active = true;
        self.start = start;
        self.end = end;
        self.placeholder = placeholder.clone();

        let doc = main_split
            .local_docs
            .get_mut(&LocalBufferKind::Rename)
            .unwrap();
        Arc::make_mut(doc).reload(Rope::from(placeholder), true);
        let editor = main_split.editors.get_mut(&self.view_id).unwrap();
        let offset = doc.buffer().line_end_offset(0, true);
        Arc::make_mut(editor).cursor.mode = lapce_core::cursor::CursorMode::Insert(
            lapce_core::selection::Selection::region(0, offset),
        );
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::Focus,
            Target::Widget(self.view_id),
        ));
    }

    pub fn prepare_rename(
        tab_id: WidgetId,
        path: PathBuf,
        offset: usize,
        rev: u64,
        buffer: Buffer,
        resp: PrepareRenameResponse,
        event_sink: ExtEventSink,
    ) {
        let (start, end, placeholder) = match resp {
            lsp_types::PrepareRenameResponse::Range(range) => (
                buffer.offset_of_position(&range.start),
                buffer.offset_of_position(&range.end),
                None,
            ),
            lsp_types::PrepareRenameResponse::RangeWithPlaceholder {
                range,
                placeholder,
            } => (
                buffer.offset_of_position(&range.start),
                buffer.offset_of_position(&range.end),
                Some(placeholder),
            ),
            lsp_types::PrepareRenameResponse::DefaultBehavior { .. } => (
                buffer.prev_code_boundary(offset),
                buffer.next_code_boundary(offset),
                None,
            ),
        };
        let placeholder = placeholder.unwrap_or_else(|| {
            let (start, end) = buffer.select_word(offset);
            buffer.slice_to_cow(start..end).to_string()
        });
        let _ = event_sink.submit_command(
            LAPCE_UI_COMMAND,
            LapceUICommand::PrepareRename {
                path,
                rev,
                offset,
                start,
                end,
                placeholder,
            },
            Target::Widget(tab_id),
        );
    }
}

impl Default for RenameData {
    fn default() -> Self {
        Self::new()
    }
}
