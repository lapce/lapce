use lapce_core::indent::IndentStyle;
use lapce_rpc::buffer::BufferId;
use lsp_types::Position;
use std::borrow::Cow;
use std::cmp::Ordering;
use std::ops::Range;
use std::sync::atomic::{self, AtomicU64};
use std::{collections::BTreeSet, sync::Arc};
use xi_rope::{multiset::Subset, rope::Rope, DeltaBuilder, RopeDelta};
use xi_rope::{Cursor, Delta};

use crate::buffer::{
    char_width, shuffle, shuffle_tombstones, str_col, BufferContent, Contents,
    EditType, InvalLines, Revision, WordCursor,
};
use crate::movement::{ColPosition, Selection};

pub const DEFAULT_INDENT: IndentStyle = IndentStyle::Spaces(4);

pub trait BufferDataListener {
    fn should_apply_edit(&self) -> bool;

    fn on_edit_applied(&mut self, buffer: &BufferData, delta: &RopeDelta);
}

#[derive(Clone)]
pub struct BufferData {
    pub(super) id: BufferId,
    pub(super) rope: Rope,
    pub(super) content: BufferContent,

    pub(super) max_len: usize,
    pub(super) max_len_line: usize,
    pub(super) num_lines: usize,

    pub(super) rev: u64,
    pub(super) atomic_rev: Arc<AtomicU64>,
    pub(super) dirty: bool,

    pub(super) revs: Vec<Revision>,
    pub(super) cur_undo: usize,
    pub(super) undos: BTreeSet<usize>,
    pub(super) undo_group_id: usize,
    pub(super) live_undos: Vec<usize>,
    pub(super) deletes_from_union: Subset,
    pub(super) undone_groups: BTreeSet<usize>,
    pub(super) tombstones: Rope,

    pub(super) last_edit_type: EditType,
    pub(super) indent_style: IndentStyle,
}

impl BufferData {
    pub fn new(str: &str, content: BufferContent) -> Self {
        Self {
            id: BufferId::next(),
            rope: Rope::from(str),
            content,

            max_len: 0,
            max_len_line: 0,
            num_lines: 0,

            rev: 0,
            atomic_rev: Arc::new(AtomicU64::new(0)),
            dirty: false,

            revs: vec![Revision {
                max_undo_so_far: 0,
                edit: Contents::Undo {
                    toggled_groups: BTreeSet::new(),
                    deletes_bitxor: Subset::new(0),
                },
            }],
            cur_undo: 1,
            undos: BTreeSet::new(),
            undo_group_id: 1,
            live_undos: vec![0],
            deletes_from_union: Subset::new(str.len()),
            undone_groups: BTreeSet::new(),
            tombstones: Rope::default(),

            last_edit_type: EditType::Other,
            indent_style: DEFAULT_INDENT,
        }
    }

    pub fn len(&self) -> usize {
        self.rope.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn id(&self) -> BufferId {
        self.id
    }

    pub fn rope(&self) -> &Rope {
        &self.rope
    }

    pub fn content(&self) -> &BufferContent {
        &self.content
    }

    pub fn indent_unit(&self) -> &'static str {
        self.indent_style.as_str()
    }

    pub fn slice_to_cow(&self, range: Range<usize>) -> Cow<str> {
        self.rope
            .slice_to_cow(range.start.min(self.len())..range.end.min(self.len()))
    }

    pub fn offset_line_content(&self, offset: usize) -> Cow<str> {
        let line = self.line_of_offset(offset);
        let start_offset = self.offset_of_line(line);
        self.slice_to_cow(start_offset..offset)
    }

    pub fn select_word(&self, offset: usize) -> (usize, usize) {
        WordCursor::new(&self.rope, offset).select_word()
    }

    pub fn char_at_offset(&self, offset: usize) -> Option<char> {
        if self.is_empty() {
            return None;
        }
        WordCursor::new(&self.rope, offset)
            .inner
            .peek_next_codepoint()
    }

    pub fn indent_on_line(&self, line: usize) -> String {
        let line_start_offset = self.rope.offset_of_line(line);
        let word_boundary =
            WordCursor::new(&self.rope, line_start_offset).next_non_blank_char();
        let indent = self.rope.slice_to_cow(line_start_offset..word_boundary);
        indent.to_string()
    }

    pub fn offset_to_line_col(
        &self,
        offset: usize,
        tab_width: usize,
    ) -> (usize, usize) {
        let max = self.len();
        let offset = if offset > max { max } else { offset };
        let line = self.line_of_offset(offset);
        let line_start = self.offset_of_line(line);
        if offset == line_start {
            return (line, 0);
        }

        let col = str_col(&self.slice_to_cow(line_start..offset), tab_width);
        (line, col)
    }

