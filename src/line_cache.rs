use serde::Deserialize;
use serde_json::Value;
use std::mem;
use std::ops::Range;

#[derive(Clone)]
pub struct Line {
    text: String,
    /// List of carets, in units of utf-16 code units.
    cursor: Vec<usize>,
    styles: Vec<StyleSpan>,
    invalid: bool,
    new_ln: usize,
}

#[derive(Deserialize)]
pub struct Style {
    pub id: usize,
    pub fg_color: Option<u32>,
    italic: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct StyleSpan {
    pub style_id: usize,
    /// Range of span, in units of utf-16 code units
    pub range: Range<usize>,
}

impl Line {
    pub fn from_json(v: &Value) -> Line {
        let text = v["text"].as_str().unwrap().to_owned();
        let mut cursor = Vec::new();
        if let Some(arr) = v["cursor"].as_array() {
            for c in arr {
                let offset_utf8 = c.as_u64().unwrap() as usize;
                cursor.push(count_utf16(&text[..offset_utf8]));
            }
        }
        let mut styles = Vec::new();
        if let Some(arr) = v["styles"].as_array() {
            let mut ix: i64 = 0;
            for triple in arr.chunks(3) {
                let start = ix + triple[0].as_i64().unwrap();
                let end = start + triple[1].as_i64().unwrap();
                // TODO: count utf from last end, if <=
                let start_utf16 = count_utf16(&text[..start as usize]);
                let end_utf16 = start_utf16 + count_utf16(&text[start as usize..end as usize]);
                let style_id = triple[2].as_u64().unwrap() as usize;
                let style_span = StyleSpan {
                    style_id,
                    range: start_utf16..end_utf16,
                };
                styles.push(style_span);
                ix = end;
            }
        }
        Line {
            text,
            cursor,
            styles,
            invalid: true,
            new_ln: 0,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn cursor(&self) -> &[usize] {
        &self.cursor
    }

    pub fn styles(&self) -> &[StyleSpan] {
        &self.styles
    }

    pub fn new_ln(&self) -> usize {
        self.new_ln
    }
}

#[derive(Deserialize, Debug, Clone)]
pub struct Annotation {
    #[serde(rename = "type")]
    pub kind: String,
    pub ranges: Vec<[usize; 4]>,
    pub payloads: Option<()>,
    pub n: usize,
}

impl Annotation {
    pub fn check_line(&self, ln: usize, line: &Line) -> Option<(usize, usize)> {
        let len = count_utf16(line.text());
        for range in &self.ranges {
            let start_line = range[0];
            let start_col = range[1];
            let end_line = range[2];
            let end_col = range[3];
            if start_line > ln {
                return None;
            }
            if end_line < ln {
                return None;
            }
            if start_line < ln && ln < end_line {
                return Some((0, len));
            }
            let mut start = 0;
            let mut end = 0;
            if start_line == ln {
                start = start_col;
                if end_line > ln {
                    end = len;
                }
            }
            if end_line == ln {
                end = end_col;
            }
            return Some((start, end));
        }
        None
    }
}

pub struct LineCache {
    lines: Vec<Option<Line>>,
    old_lines: Vec<Option<Line>>,
    annotations: Vec<Annotation>,
}

impl LineCache {
    pub fn new() -> LineCache {
        LineCache {
            lines: Vec::new(),
            old_lines: Vec::new(),
            annotations: Vec::new(),
        }
    }

    fn push_opt_line(&mut self, line: Option<Line>) {
        self.lines.push(line);
    }

    pub fn apply_update(&mut self, update: &Value) -> (usize, usize) {
        let old_cache = mem::replace(self, LineCache::new());
        let mut old_lines = old_cache.lines;
        let mut i = 0;
        let mut ln = 0;
        let mut pending_skip = 0;
        for op in update["ops"].as_array().unwrap() {
            let op_type = &op["op"];
            if op_type == "ins" {
                let lines = op["lines"].as_array().unwrap();
                pending_skip += lines.len();
                for (j, line) in lines.iter().enumerate() {
                    let line = Line::from_json(line);
                    self.push_opt_line(Some(line));
                    ln += 1;
                }
            } else if op_type == "copy" {
                pending_skip = 0;
                let n = op["n"].as_u64().unwrap();
                for _ in 0..n {
                    let line = match old_lines.get(i).unwrap_or(&None).clone() {
                        Some(mut line) => {
                            if i != ln {
                                line.invalid = true;
                            } else {
                                line.invalid = false;
                            }
                            Some(line)
                        }
                        None => None,
                    };
                    self.push_opt_line(line);

                    match old_lines.get(i).unwrap_or(&None).clone() {
                        Some(mut old_line) => {
                            old_line.new_ln = ln;
                            mem::replace(&mut old_lines[i], Some(old_line));
                        }
                        None => (),
                    };
                    i += 1;
                    ln += 1;
                    // self.push_opt_line(old_iter.next().unwrap_or_default());
                }
            } else if op_type == "skip" {
                let n = op["n"].as_u64().unwrap() as usize;
                for j in 0..n {
                    let new_ln = if j > pending_skip - 1 {
                        ln
                    } else {
                        ln - pending_skip + j
                    };
                    match old_lines.get(i).unwrap_or(&None).clone() {
                        Some(mut old_line) => {
                            old_line.new_ln = new_ln;
                            mem::replace(&mut old_lines[i], Some(old_line));
                        }
                        None => (),
                    };
                    i += 1;
                }
                pending_skip = 0;
            } else if op_type == "invalidate" {
                let n = op["n"].as_u64().unwrap() as usize;
                pending_skip += n;
                for j in 0..n {
                    let line = match old_lines.get(i + j).unwrap_or(&None).clone() {
                        Some(mut line) => {
                            if i + j != ln {
                                line.invalid = true;
                            } else {
                                line.invalid = false;
                            }
                            Some(line)
                        }
                        None => None,
                    };
                    self.push_opt_line(line);

                    ln += 1;
                }
            }
        }

        self.old_lines = old_lines;

        if let Ok(annotations) =
            serde_json::from_value::<Vec<Annotation>>(update["annotations"].clone())
        {
            self.annotations = annotations;
        }

        let mut start = -1;
        let mut end = -1;
        let mut n = 0;
        for line in &self.lines {
            match line {
                Some(line) => {
                    if line.invalid {
                        if start == -1 {
                            start = n;
                        }
                        if n > end {
                            end = n;
                        }
                    }
                }
                None => (),
            }
            n += 1;
        }

        (start as usize, end as usize)
    }

    pub fn height(&self) -> usize {
        self.lines.len()
    }

    pub fn get_line(&self, ix: usize) -> Option<&Line> {
        if ix < self.lines.len() {
            self.lines[ix].as_ref()
        } else {
            None
        }
    }

    pub fn get_old_line(&self, ix: usize) -> Option<&Line> {
        if ix < self.old_lines.len() {
            self.old_lines[ix].as_ref()
        } else {
            None
        }
    }

    pub fn annotations(&self) -> Vec<Annotation> {
        self.annotations.clone()
    }
}

/// Counts the number of utf-16 code units in the given string.
pub fn count_utf16(s: &str) -> usize {
    let mut utf16_count = 0;
    for &b in s.as_bytes() {
        if (b as i8) >= -0x40 {
            utf16_count += 1;
        }
        if b >= 0xf0 {
            utf16_count += 1;
        }
    }
    utf16_count
}
