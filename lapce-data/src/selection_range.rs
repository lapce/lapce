use lapce_rpc::buffer::BufferId;
use lsp_types::{Range, SelectionRange};

/// Lsp [selectionRange](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#selectionRange)
/// are used to do "smart" syntax selection. A buffer id, buffer revision and cursor position are
/// stored along side [`lsp_type::SelectionRange`] data to ensure the current selection still apply
/// to the current buffer.
#[derive(Clone, Debug)]
pub struct SyntaxSelectionRanges {
    pub buffer_id: BufferId,
    pub rev: u64,
    pub last_known_selection: Option<(usize, usize)>,
    pub ranges: SelectionRange,
    pub current_selection: Option<usize>,
}

/// Helper to request either the next or previous [`SyntaxSelectionRanges`],
/// see: [`crate::editor::LapceUiCommand::ApplySelectionRange`]
#[derive(Clone, Copy, Debug)]
pub enum SelectionRangeDirection {
    Next,
    Previous,
}

impl SyntaxSelectionRanges {
    /// Ensure the editor state match this selection range, if not
    /// a new SelectionRanges should be requested
    pub fn match_request(
        &self,
        buffer_id: BufferId,
        rev: u64,
        current_selection: Option<(usize, usize)>,
    ) -> bool {
        if self.last_known_selection.is_some() {
            buffer_id == self.buffer_id
                && rev == self.rev
                && current_selection == self.last_known_selection
        } else {
            buffer_id == self.buffer_id && rev == self.rev
        }
    }

    /// Get the next [`lsp_types::Range'] at `current_selection` depth
    pub fn next_range(&mut self) -> Option<Range> {
        match self.current_selection {
            None => self.current_selection = Some(0),
            Some(index) => {
                if index < self.count() - 1 {
                    self.current_selection = Some(index + 1)
                }
            }
        };
        self.get()
    }

    /// Get the previous [`lsp_types::Range'] at `current_selection` depth
    pub fn previous_range(&mut self) -> Option<Range> {
        if let Some(index) = self.current_selection {
            if index > 0 {
                self.current_selection = Some(index - 1)
            }
        }
        self.get()
    }

    fn get(&self) -> Option<Range> {
        self.current_selection.and_then(|index| {
            if index == 0 {
                Some(self.ranges.range)
            } else {
                let mut current = self.ranges.parent.as_ref();

                for _ in 1..index {
                    current = current.and_then(|c| c.parent.as_ref());
                }

                current.map(|c| c.range)
            }
        })
    }

    fn count(&self) -> usize {
        let mut count = 1;
        let mut range = &self.ranges;
        while let Some(parent) = &range.parent {
            count += 1;
            range = parent;
        }

        count
    }
}

#[cfg(test)]
mod test {
    use lapce_rpc::buffer::BufferId;
    use lsp_types::{Position, Range, SelectionRange};

    use crate::selection_range::SyntaxSelectionRanges;

    #[test]
    fn should_get_next_selection_range() {
        let range_zero = Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: 0,
                character: 0,
            },
        };
        let range_one = Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: 0,
                character: 2,
            },
        };
        let range_two = Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: 0,
                character: 4,
            },
        };

        let mut syntax_selection = SyntaxSelectionRanges {
            buffer_id: BufferId(0),
            rev: 0,
            last_known_selection: None,
            ranges: SelectionRange {
                range: range_zero,
                parent: Some(Box::new(SelectionRange {
                    range: range_one,
                    parent: Some(Box::new(SelectionRange {
                        range: range_two,
                        parent: None,
                    })),
                })),
            },
            current_selection: None,
        };

        let range = syntax_selection.next_range();
        assert_eq!(range, Some(range_zero));
        assert_eq!(syntax_selection.current_selection, Some(0));

        let range = syntax_selection.next_range();
        assert_eq!(range, Some(range_one));
        assert_eq!(syntax_selection.current_selection, Some(1));

        let range = syntax_selection.next_range();
        assert_eq!(range, Some(range_two));
        assert_eq!(syntax_selection.current_selection, Some(2));

        // Ensure we are not going out of bound
        let range = syntax_selection.next_range();
        assert_eq!(range, Some(range_two));
        assert_eq!(syntax_selection.current_selection, Some(2));

        // Going backward now
        let range = syntax_selection.previous_range();
        assert_eq!(range, Some(range_one));
        assert_eq!(syntax_selection.current_selection, Some(1));

        let range = syntax_selection.previous_range();
        assert_eq!(range, Some(range_zero));
        assert_eq!(syntax_selection.current_selection, Some(0));

        // Ensure we are not going below zero
        let range = syntax_selection.previous_range();
        assert_eq!(range, Some(range_zero));
        assert_eq!(syntax_selection.current_selection, Some(0));
    }
}