    pub fn line_end_col(&self, line: usize, caret: bool, tab_width: usize) -> usize {
        let line_start = self.offset_of_line(line);
        let offset = self.line_end_offset(line, caret);
        str_col(&self.slice_to_cow(line_start..offset), tab_width)
    }

    pub fn line_of_offset(&self, offset: usize) -> usize {
        let max = self.len();
        let offset = if offset > max { max } else { offset };
        self.rope.line_of_offset(offset)
    }

    pub fn line_content(&self, line: usize) -> Cow<str> {
        self.slice_to_cow(self.offset_of_line(line)..self.offset_of_line(line + 1))
    }

    pub fn line_end_offset(&self, line: usize, caret: bool) -> usize {
        let mut offset = self.offset_of_line(line + 1);
        let mut line_content: &str = &self.line_content(line);
        if line_content.ends_with("\r\n") {
            offset -= 2;
            line_content = &line_content[..line_content.len() - 2];
        } else if line_content.ends_with('\n') {
            offset -= 1;
            line_content = &line_content[..line_content.len() - 1];
        }
        if !caret && !line_content.is_empty() {
            offset = self.prev_grapheme_offset(offset, 1, 0);
        }
        offset
    }

    pub fn offset_line_end(&self, offset: usize, caret: bool) -> usize {
        let line = self.line_of_offset(offset);
        self.line_end_offset(line, caret)
    }

    pub fn offset_to_position(&self, offset: usize, tab_width: usize) -> Position {
        let (line, col) = self.offset_to_line_col(offset, tab_width);
        Position {
            line: line as u32,
            character: col as u32,
        }
    }

    pub fn offset_of_line_col(
        &self,
        line: usize,
        col: usize,
        tab_width: usize,
    ) -> usize {
        let mut pos = 0;
        let mut offset = self.offset_of_line(line);
        for c in self
            .slice_to_cow(self.offset_of_line(line)..self.offset_of_line(line + 1))
            .chars()
        {
            if c == '\n' {
                return offset;
            }
            let width = if c == '\t' {
                tab_width - pos % tab_width
            } else {
                char_width(c)
            };

            pos += width;
            if pos > col {
                return offset;
            }

            offset += c.len_utf8();
            if pos == col {
                return offset;
            }
        }
        offset
    }

    pub fn first_non_blank_character_on_line(&self, line: usize) -> usize {
        let last_line = self.last_line();
        let line = if line > last_line + 1 {
            last_line
        } else {
            line
        };
        let line_start_offset = self.rope.offset_of_line(line);
        WordCursor::new(&self.rope, line_start_offset).next_non_blank_char()
    }

    pub(super) fn mk_new_rev(
        &self,
        undo_group: usize,
        delta: RopeDelta,
    ) -> (Revision, Rope, Rope, Subset) {
        let (ins_delta, deletes) = delta.factor();

        let deletes_at_rev = &self.deletes_from_union;

        let union_ins_delta = ins_delta.transform_expand(deletes_at_rev, true);
        let mut new_deletes = deletes.transform_expand(deletes_at_rev);

        let new_inserts = union_ins_delta.inserted_subset();
        if !new_inserts.is_empty() {
            new_deletes = new_deletes.transform_expand(&new_inserts);
        }
        let cur_deletes_from_union = &self.deletes_from_union;
        let text_ins_delta =
            union_ins_delta.transform_shrink(cur_deletes_from_union);
        let text_with_inserts = text_ins_delta.apply(&self.rope);
        let rebased_deletes_from_union =
            cur_deletes_from_union.transform_expand(&new_inserts);

        let undone = self.undone_groups.contains(&undo_group);
        let new_deletes_from_union = {
            let to_delete = if undone { &new_inserts } else { &new_deletes };
            rebased_deletes_from_union.union(to_delete)
        };

        let (new_text, new_tombstones) = shuffle(
            &text_with_inserts,
            &self.tombstones,
            &rebased_deletes_from_union,
            &new_deletes_from_union,
        );

        let head_rev = &self.revs.last().unwrap();
        (
            Revision {
                max_undo_so_far: std::cmp::max(undo_group, head_rev.max_undo_so_far),
                edit: Contents::Edit {
                    undo_group,
                    inserts: new_inserts,
                    deletes: new_deletes,
                },
            },
            new_text,
            new_tombstones,
            new_deletes_from_union,
        )
    }

