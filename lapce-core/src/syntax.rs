use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    path::Path,
    rc::Rc,
    sync::Arc,
};

use itertools::Itertools;
use tree_sitter::{Parser, Point, Query, QueryCursor, Tree};
use xi_rope::{
    spans::{Spans, SpansBuilder},
    Interval, Rope, RopeDelta,
};

use crate::{
    language::LapceLanguage,
    lens::{Lens, LensBuilder},
    style::{Highlight, HighlightEvent, Highlighter, LineStyle, Style, SCOPES},
};

thread_local! {
   static PARSER: RefCell<HashMap<LapceLanguage, Parser>> = RefCell::new(HashMap::new());
   static HIGHLIGHTS: RefCell<HashMap<LapceLanguage, crate::style::HighlightConfiguration>> = RefCell::new(HashMap::new());
}

#[derive(Clone)]
pub struct Syntax {
    rev: u64,
    language: LapceLanguage,
    pub text: Rope,
    tree: Option<Tree>,
    pub lens: Lens,
    pub normal_lines: Vec<usize>,
    pub line_height: usize,
    pub lens_height: usize,
    pub styles: Option<Spans<Style>>,
}

impl Syntax {
    pub fn init(path: &Path) -> Option<Syntax> {
        LapceLanguage::from_path(path).map(|l| Syntax {
            rev: 0,
            language: l,
            text: Rope::from(""),
            tree: None,
            lens: Self::lens_from_normal_lines(0, 0, 0, &Vec::new()),
            line_height: 0,
            lens_height: 0,
            normal_lines: Vec::new(),
            styles: None,
        })
    }

