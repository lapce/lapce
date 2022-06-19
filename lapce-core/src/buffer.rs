use std::{
    borrow::Cow,
    cmp::Ordering,
    collections::BTreeSet,
    ops::Range,
    sync::{
        atomic::{self, AtomicU64},
        Arc,
    },
};

use lsp_types::Position;
use xi_rope::{
    diff::{Diff, LineHashDiff},
    multiset::Subset,
    Cursor, Delta, DeltaBuilder, Interval, Rope, RopeDelta,
};

use crate::{
    cursor::CursorMode,
    editor::EditType,
    indent::{auto_detect_indent_style, IndentStyle},
    mode::Mode,
    selection::Selection,
    syntax::Syntax,
    word::WordCursor,
};

#[derive(Clone)]
enum Contents {
    Edit {
        /// Groups related edits together so that they are undone and re-done
        /// together. For example, an auto-indent insertion would be un-done
        /// along with the newline that triggered it.
        undo_group: usize,
        /// The subset of the characters of the union string from after this
        /// revision that were added by this revision.
        inserts: Subset,
        /// The subset of the characters of the union string from after this
        /// revision that were deleted by this revision.
        deletes: Subset,
    },
    Undo {
        /// The set of groups toggled between undone and done.
        /// Just the `symmetric_difference` (XOR) of the two sets.
        toggled_groups: BTreeSet<usize>, // set of undo_group id's
        /// Used to store a reversible difference between the old
        /// and new deletes_from_union
        deletes_bitxor: Subset,
    },
}

#[derive(Clone)]
struct Revision {
    num: u64,
    max_undo_so_far: usize,
    edit: Contents,
    cursor_before: Option<CursorMode>,
    cursor_after: Option<CursorMode>,
}

#[derive(Debug, Clone)]
pub struct InvalLines {
    pub start_line: usize,
    pub inval_count: usize,
    pub new_count: usize,
}

#[derive(Clone)]
pub struct Buffer {
    rev_counter: u64,
    pristine_rev_id: u64,
    atomic_rev: Arc<AtomicU64>,

    text: Rope,
    revs: Vec<Revision>,
    cur_undo: usize,
    undos: BTreeSet<usize>,
    undo_group_id: usize,
    live_undos: Vec<usize>,
    deletes_from_union: Subset,
    undone_groups: BTreeSet<usize>,
    tombstones: Rope,
    this_edit_type: EditType,
    last_edit_type: EditType,

    indent_style: IndentStyle,

    max_len: usize,
    max_len_line: usize,
}

impl Buffer {
    pub fn new(text: &str) -> Self {
        Self {
            text: Rope::from(text),

            rev_counter: 1,
            pristine_rev_id: 0,
            atomic_rev: Arc::new(AtomicU64::new(0)),

            revs: vec![Revision {
                num: 0,
                max_undo_so_far: 0,
                edit: Contents::Undo {
                    toggled_groups: BTreeSet::new(),
                    deletes_bitxor: Subset::new(0),
                },
                cursor_before: None,
                cursor_after: None,
            }],
            cur_undo: 1,
            undos: BTreeSet::new(),
            undo_group_id: 1,
            live_undos: vec![0],
            deletes_from_union: Subset::new(text.len()),
            undone_groups: BTreeSet::new(),
            tombstones: Rope::default(),

            this_edit_type: EditType::Other,
            last_edit_type: EditType::Other,
            indent_style: IndentStyle::DEFAULT_INDENT,

            max_len: 0,
            max_len_line: 0,
        }
    }

    pub fn rev(&self) -> u64 {
        self.revs.last().unwrap().num
    }

    pub fn set_pristine(&mut self) {
        self.pristine_rev_id = self.rev()
    }

    pub fn is_pristine(&self) -> bool {
        self.is_equivalent_revision(self.pristine_rev_id, self.rev())
    }

    pub fn set_cursor_before(&mut self, cursor: CursorMode) {
        if let Some(rev) = self.revs.last_mut() {
            rev.cursor_before = Some(cursor);
        }
    }