    fn calculate_undo_group(&mut self, edit_type: EditType) -> usize {
        let has_undos = !self.live_undos.is_empty();
        let is_unbroken_group = !edit_type.breaks_undo_group(self.last_edit_type);

        if has_undos && is_unbroken_group {
            *self.live_undos.last().unwrap()
        } else {
            let undo_group = self.undo_group_id;
            self.live_undos.truncate(self.cur_undo);
            self.live_undos.push(undo_group);
            self.cur_undo += 1;
            self.undo_group_id += 1;
            undo_group
        }
    }
    fn deletes_from_union_before_index(
        &self,
        rev_index: usize,
        invert_undos: bool,
    ) -> Cow<Subset> {
        let mut deletes_from_union = Cow::Borrowed(&self.deletes_from_union);
        let mut undone_groups = Cow::Borrowed(&self.undone_groups);

        // invert the changes to deletes_from_union starting in the present and working backwards
        for rev in self.revs[rev_index..].iter().rev() {
            deletes_from_union = match rev.edit {
                Contents::Edit {
                    ref inserts,
                    ref deletes,
                    ref undo_group,
                    ..
                } => {
                    if undone_groups.contains(undo_group) {
                        // no need to un-delete undone inserts since we'll just shrink them out
                        Cow::Owned(deletes_from_union.transform_shrink(inserts))
                    } else {
                        let un_deleted = deletes_from_union.subtract(deletes);
                        Cow::Owned(un_deleted.transform_shrink(inserts))
                    }
                }
                Contents::Undo {
                    ref toggled_groups,
                    ref deletes_bitxor,
                } => {
                    if invert_undos {
                        let new_undone = undone_groups
                            .symmetric_difference(toggled_groups)
                            .cloned()
                            .collect();
                        undone_groups = Cow::Owned(new_undone);
                        Cow::Owned(deletes_from_union.bitxor(deletes_bitxor))
                    } else {
                        deletes_from_union
                    }
                }
            }
        }
        deletes_from_union
    }

    fn find_first_undo_candidate_index(
        &self,
        toggled_groups: &BTreeSet<usize>,
    ) -> usize {
        // find the lowest toggled undo group number
        if let Some(lowest_group) = toggled_groups.iter().cloned().next() {
            for (i, rev) in self.revs.iter().enumerate().rev() {
                if rev.max_undo_so_far < lowest_group {
                    return i + 1; // +1 since we know the one we just found doesn't have it
                }
            }
            0
        } else {
            // no toggled groups, return past end
            self.revs.len()
        }
    }

    fn compute_undo(&self, groups: &BTreeSet<usize>) -> (Revision, Subset) {
        let toggled_groups = self
            .undone_groups
            .symmetric_difference(groups)
            .cloned()
            .collect();
        let first_candidate = self.find_first_undo_candidate_index(&toggled_groups);
        // the `false` below: don't invert undos since our first_candidate is based on the current undo set, not past
        let mut deletes_from_union = self
            .deletes_from_union_before_index(first_candidate, false)
            .into_owned();

        for rev in &self.revs[first_candidate..] {
            if let Contents::Edit {
                ref undo_group,
                ref inserts,
                ref deletes,
                ..
            } = rev.edit
            {
                if groups.contains(undo_group) {
                    if !inserts.is_empty() {
                        deletes_from_union =
                            deletes_from_union.transform_union(inserts);
                    }
                } else {
                    if !inserts.is_empty() {
                        deletes_from_union =
                            deletes_from_union.transform_expand(inserts);
                    }
                    if !deletes.is_empty() {
                        deletes_from_union = deletes_from_union.union(deletes);
                    }
                }
            }
        }

        let deletes_bitxor = self.deletes_from_union.bitxor(&deletes_from_union);
        let max_undo_so_far = self.revs.last().unwrap().max_undo_so_far;
        (
            Revision {
                max_undo_so_far,
                edit: Contents::Undo {
                    toggled_groups,
                    deletes_bitxor,
                },
            },
            deletes_from_union,
        )
    }

    fn apply_edit(
        &mut self,

        delta: &RopeDelta,
        new_rev: Revision,
        new_text: Rope,
        new_tombstones: Rope,
        new_deletes_from_union: Subset,
    ) -> InvalLines {
        self.rev += 1;
        self.atomic_rev.store(self.rev, atomic::Ordering::Release);
        self.dirty = true;

        let (iv, newlen) = delta.summary();
        let old_logical_end_line = self.rope.line_of_offset(iv.end) + 1;

        self.revs.push(new_rev);
        self.rope = new_text;
        self.tombstones = new_tombstones;
        self.deletes_from_union = new_deletes_from_union;

        let logical_start_line = self.rope.line_of_offset(iv.start);
        let new_logical_end_line = self.rope.line_of_offset(iv.start + newlen) + 1;
        let old_hard_count = old_logical_end_line - logical_start_line;
        let new_hard_count = new_logical_end_line - logical_start_line;

        InvalLines {
            start_line: logical_start_line,
            inval_count: old_hard_count,
            new_count: new_hard_count,
        }
    }

