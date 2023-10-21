use std::{
    borrow::{Borrow, Cow},
    cmp::Ordering,
    collections::BTreeSet,
    sync::{
        atomic::{self, AtomicU64},
        Arc,
    },
};

use lapce_xi_rope::{
    delta::InsertDelta,
    multiset::{CountMatcher, Subset},
    tree::{Node, NodeInfo},
    Delta, DeltaBuilder, DeltaElement, Interval, Rope, RopeDelta, RopeInfo,
};

use crate::{
    char_buffer::CharBuffer,
    cursor::CursorMode,
    editor::EditType,
    indent::{auto_detect_indent_style, IndentStyle},
    mode::Mode,
    selection::Selection,
    syntax::{self, edit::SyntaxEdit, Syntax},
    word::WordCursor,
};

pub mod diff;
pub mod rope_text;

use rope_text::*;

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
    pub old_text: Rope,
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

impl ToString for Buffer {
    fn to_string(&self) -> String {
        self.text().to_string()
    }
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

    /// The current buffer revision
    pub fn rev(&self) -> u64 {
        self.revs.last().unwrap().num
    }

    /// Mark the buffer as pristine (aka 'saved')
    pub fn set_pristine(&mut self) {
        self.pristine_rev_id = self.rev();
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

    fn get_max_line_len(&self) -> (usize, usize) {
        let mut pre_offset = 0;
        let mut max_len = 0;
        let mut max_len_line = 0;
        for line in 0..=self.num_lines() {
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

    pub fn init_content(&mut self, content: Rope) {
        if !content.is_empty() {
            let delta = Delta::simple_edit(Interval::new(0, 0), content, 0);
            let (new_rev, new_text, new_tombstones, new_deletes_from_union, _) =
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
    ) -> (RopeDelta, InvalLines, SyntaxEdit) {
        let len = self.text.len();
        let delta = Delta::simple_edit(Interval::new(0, len), content, len);
        self.this_edit_type = EditType::Other;
        let (delta, inval_lines, edits) = self.add_delta(delta);
        if set_pristine {
            self.set_pristine();
        }
        (delta, inval_lines, edits)
    }

    pub fn detect_indent(&mut self, syntax: &Syntax) {
        self.indent_style = auto_detect_indent_style(&self.text)
            .unwrap_or_else(|| IndentStyle::from_str(syntax.language.indent_unit()));
    }

    pub fn indent_unit(&self) -> &'static str {
        self.indent_style.as_str()
    }

    pub fn reset_edit_type(&mut self) {
        self.last_edit_type = EditType::Other;
    }

    pub fn edit<'a, I, E, S>(
        &mut self,
        edits: I,
        edit_type: EditType,
    ) -> (RopeDelta, InvalLines, SyntaxEdit)
    where
        I: IntoIterator<Item = E>,
        E: Borrow<(S, &'a str)>,
        S: AsRef<Selection>,
    {
        let mut builder = DeltaBuilder::new(self.len());
        let mut interval_rope = Vec::new();
        for edit in edits {
            let (selection, content) = edit.borrow();
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

    fn add_delta(
        &mut self,
        delta: RopeDelta,
    ) -> (RopeDelta, InvalLines, SyntaxEdit) {
        let undo_group = self.calculate_undo_group();
        self.last_edit_type = self.this_edit_type;

        let (new_rev, new_text, new_tombstones, new_deletes_from_union, edits) =
            self.mk_new_rev(undo_group, delta.clone());

        let inval_lines = self.apply_edit(
            &delta,
            new_rev,
            new_text,
            new_tombstones,
            new_deletes_from_union,
        );

        (delta, inval_lines, edits)
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
        let old_text = self.text.clone();

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
            old_text,
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

    fn generate_edits(
        &self,
        ins_delta: &InsertDelta<RopeInfo>,
        deletes: &Subset,
    ) -> SyntaxEdit {
        let deletes = deletes.transform_expand(&ins_delta.inserted_subset());

        let mut edits = Vec::new();

        let mut insert_edits: Vec<tree_sitter::InputEdit> =
            InsertsValueIter::new(ins_delta)
                .map(|insert| {
                    let start = insert.old_offset;
                    let inserted = insert.node;
                    syntax::edit::create_insert_edit(&self.text, start, inserted)
                })
                .collect();
        insert_edits.reverse();
        edits.append(&mut insert_edits);

        let text = ins_delta.apply(&self.text);
        let mut delete_edits: Vec<tree_sitter::InputEdit> = deletes
            .range_iter(CountMatcher::NonZero)
            .map(|(start, end)| syntax::edit::create_delete_edit(&text, start, end))
            .collect();
        delete_edits.reverse();
        edits.append(&mut delete_edits);

        SyntaxEdit::new(edits)
    }

    fn mk_new_rev(
        &self,
        undo_group: usize,
        delta: RopeDelta,
    ) -> (Revision, Rope, Rope, Subset, SyntaxEdit) {
        let (ins_delta, deletes) = delta.factor();

        let edits = self.generate_edits(&ins_delta, &deletes);

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

        let head_rev = self.revs.last().unwrap();
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
            edits,
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
                let mut cursor = None;
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
        SyntaxEdit,
        Option<CursorMode>,
        Option<CursorMode>,
    ) {
        let (new_rev, new_deletes_from_union) = self.compute_undo(&groups);
        let delta = Delta::synthesize(
            &self.tombstones,
            &self.deletes_from_union,
            &new_deletes_from_union,
        );
        let edits = {
            let (ins_delta, deletes) = delta.clone().factor();
            self.generate_edits(&ins_delta, &deletes)
        };
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

        (delta, inval_lines, edits, cursor_before, cursor_after)
    }

    pub fn do_undo(
        &mut self,
    ) -> Option<(RopeDelta, InvalLines, SyntaxEdit, Option<CursorMode>)> {
        if self.cur_undo <= 1 {
            return None;
        }

        self.cur_undo -= 1;
        self.undos.insert(self.live_undos[self.cur_undo]);
        self.last_edit_type = EditType::Undo;
        let (delta, inval_lines, edits, cursor_before, _cursor_after) =
            self.undo(self.undos.clone());

        Some((delta, inval_lines, edits, cursor_before))
    }

    pub fn do_redo(
        &mut self,
    ) -> Option<(RopeDelta, InvalLines, SyntaxEdit, Option<CursorMode>)> {
        if self.cur_undo >= self.live_undos.len() {
            return None;
        }

        self.undos.remove(&self.live_undos[self.cur_undo]);
        self.cur_undo += 1;
        self.last_edit_type = EditType::Redo;
        let (delta, inval_lines, edits, _cursor_before, cursor_after) =
            self.undo(self.undos.clone());

        Some((delta, inval_lines, edits, cursor_after))
    }

    pub fn move_word_forward(&self, offset: usize) -> usize {
        self.move_n_words_forward(offset, 1)
    }

    pub fn move_word_backward(&self, offset: usize, mode: Mode) -> usize {
        self.move_n_words_backward(offset, 1, mode)
    }

    pub fn char_at_offset(&self, offset: usize) -> Option<char> {
        if self.is_empty() {
            return None;
        }
        let offset = offset.min(self.len());
        WordCursor::new(&self.text, offset)
            .inner
            .peek_next_codepoint()
    }

    pub fn previous_unmatched(
        &self,
        syntax: &Syntax,
        c: char,
        offset: usize,
    ) -> Option<usize> {
        if syntax.layers.is_some() {
            syntax.find_tag(offset, true, &CharBuffer::new(c))
        } else {
            WordCursor::new(&self.text, offset).previous_unmatched(c)
        }
    }
}

impl RopeText for Buffer {
    fn text(&self) -> &Rope {
        &self.text
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

pub struct DeltaValueRegion<'a, N: NodeInfo + 'a> {
    pub old_offset: usize,
    pub new_offset: usize,
    pub len: usize,
    pub node: &'a Node<N>,
}

/// Modified version of `xi_rope::delta::InsertsIter` which includes the node
pub struct InsertsValueIter<'a, N: NodeInfo + 'a> {
    pos: usize,
    last_end: usize,
    els_iter: std::slice::Iter<'a, DeltaElement<N>>,
}
impl<'a, N: NodeInfo + 'a> InsertsValueIter<'a, N> {
    pub fn new(delta: &'a Delta<N>) -> InsertsValueIter<'a, N> {
        InsertsValueIter {
            pos: 0,
            last_end: 0,
            els_iter: delta.els.iter(),
        }
    }
}
impl<'a, N: NodeInfo> Iterator for InsertsValueIter<'a, N> {
    type Item = DeltaValueRegion<'a, N>;

    fn next(&mut self) -> Option<Self::Item> {
        for elem in &mut self.els_iter {
            match *elem {
                DeltaElement::Copy(b, e) => {
                    self.pos += e - b;
                    self.last_end = e;
                }
                DeltaElement::Insert(ref n) => {
                    let result = Some(DeltaValueRegion {
                        old_offset: self.last_end,
                        new_offset: self.pos,
                        len: n.len(),
                        node: n,
                    });
                    self.pos += n.len();
                    self.last_end += n.len();
                    return result;
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod test;