    pub fn set_cursor_after(&mut self, cursor: CursorMode) {
        if let Some(rev) = self.revs.last_mut() {
            rev.cursor_after = Some(cursor);
        }
    }

    fn is_equivalent_revision(&self, base_rev: u64, other_rev: u64) -> bool {
        let base_subset = self
            .find_rev(base_rev)
            .map(|rev_index| self.deletes_from_cur_union_for_index(rev_index));
        let other_subset = self
            .find_rev(other_rev)
            .map(|rev_index| self.deletes_from_cur_union_for_index(rev_index));

        base_subset.is_some() && base_subset == other_subset
    }

    fn find_rev(&self, rev_id: u64) -> Option<usize> {
        self.revs
            .iter()
            .enumerate()
            .rev()
            .find(|&(_, rev)| rev.num == rev_id)
            .map(|(i, _)| i)
    }

    pub fn atomic_rev(&self) -> Arc<AtomicU64> {
        self.atomic_rev.clone()
    }

    pub fn text(&self) -> &Rope {
        &self.text
    }

    pub fn num_lines(&self) -> usize {
        self.line_of_offset(self.text.len()) + 1
    }

    fn get_max_line_len(&self) -> (usize, usize) {
        let mut pre_offset = 0;
        let mut max_len = 0;
        let mut max_len_line = 0;
        for line in 0..self.num_lines() + 1 {
            let offset = self.offset_of_line(line);
            let line_len = offset - pre_offset;
            pre_offset = offset;
            if line_len > max_len {
                max_len = line_len;
                max_len_line = line;
            }
        }
        (max_len, max_len_line)
    }

    fn update_size(&mut self, inval_lines: &InvalLines) {
        if self.max_len_line >= inval_lines.start_line
            && self.max_len_line <= inval_lines.start_line + inval_lines.inval_count
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

    pub fn max_len(&self) -> usize {
        self.max_len
    }

    fn line_len(&self, line: usize) -> usize {
        self.offset_of_line(line + 1) - self.offset_of_line(line)
    }

    pub fn init_content(&mut self, content: Rope) {
        if !content.is_empty() {
            let delta = Delta::simple_edit(Interval::new(0, 0), content, 0);
            let (new_rev, new_text, new_tombstones, new_deletes_from_union) =
                self.mk_new_rev(0, delta.clone());
            self.apply_edit(
                &delta,
                new_rev,
                new_text,
                new_tombstones,
                new_deletes_from_union,
            );
        }
        self.set_pristine();
    }

    pub fn reload(
        &mut self,
        content: Rope,
        set_pristine: bool,
    ) -> (RopeDelta, InvalLines) {
        let delta = LineHashDiff::compute_delta(&self.text, &content);
        self.this_edit_type = EditType::Other;
        let (delta, inval_lines) = self.add_delta(delta);
        if set_pristine {
            self.set_pristine();
        }
        (delta, inval_lines)
    }

    pub fn detect_indent(&mut self, syntax: Option<&Syntax>) {
        self.indent_style =
            auto_detect_indent_style(&self.text).unwrap_or_else(|| {
                syntax
                    .map(|s| IndentStyle::from_str(s.language.indent_unit()))
                    .unwrap_or(IndentStyle::DEFAULT_INDENT)
            });
    }

    pub fn indent_unit(&self) -> &'static str {
        self.indent_style.as_str()
    }

    pub fn reset_edit_type(&mut self) {
        self.last_edit_type = EditType::Other
    }