    fn update_size(&mut self, inval_lines: &InvalLines) {
        if inval_lines.inval_count != inval_lines.new_count {
            self.num_lines = self.num_lines();
        }
        if self.max_len_line >= inval_lines.start_line
            && self.max_len_line < inval_lines.start_line + inval_lines.inval_count
        {
            let (max_len, max_len_line) = self.get_max_line_len();
            self.max_len = max_len;
            self.max_len_line = max_len_line;
        } else {
            let mut max_len = 0;
            let mut max_len_line = 0;
            for line in inval_lines.start_line
                ..inval_lines.start_line + inval_lines.new_count
            {
                let line_len = self.line_len(line);
                if line_len > max_len {
                    max_len = line_len;
                    max_len_line = line;
                }
            }
            if max_len > self.max_len {
                self.max_len = max_len;
                self.max_len_line = max_len_line;
            } else if self.max_len_line >= inval_lines.start_line {
                self.max_len_line = self.max_len_line + inval_lines.new_count
                    - inval_lines.inval_count;
            }
        }
    }

    pub fn last_line(&self) -> usize {
        self.line_of_offset(self.rope.len())
    }

    pub fn offset_of_line(&self, line: usize) -> usize {
        let last_line = self.last_line();
        let line = if line > last_line + 1 {
            last_line + 1
        } else {
            line
        };
        self.rope.offset_of_line(line)
    }

    pub fn line_len(&self, line: usize) -> usize {
        self.offset_of_line(line + 1) - self.offset_of_line(line)
    }

    pub fn line_horiz_col(
        &self,
        line: usize,
        horiz: &ColPosition,
        caret: bool,
        tab_width: usize,
    ) -> usize {
        match *horiz {
            ColPosition::Col(n) => n.min(self.line_end_col(line, caret, tab_width)),
            ColPosition::End => self.line_end_col(line, caret, tab_width),
            ColPosition::Start => 0,
            ColPosition::FirstNonBlank => {
                self.first_non_blank_character_on_line(line)
            }
        }
    }

    pub fn num_lines(&self) -> usize {
        self.line_of_offset(self.rope.len()) + 1
    }

    pub fn get_max_line_len(&self) -> (usize, usize) {
        let mut pre_offset = 0;
        let mut max_len = 0;
        let mut max_len_line = 0;
        for line in 0..self.num_lines() + 1 {
            let offset = self.rope.offset_of_line(line);
            let line_len = offset - pre_offset;
            pre_offset = offset;
            if line_len > max_len {
                max_len = line_len;
                max_len_line = line;
            }
        }
        (max_len, max_len_line)
    }

    pub(super) fn reset_revs(&mut self) {
        self.rope = Rope::from("");
        self.revs = vec![Revision {
            max_undo_so_far: 0,
            edit: Contents::Undo {
                toggled_groups: BTreeSet::new(),
                deletes_bitxor: Subset::new(0),
            },
        }];
        self.cur_undo = 1;
        self.undo_group_id = 1;
        self.live_undos = vec![0];
        self.deletes_from_union = Subset::new(0);
        self.undone_groups = BTreeSet::new();
        self.tombstones = Rope::default();
    }

    pub fn prev_grapheme_offset(
        &self,
        offset: usize,
        count: usize,
        limit: usize,
    ) -> usize {
        let mut cursor = Cursor::new(&self.rope, offset);
        let mut new_offset = offset;
        for _i in 0..count {
            if let Some(prev_offset) = cursor.prev_grapheme() {
                if prev_offset < limit {
                    return new_offset;
                }
                new_offset = prev_offset;
                cursor.set(prev_offset);
            } else {
                return new_offset;
            }
        }
        new_offset
    }

    pub fn next_grapheme_offset(
        &self,
        offset: usize,
        count: usize,
        limit: usize,
    ) -> usize {
        let offset = if offset > self.len() {
            self.len()
        } else {
            offset
        };
        let mut cursor = Cursor::new(&self.rope, offset);
        let mut new_offset = offset;
        for _i in 0..count {
            if let Some(next_offset) = cursor.next_grapheme() {
                if next_offset > limit {
                    return new_offset;
                }
                new_offset = next_offset;
                cursor.set(next_offset);
            } else {
                return new_offset;
            }
        }
        new_offset
    }
}