    pub fn parse(
        &self,
        new_rev: u64,
        new_text: Rope,
        delta: Option<RopeDelta>,
    ) -> Syntax {
        let mut old_tree = None;
        if new_rev == self.rev + 1 {
            if let Some(delta) = delta {
                fn point_at_offset(text: &Rope, offset: usize) -> Point {
                    let line = text.line_of_offset(offset);
                    let col = text.offset_of_line(line + 1) - offset;
                    Point::new(line, col)
                }
                let (interval, _) = delta.summary();
                let (start, end) = interval.start_end();
                if let Some(inserted) = delta.as_simple_insert() {
                    fn traverse(point: Point, text: &str) -> Point {
                        let Point {
                            mut row,
                            mut column,
                        } = point;

                        for ch in text.chars() {
                            if ch == '\n' {
                                row += 1;
                                column = 0;
                            } else {
                                column += 1;
                            }
                        }
                        Point { row, column }
                    }

                    let start_position = point_at_offset(&self.text, start);

                    let edit = tree_sitter::InputEdit {
                        start_byte: start,
                        old_end_byte: start,
                        new_end_byte: start + inserted.len(),
                        start_position,
                        old_end_position: start_position,
                        new_end_position: traverse(
                            start_position,
                            &inserted.slice_to_cow(0..inserted.len()),
                        ),
                    };
                    old_tree = self.tree.as_ref().map(|tree| {
                        let mut tree = tree.clone();
                        tree.edit(&edit);
                        tree
                    });
                } else if delta.is_simple_delete() {
                    let start_position = point_at_offset(&self.text, start);
                    let end_position = point_at_offset(&self.text, end);
                    let edit = tree_sitter::InputEdit {
                        start_byte: start,
                        old_end_byte: end,
                        new_end_byte: start,
                        start_position,
                        old_end_position: end_position,
                        new_end_position: start_position,
                    };
                    old_tree = self.tree.as_ref().map(|tree| {
                        let mut tree = tree.clone();
                        tree.edit(&edit);
                        tree
                    });
                };
            }
        }

        let new_tree = PARSER.with(|parsers| {
            let mut parsers = parsers.borrow_mut();
            parsers
                .entry(self.language)
                .or_insert_with(|| self.language.new_parser());
            let parser = parsers.get_mut(&self.language).unwrap();

            parser.parse_with(
                &mut |byte, _| {
                    if byte <= new_text.len() {
                        new_text
                            .iter_chunks(byte..)
                            .next()
                            .map(|s| s.as_bytes())
                            .unwrap_or(&[])
                    } else {
                        &[]
                    }
                },
                old_tree.as_ref(),
            )
        });

        let styles = if let Some(tree) = new_tree.as_ref() {
            let styles = HIGHLIGHTS.with(|configs| {
                let mut configs = configs.borrow_mut();
                configs
                    .entry(self.language)
                    .or_insert_with(|| self.language.new_highlight_config());
                let config = configs.get(&self.language).unwrap();
                let mut current_hl: Option<Highlight> = None;
                let mut highlights = SpansBuilder::new(new_text.len());
                let mut highlighter = Highlighter::new();
                for highlight in highlighter
                    .highlight(
                        tree.clone(),
                        config,
                        new_text.slice_to_cow(0..new_text.len()).as_bytes(),
                        None,
                        |_| None,
                    )
                    .flatten()
                {
                    match highlight {
                        HighlightEvent::Source { start, end } => {
                            if let Some(hl) = current_hl {
                                if let Some(hl) = SCOPES.get(hl.0) {
                                    highlights.add_span(
                                        Interval::new(start, end),
                                        Style {
                                            fg_color: Some(hl.to_string()),
                                        },
                                    );
                                }
                            }
                        }
                        HighlightEvent::HighlightStart(hl) => {
                            current_hl = Some(hl);
                        }
                        HighlightEvent::HighlightEnd => current_hl = None,
                    }
                }
                highlights.build()
            });
            Some(styles)
        } else {
            None
        };

        let normal_lines = if let Some(tree) = new_tree.as_ref() {
            let mut cursor = tree.walk();
            let mut normal_lines = HashSet::new();
            self.language.walk_tree(&mut cursor, &mut normal_lines);
            let normal_lines: Vec<usize> =
                normal_lines.into_iter().sorted().collect();
            normal_lines
        } else {
            Vec::new()
        };

        let lens = Self::lens_from_normal_lines(
            new_text.line_of_offset(new_text.len()),
            0,
            0,
            &normal_lines,
        );
        Syntax {
            rev: new_rev,
            language: self.language,
            tree: new_tree,
            text: new_text,
            lens,
            line_height: 0,
            lens_height: 0,
            normal_lines,
            styles,
        }
    }

    pub fn update_lens_height(&mut self, line_height: usize, lens_height: usize) {
        self.lens = Self::lens_from_normal_lines(
            self.text.line_of_offset(self.text.len()),
            line_height,
            lens_height,
            &self.normal_lines,
        );
        self.line_height = line_height;
        self.lens_height = lens_height;
    }

    pub fn lens_from_normal_lines(
        total_lines: usize,
        line_height: usize,
        lens_height: usize,
        normal_lines: &[usize],
    ) -> Lens {
        let mut builder = LensBuilder::new();
        let mut current_line = 0;
        for normal_line in normal_lines.iter() {
            let normal_line = *normal_line;
            if normal_line > current_line {
                builder.add_section(normal_line - current_line, lens_height);
            }
            builder.add_section(1, line_height);
            current_line = normal_line + 1;
        }
        if current_line < total_lines {
            builder.add_section(total_lines - current_line, lens_height);
        }
        builder.build()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lens() {
        let lens = Syntax::lens_from_normal_lines(5, 25, 2, &[4]);
        assert_eq!(5, lens.len());
        assert_eq!(8, lens.height_of_line(4));
        assert_eq!(33, lens.height_of_line(5));

        let lens = Syntax::lens_from_normal_lines(5, 25, 2, &[3]);
        assert_eq!(5, lens.len());
        assert_eq!(6, lens.height_of_line(3));
        assert_eq!(31, lens.height_of_line(4));
        assert_eq!(33, lens.height_of_line(5));
    }
}