    pub fn edit(
        &mut self,
        edits: &[(impl AsRef<Selection>, &str)],
        edit_type: EditType,
    ) -> (RopeDelta, InvalLines) {
        let mut builder = DeltaBuilder::new(self.len());
        let mut interval_rope = Vec::new();
        for (selection, content) in edits {
            let rope = Rope::from(content);
            for region in selection.as_ref().regions() {
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
        self.this_edit_type = edit_type;
        self.add_delta(delta)
    }

    fn add_delta(&mut self, delta: RopeDelta) -> (RopeDelta, InvalLines) {
        let undo_group = self.calculate_undo_group();
        self.last_edit_type = self.this_edit_type;

        let (new_rev, new_text, new_tombstones, new_deletes_from_union) =
            self.mk_new_rev(undo_group, delta.clone());

        let inval_lines = self.apply_edit(
            &delta,
            new_rev,
            new_text,
            new_tombstones,
            new_deletes_from_union,
        );

        (delta, inval_lines)
    }

    fn apply_edit(
        &mut self,
        delta: &RopeDelta,
        new_rev: Revision,
        new_text: Rope,
        new_tombstones: Rope,
        new_deletes_from_union: Subset,
    ) -> InvalLines {
        self.rev_counter += 1;

        let (iv, newlen) = delta.summary();
        let old_logical_end_line = self.text.line_of_offset(iv.end) + 1;

        self.revs.push(new_rev);
        self.text = new_text;
        self.tombstones = new_tombstones;
        self.deletes_from_union = new_deletes_from_union;

        let logical_start_line = self.text.line_of_offset(iv.start);
        let new_logical_end_line = self.text.line_of_offset(iv.start + newlen) + 1;
        let old_hard_count = old_logical_end_line - logical_start_line;
        let new_hard_count = new_logical_end_line - logical_start_line;

        let inval_lines = InvalLines {
            start_line: logical_start_line,
            inval_count: old_hard_count,
            new_count: new_hard_count,
        };
        self.update_size(&inval_lines);

        inval_lines
    }

    fn calculate_undo_group(&mut self) -> usize {
        let has_undos = !self.live_undos.is_empty();
        let is_unbroken_group =
            !self.this_edit_type.breaks_undo_group(self.last_edit_type);

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

    fn mk_new_rev(
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
        let text_with_inserts = text_ins_delta.apply(&self.text);
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
        self.atomic_rev
            .store(self.rev_counter, atomic::Ordering::Release);
        (
            Revision {
                num: self.rev_counter,
                max_undo_so_far: std::cmp::max(undo_group, head_rev.max_undo_so_far),
                edit: Contents::Edit {
                    undo_group,
                    inserts: new_inserts,
                    deletes: new_deletes,
                },
                cursor_before: None,
                cursor_after: None,
            },
            new_text,
            new_tombstones,
            new_deletes_from_union,
        )
    }

    fn deletes_from_union_for_index(&self, rev_index: usize) -> Cow<Subset> {
        self.deletes_from_union_before_index(rev_index + 1, true)
    }

    fn deletes_from_cur_union_for_index(&self, rev_index: usize) -> Cow<Subset> {
        let mut deletes_from_union = self.deletes_from_union_for_index(rev_index);
        for rev in &self.revs[rev_index + 1..] {
            if let Contents::Edit { ref inserts, .. } = rev.edit {
                if !inserts.is_empty() {
                    deletes_from_union =
                        Cow::Owned(deletes_from_union.transform_union(inserts));
                }
            }
        }
        deletes_from_union
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

        let cursor_before = self
            .revs
            .get(first_candidate)
            .and_then(|rev| rev.cursor_before.clone());

        let cursor_after = self
            .revs
            .get(first_candidate)
            .and_then(|rev| match &rev.edit {
                Contents::Edit { undo_group, .. } => Some(undo_group),
                Contents::Undo { .. } => None,
            })
            .and_then(|group| {
                let mut cursor: Option<&CursorMode> = None;
                for rev in &self.revs[first_candidate..] {
                    if let Contents::Edit { ref undo_group, .. } = rev.edit {
                        if group == undo_group {
                            cursor = rev.cursor_after.as_ref();
                        } else {
                            break;
                        }
                    }
                }
                cursor.cloned()
            });

        let deletes_bitxor = self.deletes_from_union.bitxor(&deletes_from_union);
        let max_undo_so_far = self.revs.last().unwrap().max_undo_so_far;
        self.atomic_rev
            .store(self.rev_counter, atomic::Ordering::Release);
        (
            Revision {
                num: self.rev_counter,
                max_undo_so_far,
                edit: Contents::Undo {
                    toggled_groups,
                    deletes_bitxor,
                },
                cursor_before,
                cursor_after,
            },
            deletes_from_union,
        )
    }

    fn undo(
        &mut self,
        groups: BTreeSet<usize>,
    ) -> (
        RopeDelta,
        InvalLines,
        Option<CursorMode>,
        Option<CursorMode>,
    ) {
        let (new_rev, new_deletes_from_union) = self.compute_undo(&groups);
        let delta = Delta::synthesize(
            &self.tombstones,
            &self.deletes_from_union,
            &new_deletes_from_union,
        );
        let new_text = delta.apply(&self.text);
        let new_tombstones = shuffle_tombstones(
            &self.text,
            &self.tombstones,
            &self.deletes_from_union,
            &new_deletes_from_union,
        );
        self.undone_groups = groups;

        let cursor_before = new_rev.cursor_before.clone();
        let cursor_after = new_rev.cursor_after.clone();

        let inval_lines = self.apply_edit(
            &delta,
            new_rev,
            new_text,
            new_tombstones,
            new_deletes_from_union,
        );

        (delta, inval_lines, cursor_before, cursor_after)
    }

    pub fn do_undo(
        &mut self,
    ) -> Option<(RopeDelta, InvalLines, Option<CursorMode>)> {
        if self.cur_undo > 1 {
            self.cur_undo -= 1;
            self.undos.insert(self.live_undos[self.cur_undo]);
            self.last_edit_type = EditType::Undo;
            let (delta, inval_lines, cursor_before, _cursor_after) =
                self.undo(self.undos.clone());
            Some((delta, inval_lines, cursor_before))
        } else {
            None
        }
    }

    pub fn do_redo(
        &mut self,
    ) -> Option<(RopeDelta, InvalLines, Option<CursorMode>)> {
        if self.cur_undo < self.live_undos.len() {
            self.undos.remove(&self.live_undos[self.cur_undo]);
            self.cur_undo += 1;
            self.last_edit_type = EditType::Redo;
            let (delta, inval_lines, _cursor_before, cursor_after) =
                self.undo(self.undos.clone());
            Some((delta, inval_lines, cursor_after))
        } else {
            None
        }
    }

    pub fn last_line(&self) -> usize {
        self.line_of_offset(self.text.len())
    }

    pub fn offset_of_line(&self, line: usize) -> usize {
        let last_line = self.last_line();
        let line = if line > last_line + 1 {
            last_line + 1
        } else {
            line
        };
        self.text.offset_of_line(line)
    }

    pub fn offset_line_end(&self, offset: usize, caret: bool) -> usize {
        let line = self.line_of_offset(offset);
        self.line_end_offset(line, caret)
    }

    pub fn line_of_offset(&self, offset: usize) -> usize {
        let max = self.len();
        let offset = if offset > max { max } else { offset };
        self.text.line_of_offset(offset)
    }

    pub fn offset_to_position(&self, offset: usize) -> Position {
        let (line, col) = self.offset_to_line_col(offset);
        Position {
            line: line as u32,
            character: col as u32,
        }
    }

    pub fn offset_of_position(&self, pos: &Position) -> usize {
        self.offset_of_line_col(pos.line as usize, pos.character as usize)
    }

    pub fn offset_to_line_col(&self, offset: usize) -> (usize, usize) {
        let max = self.len();
        let offset = if offset > max { max } else { offset };
        let line = self.line_of_offset(offset);
        let line_start = self.offset_of_line(line);
        if offset == line_start {
            return (line, 0);
        }

        let col = offset - line_start;
        (line, col)
    }

    pub fn offset_of_line_col(&self, line: usize, col: usize) -> usize {
        let mut pos = 0;
        let mut offset = self.offset_of_line(line);
        for c in self
            .slice_to_cow(offset..self.offset_of_line(line + 1))
            .chars()
        {
            if c == '\n' {
                return offset;
            }

            let char_len = c.len_utf8();
            if pos + char_len > col {
                return offset;
            }
            pos += char_len;
            offset += char_len;
        }
        offset
    }

    pub fn line_end_col(&self, line: usize, caret: bool) -> usize {
        let line_start = self.offset_of_line(line);
        let offset = self.line_end_offset(line, caret);
        offset - line_start
    }

    pub fn first_non_blank_character_on_line(&self, line: usize) -> usize {
        let last_line = self.last_line();
        let line = if line > last_line + 1 {
            last_line
        } else {
            line
        };
        let line_start_offset = self.text.offset_of_line(line);
        WordCursor::new(&self.text, line_start_offset).next_non_blank_char()
    }

    pub fn indent_on_line(&self, line: usize) -> String {
        let line_start_offset = self.text.offset_of_line(line);
        let word_boundary =
            WordCursor::new(&self.text, line_start_offset).next_non_blank_char();
        let indent = self.text.slice_to_cow(line_start_offset..word_boundary);
        indent.to_string()
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

    pub fn line_content(&self, line: usize) -> Cow<str> {
        self.text
            .slice_to_cow(self.offset_of_line(line)..self.offset_of_line(line + 1))
    }

    pub fn prev_grapheme_offset(
        &self,
        offset: usize,
        count: usize,
        limit: usize,
    ) -> usize {
        let mut cursor = Cursor::new(&self.text, offset);
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

    pub fn prev_code_boundary(&self, offset: usize) -> usize {
        WordCursor::new(&self.text, offset).prev_code_boundary()
    }

    pub fn next_code_boundary(&self, offset: usize) -> usize {
        WordCursor::new(&self.text, offset).next_code_boundary()
    }

    pub fn move_left(&self, offset: usize, mode: Mode, count: usize) -> usize {
        let line = self.line_of_offset(offset);
        let line_start_offset = self.offset_of_line(line);
        let min_offset = if mode == Mode::Insert {
            0
        } else {
            line_start_offset
        };

        self.prev_grapheme_offset(offset, count, min_offset)
    }

    pub fn move_right(&self, offset: usize, mode: Mode, count: usize) -> usize {
        let line_end = self.offset_line_end(offset, mode != Mode::Normal);

        let max_offset = if mode == Mode::Insert {
            self.len()
        } else {
            line_end
        };

        self.next_grapheme_offset(offset, count, max_offset)
    }

    pub fn move_word_forward(&self, offset: usize) -> usize {
        self.move_n_words_forward(offset, 1)
    }

    pub fn move_word_backward(&self, offset: usize) -> usize {
        self.move_n_words_backward(offset, 1)
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
        let mut cursor = Cursor::new(&self.text, offset);
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

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn select_word(&self, offset: usize) -> (usize, usize) {
        WordCursor::new(&self.text, offset).select_word()
    }

    pub fn char_at_offset(&self, offset: usize) -> Option<char> {
        if self.is_empty() {
            return None;
        }
        WordCursor::new(&self.text, offset)
            .inner
            .peek_next_codepoint()
    }

    pub fn previous_unmatched(
        &self,
        syntax: Option<&Syntax>,
        c: char,
        offset: usize,
    ) -> Option<usize> {
        if let Some(syntax) = syntax {
            syntax.find_tag(offset, true, &c.to_string())
        } else {
            WordCursor::new(&self.text, offset).previous_unmatched(c)
        }
    }

    pub fn slice_to_cow(&self, range: Range<usize>) -> Cow<str> {
        self.text
            .slice_to_cow(range.start.min(self.len())..range.end.min(self.len()))
    }

    pub fn len(&self) -> usize {
        self.text.len()
    }

    /// Find the nth (`count`) word starting at `offset` in either direction
    /// depending on `find_next`.
    ///
    /// A `WordCursor` is created and given to the `find_next` function for the
    /// search.  The `find_next` function should return None when there is no
    /// more word found.  Despite the name, `find_next` can search in either
    /// direction.
    fn find_nth_word<F>(
        &self,
        offset: usize,
        mut count: usize,
        mut find_next: F,
    ) -> usize
    where
        F: FnMut(&mut WordCursor) -> Option<usize>,
    {
        let mut cursor = WordCursor::new(self.text(), offset);
        let mut new_offset = offset;
        while count != 0 {
            // FIXME: wait for if-let-chain
            if let Some(offset) = find_next(&mut cursor) {
                new_offset = offset;
            } else {
                break;
            }
            count -= 1;
        }
        new_offset
    }

    pub fn move_n_words_forward(&self, offset: usize, count: usize) -> usize {
        self.find_nth_word(offset, count, |cursor| cursor.next_boundary())
    }

    pub fn move_n_wordends_forward(
        &self,
        offset: usize,
        count: usize,
        inserting: bool,
    ) -> usize {
        let mut new_offset =
            self.find_nth_word(offset, count, |cursor| cursor.end_boundary());
        if !inserting && new_offset != self.len() {
            new_offset = self.prev_grapheme_offset(new_offset, 1, 0);
        }
        new_offset
    }

    pub fn move_n_words_backward(&self, offset: usize, count: usize) -> usize {
        self.find_nth_word(offset, count, |cursor| cursor.prev_boundary())
    }
}

fn shuffle_tombstones(
    text: &Rope,
    tombstones: &Rope,
    old_deletes_from_union: &Subset,
    new_deletes_from_union: &Subset,
) -> Rope {
    // Taking the complement of deletes_from_union leads to an interleaving valid for swapped text and tombstones,
    // allowing us to use the same method to insert the text into the tombstones.
    let inverse_tombstones_map = old_deletes_from_union.complement();
    let move_delta = Delta::synthesize(
        text,
        &inverse_tombstones_map,
        &new_deletes_from_union.complement(),
    );
    move_delta.apply(tombstones)
}

fn shuffle(
    text: &Rope,
    tombstones: &Rope,
    old_deletes_from_union: &Subset,
    new_deletes_from_union: &Subset,
) -> (Rope, Rope) {
    // Delta that deletes the right bits from the text
    let del_delta = Delta::synthesize(
        tombstones,
        old_deletes_from_union,
        new_deletes_from_union,
    );
    let new_text = del_delta.apply(text);
    (
        new_text,
        shuffle_tombstones(
            text,
            tombstones,
            old_deletes_from_union,
            new_deletes_from_union,
        ),
    )
}

#[derive(Clone, Debug, PartialEq)]
pub enum DiffResult<T> {
    Left(T),
    Both(T, T),
    Right(T),
}

#[derive(Clone, Debug)]
pub enum DiffLines {
    Left(Range<usize>),
    Both(Range<usize>, Range<usize>),
    Skip(Range<usize>, Range<usize>),
    Right(Range<usize>),
}

pub fn rope_diff(
    left_rope: Rope,
    right_rope: Rope,
    rev: u64,
    atomic_rev: Arc<AtomicU64>,
) -> Option<Vec<DiffLines>> {
    let left_lines = left_rope.lines(..).collect::<Vec<Cow<str>>>();
    let right_lines = right_rope.lines(..).collect::<Vec<Cow<str>>>();

    let left_count = left_lines.len();
    let right_count = right_lines.len();
    let min_count = std::cmp::min(left_count, right_count);

    let leading_equals = left_lines
        .iter()
        .zip(right_lines.iter())
        .take_while(|p| p.0 == p.1)
        .count();
    let trailing_equals = left_lines
        .iter()
        .rev()
        .zip(right_lines.iter().rev())
        .take(min_count - leading_equals)
        .take_while(|p| p.0 == p.1)
        .count();

    let left_diff_size = left_count - leading_equals - trailing_equals;
    let right_diff_size = right_count - leading_equals - trailing_equals;

    let table: Vec<Vec<u32>> = {
        let mut table = vec![vec![0; right_diff_size + 1]; left_diff_size + 1];
        let left_skip = left_lines.iter().skip(leading_equals).take(left_diff_size);
        let right_skip = right_lines
            .iter()
            .skip(leading_equals)
            .take(right_diff_size);

        for (i, l) in left_skip.enumerate() {
            for (j, r) in right_skip.clone().enumerate() {
                if atomic_rev.load(atomic::Ordering::Acquire) != rev {
                    return None;
                }
                table[i + 1][j + 1] = if l == r {
                    table[i][j] + 1
                } else {
                    std::cmp::max(table[i][j + 1], table[i + 1][j])
                };
            }
        }

        table
    };

    let diff = {
        let mut diff = Vec::with_capacity(left_diff_size + right_diff_size);
        let mut i = left_diff_size;
        let mut j = right_diff_size;
        let mut li = left_lines.iter().rev().skip(trailing_equals);
        let mut ri = right_lines.iter().skip(trailing_equals);

        loop {
            if atomic_rev.load(atomic::Ordering::Acquire) != rev {
                return None;
            }
            if j > 0 && (i == 0 || table[i][j] == table[i][j - 1]) {
                j -= 1;
                diff.push(DiffResult::Right(ri.next().unwrap()));
            } else if i > 0 && (j == 0 || table[i][j] == table[i - 1][j]) {
                i -= 1;
                diff.push(DiffResult::Left(li.next().unwrap()));
            } else if i > 0 && j > 0 {
                i -= 1;
                j -= 1;
                diff.push(DiffResult::Both(li.next().unwrap(), ri.next().unwrap()));
            } else {
                break;
            }
        }

        diff
    };

    let mut changes = Vec::new();
    let mut left_line = 0;
    let mut right_line = 0;
    if leading_equals > 0 {
        changes.push(DiffLines::Both(0..leading_equals, 0..leading_equals));
    }
    left_line += leading_equals;
    right_line += leading_equals;

    for diff in diff.iter().rev() {
        if atomic_rev.load(atomic::Ordering::Acquire) != rev {
            return None;
        }
        match diff {
            DiffResult::Left(_) => {
                match changes.last_mut() {
                    Some(DiffLines::Left(r)) => r.end = left_line + 1,
                    _ => changes.push(DiffLines::Left(left_line..left_line + 1)),
                }
                left_line += 1;
            }
            DiffResult::Both(_, _) => {
                match changes.last_mut() {
                    Some(DiffLines::Both(l, r)) => {
                        l.end = left_line + 1;
                        r.end = right_line + 1;
                    }
                    _ => changes.push(DiffLines::Both(
                        left_line..left_line + 1,
                        right_line..right_line + 1,
                    )),
                }
                left_line += 1;
                right_line += 1;
            }
            DiffResult::Right(_) => {
                match changes.last_mut() {
                    Some(DiffLines::Right(r)) => r.end = right_line + 1,
                    _ => changes.push(DiffLines::Right(right_line..right_line + 1)),
                }
                right_line += 1;
            }
        }
    }

    if trailing_equals > 0 {
        changes.push(DiffLines::Both(
            left_count - trailing_equals..left_count,
            right_count - trailing_equals..right_count,
        ));
    }
    if !changes.is_empty() {
        let changes_last = changes.len() - 1;
        for (i, change) in changes.clone().iter().enumerate().rev() {
            if atomic_rev.load(atomic::Ordering::Acquire) != rev {
                return None;
            }
            if let DiffLines::Both(l, r) = change {
                if i == 0 || i == changes_last {
                    if r.len() > 3 {
                        if i == 0 {
                            changes[i] =
                                DiffLines::Both(l.end - 3..l.end, r.end - 3..r.end);
                            changes.insert(
                                i,
                                DiffLines::Skip(
                                    l.start..l.end - 3,
                                    r.start..r.end - 3,
                                ),
                            );
                        } else {
                            changes[i] = DiffLines::Skip(
                                l.start + 3..l.end,
                                r.start + 3..r.end,
                            );
                            changes.insert(
                                i,
                                DiffLines::Both(
                                    l.start..l.start + 3,
                                    r.start..r.start + 3,
                                ),
                            );
                        }
                    }
                } else if r.len() > 6 {
                    changes[i] = DiffLines::Both(l.end - 3..l.end, r.end - 3..r.end);
                    changes.insert(
                        i,
                        DiffLines::Skip(
                            l.start + 3..l.end - 3,
                            r.start + 3..r.end - 3,
                        ),
                    );
                    changes.insert(
                        i,
                        DiffLines::Both(l.start..l.start + 3, r.start..r.start + 3),
                    );
                }
            }
        }
    }
    Some(changes)
}

#[cfg(test)]
mod test;