/// Make BufferData temporarily editable by attaching a listener object to it.
pub struct EditableBufferData<'a, L> {
    pub listener: L,
    pub buffer: &'a mut BufferData,
}

impl<L: BufferDataListener> EditableBufferData<'_, L> {
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    pub fn indent_unit(&self) -> &'static str {
        self.buffer.indent_unit()
    }

    pub fn offset_of_line(&self, line: usize) -> usize {
        self.buffer.offset_of_line(line)
    }

    pub fn offset_line_end(&self, offset: usize, caret: bool) -> usize {
        self.buffer.offset_line_end(offset, caret)
    }

    pub fn offset_to_line_col(
        &self,
        offset: usize,
        tab_width: usize,
    ) -> (usize, usize) {
        self.buffer.offset_to_line_col(offset, tab_width)
    }

    pub fn line_of_offset(&self, offset: usize) -> usize {
        self.buffer.line_of_offset(offset)
    }

    pub fn first_non_blank_character_on_line(&self, line: usize) -> usize {
        self.buffer.first_non_blank_character_on_line(line)
    }

    pub fn edit_multiple(
        &mut self,
        edits: &[(&Selection, &str)],
        edit_type: EditType,
    ) -> RopeDelta {
        let mut builder = DeltaBuilder::new(self.len());
        let mut interval_rope = Vec::new();
        for (selection, content) in edits {
            let rope = Rope::from(content);
            for region in selection.regions() {
                interval_rope.push((region.min(), region.max(), rope.clone()));
            }
        }
        interval_rope.sort_by(|a, b| {
            if a.0 == b.0 && a.1 == b.1 {
                Ordering::Equal
            } else if a.1 == b.0 {
                Ordering::Less
            } else {
                a.1.cmp(&b.0)
            }
        });
        for (start, end, rope) in interval_rope.into_iter() {
            builder.replace(start..end, rope);
        }
        let delta = builder.build();
        let undo_group = self.buffer.calculate_undo_group(edit_type);
        self.buffer.last_edit_type = edit_type;

        let (new_rev, new_text, new_tombstones, new_deletes_from_union) =
            self.buffer.mk_new_rev(undo_group, delta.clone());

        if self.listener.should_apply_edit() {
            self.apply_edit(
                &delta,
                new_rev,
                new_text,
                new_tombstones,
                new_deletes_from_union,
            );
        }

        delta
    }

    pub fn do_undo(&mut self) -> Option<RopeDelta> {
        if self.buffer.cur_undo > 1 {
            self.buffer.cur_undo -= 1;
            self.buffer
                .undos
                .insert(self.buffer.live_undos[self.buffer.cur_undo]);
            self.buffer.last_edit_type = EditType::Undo;
            Some(self.undo(self.buffer.undos.clone()))
        } else {
            None
        }
    }

    pub fn do_redo(&mut self) -> Option<RopeDelta> {
        if self.buffer.cur_undo < self.buffer.live_undos.len() {
            self.buffer
                .undos
                .remove(&self.buffer.live_undos[self.buffer.cur_undo]);
            self.buffer.cur_undo += 1;
            self.buffer.last_edit_type = EditType::Redo;
            Some(self.undo(self.buffer.undos.clone()))
        } else {
            None
        }
    }

    fn apply_edit(
        &mut self,
        delta: &RopeDelta,
        new_rev: Revision,
        new_text: Rope,
        new_tombstones: Rope,
        new_deletes_from_union: Subset,
    ) {
        let inval_lines = self.buffer.apply_edit(
            delta,
            new_rev,
            new_text,
            new_tombstones,
            new_deletes_from_union,
        );

        self.buffer.update_size(&inval_lines);
        self.listener.on_edit_applied(&self.buffer, delta);
    }

    fn undo(&mut self, groups: BTreeSet<usize>) -> RopeDelta {
        let (new_rev, new_deletes_from_union) = self.buffer.compute_undo(&groups);
        let delta = Delta::synthesize(
            &self.buffer.tombstones,
            &self.buffer.deletes_from_union,
            &new_deletes_from_union,
        );
        let new_text = delta.apply(&self.buffer.rope);
        let new_tombstones = shuffle_tombstones(
            &self.buffer.rope,
            &self.buffer.tombstones,
            &self.buffer.deletes_from_union,
            &new_deletes_from_union,
        );
        self.buffer.undone_groups = groups;

        if self.listener.should_apply_edit() {
            self.apply_edit(
                &delta,
                new_rev,
                new_text,
                new_tombstones,
                new_deletes_from_union,
            );
        }

        delta
    }
}
