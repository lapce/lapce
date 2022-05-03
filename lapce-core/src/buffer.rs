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

use lapce_rpc::buffer::BufferId;
use lsp_types::Position;
use xi_rope::{
    multiset::Subset, Cursor, Delta, DeltaBuilder, Interval, Rope, RopeDelta,
};

use crate::{
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
    max_undo_so_far: usize,
    edit: Contents,
}

#[derive(Debug, Clone)]
pub struct InvalLines {
    pub start_line: usize,
    pub inval_count: usize,
    pub new_count: usize,
}

#[derive(Clone)]
pub struct Buffer {
    rev: u64,
    atomic_rev: Arc<AtomicU64>,
    dirty: bool,

    text: Rope,
    revs: Vec<Revision>,
    cur_undo: usize,
    undos: BTreeSet<usize>,
    undo_group_id: usize,
    live_undos: Vec<usize>,
    deletes_from_union: Subset,
    undone_groups: BTreeSet<usize>,
    tombstones: Rope,
    last_edit_type: EditType,

    indent_style: IndentStyle,

    max_len: usize,
    max_len_line: usize,
}

impl Buffer {
    pub fn new(text: &str) -> Self {
        Self {
            text: Rope::from(text),

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
            deletes_from_union: Subset::new(text.len()),
            undone_groups: BTreeSet::new(),
            tombstones: Rope::default(),

            last_edit_type: EditType::Other,
            indent_style: IndentStyle::DEFAULT_INDENT,

            max_len: 0,
            max_len_line: 0,
        }
    }

    pub fn rev(&self) -> u64 {
        self.rev
    }

    pub fn dirty(&self) -> bool {
        self.dirty
    }

    pub fn set_dirty(&mut self, dirty: bool) {
        self.dirty = dirty;
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

    pub fn max_len(&self) -> usize {
        self.max_len
    }

    fn line_len(&self, line: usize) -> usize {
        self.offset_of_line(line + 1) - self.offset_of_line(line)
    }

    fn reset_revs(&mut self) {
        self.text = Rope::from("");
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

    pub fn load_content(&mut self, content: &str) {
        self.reset_revs();

        if !content.is_empty() {
            let delta =
                Delta::simple_edit(Interval::new(0, 0), Rope::from(content), 0);
            let (new_rev, new_text, new_tombstones, new_deletes_from_union) =
                self.mk_new_rev(0, delta);
            self.revs.push(new_rev);
            self.text = new_text;
            self.tombstones = new_tombstones;
            self.deletes_from_union = new_deletes_from_union;
        }
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
        let undo_group = self.calculate_undo_group(edit_type);
        self.last_edit_type = edit_type;

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
        self.rev += 1;
        self.atomic_rev.store(self.rev, atomic::Ordering::Release);
        self.dirty = true;

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

    fn undo(&mut self, groups: BTreeSet<usize>) -> (RopeDelta, InvalLines) {
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

        let inval_lines = self.apply_edit(
            &delta,
            new_rev,
            new_text,
            new_tombstones,
            new_deletes_from_union,
        );

        (delta, inval_lines)
    }

    pub fn do_undo(&mut self) -> Option<(RopeDelta, InvalLines)> {
        if self.cur_undo > 1 {
            self.cur_undo -= 1;
            self.undos.insert(self.live_undos[self.cur_undo]);
            self.last_edit_type = EditType::Undo;
            Some(self.undo(self.undos.clone()))
        } else {
            None
        }
    }

    pub fn do_redo(&mut self) -> Option<(RopeDelta, InvalLines)> {
        if self.cur_undo < self.live_undos.len() {
            self.undos.remove(&self.live_undos[self.cur_undo]);
            self.cur_undo += 1;
            self.last_edit_type = EditType::Redo;
            Some(self.undo(self.undos.clone()))
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
        let new_offset = WordCursor::new(&self.text, offset)
            .next_boundary()
            .unwrap_or(offset);
        new_offset
    }

    pub fn move_word_backward(&self, offset: usize) -> usize {
        let new_offset = WordCursor::new(&self.text, offset)
            .prev_boundary()
            .unwrap_or(offset);
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
