use core::fmt;
use std::{fmt::Display, str::FromStr};

use anyhow::Error;
use once_cell::sync::Lazy;
use regex::Regex;

#[derive(Debug, PartialEq)]
pub enum SnippetElement {
    Text(String),
    PlaceHolder(usize, Vec<SnippetElement>),
    Tabstop(usize),
}

impl Display for SnippetElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self {
            SnippetElement::Text(text) => f.write_str(text),
            SnippetElement::PlaceHolder(tab, elements) => {
                // Trying to write to the provided buffer in the form "${tab:text}"
                write!(f, "${{{tab}:")?;
                for child_snippet_elm in elements {
                    // call ourselves recursively
                    fmt::Display::fmt(child_snippet_elm, f)?;
                }
                f.write_str("}")
            }
            SnippetElement::Tabstop(tab) => write!(f, "${tab}"),
        }
    }
}

impl SnippetElement {
    pub fn len(&self) -> usize {
        match &self {
            SnippetElement::Text(text) => text.len(),
            SnippetElement::PlaceHolder(_, elements) => {
                elements.iter().map(|e| e.len()).sum()
            }
            SnippetElement::Tabstop(_) => 0,
        }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    pub fn text(&self) -> String {
        let mut buf = String::new();
        self.write_text_to(&mut buf)
            .expect("a write_to function returned an error unexpectedly");
        buf
    }

    fn write_text_to<Buffer: fmt::Write>(&self, buf: &mut Buffer) -> fmt::Result {
        match self {
            SnippetElement::Text(text) => buf.write_str(text),
            SnippetElement::PlaceHolder(_, elements) => {
                for child_snippet_elm in elements {
                    // call ourselves recursively
                    child_snippet_elm.write_text_to(buf)?;
                }
                fmt::Result::Ok(())
            }
            SnippetElement::Tabstop(_) => fmt::Result::Ok(()),
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct Snippet {
    elements: Vec<SnippetElement>,
}

impl FromStr for Snippet {
    type Err = Error;

    #[inline]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (elements, _) = Self::extract_elements(s, 0, &['$', '\\'], &['}']);
        Ok(Snippet { elements })
    }
}

impl Display for Snippet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for snippet_element in self.elements.iter() {
            fmt::Display::fmt(snippet_element, f)?;
        }
        fmt::Result::Ok(())
    }
}

impl Snippet {
    #[inline]
    fn extract_elements(
        s: &str,
        pos: usize,
        escs: &[char],
        loose_escs: &[char],
    ) -> (Vec<SnippetElement>, usize) {
        let mut elements = Vec::new();
        let mut pos = pos;
        loop {
            if s.len() == pos {
                break;
            } else if let Some((ele, end)) = Self::extract_tabstop(s, pos) {
                elements.push(ele);
                pos = end;
            } else if let Some((ele, end)) = Self::extract_placeholder(s, pos) {
                elements.push(ele);
                pos = end;
            } else if let Some((ele, end)) =
                Self::extract_text(s, pos, escs, loose_escs)
            {
                elements.push(ele);
                pos = end;
            } else {
                break;
            }
        }
        (elements, pos)
    }

    #[inline]
    fn extract_tabstop(str: &str, pos: usize) -> Option<(SnippetElement, usize)> {
        // Regex for `$...` pattern, where `...` is some number (for example `$1`)
        static REGEX_FIRST: Lazy<Regex> =
            Lazy::new(|| Regex::new(r"^\$(\d+)").unwrap());
        // Regex for `${...}` pattern, where `...` is some number (for example `${1}`)
        static REGEX_SECOND: Lazy<Regex> =
            Lazy::new(|| Regex::new(r"^\$\{(\d+)\}").unwrap());

        let str = &str[pos..];
        if let Some(matched) = REGEX_FIRST.find(str) {
            // SAFETY:
            // * The start index is guaranteed not to exceed the end index, since we
            //   compare with the `$ ...` pattern, and, therefore, the first element
            //   is always equal to the symbol `$`;
            // * The indices are within the bounds of the original slice and lie on
            //   UTF-8 sequence boundaries, since we take the entire slice, with the
            //   exception of the first `$` char which is 1 byte in accordance with
            //   the UTF-8 standard.
            let n = unsafe {
                matched.as_str().get_unchecked(1..).parse::<usize>().ok()?
            };
            let end = pos + matched.end();
            return Some((SnippetElement::Tabstop(n), end));
        }
        if let Some(matched) = REGEX_SECOND.find(str) {
            let matched = matched.as_str();
            // SAFETY:
            // * The start index is guaranteed not to exceed the end index, since we
            //   compare with the `${...}` pattern, and, therefore, the first two elements
            //   are always equal to the `${` and the last one is equal to `}`;
            // * The indices are within the bounds of the original slice and lie on UTF-8
            //   sequence boundaries, since we take the entire slice, with the exception
            //   of the first two `${` and last one `}` chars each of which is 1 byte in
            //   accordance with the UTF-8 standard.
            let n = unsafe {
                matched
                    .get_unchecked(2..matched.len() - 1)
                    .parse::<usize>()
                    .ok()?
            };
            let end = pos + matched.len();
            return Some((SnippetElement::Tabstop(n), end));
        }
        None
    }

    #[inline]
    fn extract_placeholder(s: &str, pos: usize) -> Option<(SnippetElement, usize)> {
        // Regex for `${num:text}` pattern, where text can be empty (for example `${1:first}`
        // and `${2:}`)
        static REGEX: Lazy<Regex> =
            Lazy::new(|| Regex::new(r"^\$\{(\d+):(.*?)\}").unwrap());

        let caps = REGEX.captures(&s[pos..])?;

        let tab = caps.get(1)?.as_str().parse::<usize>().ok()?;

        let m = caps.get(2)?;
        let content = m.as_str();
        if content.is_empty() {
            return Some((
                SnippetElement::PlaceHolder(
                    tab,
                    vec![SnippetElement::Text(String::new())],
                ),
                pos + caps.get(0).unwrap().end(),
            ));
        }
        let (els, pos) =
            Self::extract_elements(s, pos + m.start(), &['$', '}', '\\'], &[]);
        Some((SnippetElement::PlaceHolder(tab, els), pos + 1))
    }

    #[inline]
    fn extract_text(
        s: &str,
        pos: usize,
        escs: &[char],
        loose_escs: &[char],
    ) -> Option<(SnippetElement, usize)> {
        let mut ele = String::new();
        let mut end = pos;
        let mut chars_iter = s[pos..].chars().peekable();

        while let Some(char) = chars_iter.next() {
            if char == '\\' {
                if let Some(&next) = chars_iter.peek() {
                    if escs.iter().chain(loose_escs.iter()).any(|c| *c == next) {
                        chars_iter.next();
                        ele.push(next);
                        end += 1 + next.len_utf8();
                        continue;
                    }
                }
            }
            if escs.contains(&char) {
                break;
            }
            ele.push(char);
            end += char.len_utf8();
        }
        if ele.is_empty() {
            return None;
        }
        Some((SnippetElement::Text(ele), end))
    }

    #[inline]
    pub fn text(&self) -> String {
        let mut buf = String::new();
        self.write_text_to(&mut buf)
            .expect("Snippet::write_text_to function unexpectedly return error");
        buf
    }

    #[inline]
    fn write_text_to<Buffer: fmt::Write>(&self, buf: &mut Buffer) -> fmt::Result {
        for snippet_element in self.elements.iter() {
            snippet_element.write_text_to(buf)?
        }
        fmt::Result::Ok(())
    }

    #[inline]
    pub fn tabs(&self, pos: usize) -> Vec<(usize, (usize, usize))> {
        Self::elements_tabs(&self.elements, pos)
    }

    pub fn elements_tabs(
        elements: &[SnippetElement],
        start: usize,
    ) -> Vec<(usize, (usize, usize))> {
        let mut tabs = Vec::new();
        let mut pos = start;
        for el in elements {
            match el {
                SnippetElement::Text(t) => {
                    pos += t.len();
                }
                SnippetElement::PlaceHolder(tab, els) => {
                    let placeholder_tabs = Self::elements_tabs(els, pos);
                    let end = pos + els.iter().map(|e| e.len()).sum::<usize>();
                    tabs.push((*tab, (pos, end)));
                    tabs.extend_from_slice(&placeholder_tabs);
                    pos = end;
                }
                SnippetElement::Tabstop(tab) => {
                    tabs.push((*tab, (pos, pos)));
                }
            }
        }
        tabs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snippet() {
        use SnippetElement::*;

        let s = "start $1${2:second ${3:third}} $0";
        let parsed = Snippet::from_str(s).unwrap();
        assert_eq!(s, parsed.to_string());

        let text = "start second third ";
        assert_eq!(text, parsed.text());

        assert_eq!(
            vec![(1, (6, 6)), (2, (6, 18)), (3, (13, 18)), (0, (19, 19))],
            parsed.tabs(0)
        );

        let s = "start ${1}${2:second ${3:third}} $0and ${4}fourth";

        let parsed = Snippet::from_str(s).unwrap();
        assert_eq!(
            "start $1${2:second ${3:third}} $0and $4fourth",
            parsed.to_string()
        );

        let text = "start second third and fourth";
        assert_eq!(text, parsed.text());

        assert_eq!(
            vec![
                (1, (6, 6)),
                (2, (6, 18)),
                (3, (13, 18)),
                (0, (19, 19)),
                (4, (23, 23))
            ],
            parsed.tabs(0)
        );

        let s = "${1:first $6${2:second ${7}${3:third ${4:fourth ${5:fifth}}}}}";

        let parsed = Snippet::from_str(s).unwrap();
        assert_eq!(
            "${1:first $6${2:second $7${3:third ${4:fourth ${5:fifth}}}}}",
            parsed.to_string()
        );

        let text = "first second third fourth fifth";
        assert_eq!(text, parsed.text());

        assert_eq!(
            vec![
                (1, (0, 31)),
                (6, (6, 6)),
                (2, (6, 31)),
                (7, (13, 13)),
                (3, (13, 31)),
                (4, (19, 31)),
                (5, (26, 31))
            ],
            parsed.tabs(0)
        );

        assert_eq!(
            Snippet {
                elements: vec![PlaceHolder(
                    1,
                    vec![
                        Text("first ".into()),
                        Tabstop(6),
                        PlaceHolder(
                            2,
                            vec![
                                Text("second ".into()),
                                Tabstop(7),
                                PlaceHolder(
                                    3,
                                    vec![
                                        Text("third ".into()),
                                        PlaceHolder(
                                            4,
                                            vec![
                                                Text("fourth ".into()),
                                                PlaceHolder(
                                                    5,
                                                    vec![Text("fifth".into())]
                                                )
                                            ]
                                        )
                                    ]
                                )
                            ]
                        )
                    ]
                )]
            },
            parsed
        );

        let s = "\\$1 start \\$2$3${4}${5:some text\\${6:third\\} $7}";

        let parsed = Snippet::from_str(s).unwrap();
        assert_eq!(
            "$1 start $2$3$4${5:some text${6:third} $7}",
            parsed.to_string()
        );

        let text = "$1 start $2some text${6:third} ";
        assert_eq!(text, parsed.text());

        assert_eq!(
            vec![(3, (11, 11)), (4, (11, 11)), (5, (11, 31)), (7, (31, 31))],
            parsed.tabs(0)
        );

        assert_eq!(
            Snippet {
                elements: vec![
                    Text("$1 start $2".into()),
                    Tabstop(3),
                    Tabstop(4),
                    PlaceHolder(
                        5,
                        vec![Text("some text${6:third} ".into()), Tabstop(7)]
                    )
                ]
            },
            parsed
        );
    }

    #[test]
    fn test_extract_tabstop() {
        fn vec_of_tab_elms(s: &str) -> Vec<(usize, usize)> {
            let mut pos = 0;
            let mut vec = Vec::new();
            for char in s.chars() {
                if let Some((SnippetElement::Tabstop(stop), end)) =
                    Snippet::extract_tabstop(s, pos)
                {
                    vec.push((stop, end));
                }
                pos += char.len_utf8();
            }
            vec
        }

        let s = "start $1${2:second ${3:third}} $0";
        assert_eq!(&[(1, 8), (0, 33)][..], &vec_of_tab_elms(s)[..]);

        let s = "start ${1}${2:second ${3:third}} $0and ${4}fourth";
        assert_eq!(&[(1, 10), (0, 35), (4, 43)][..], &vec_of_tab_elms(s)[..]);

        let s = "$s$1first${2}$second$3${4}${5}$6and${7}$8fourth$9$$$10$$${11}$$$12$$$13$$${14}$$${15}";
        assert_eq!(
            &[
                (1, 4),
                (2, 13),
                (3, 22),
                (4, 26),
                (5, 30),
                (6, 32),
                (7, 39),
                (8, 41),
                (9, 49),
                (10, 54),
                (11, 61),
                (12, 66),
                (13, 71),
                (14, 78),
                (15, 85)
            ][..],
            &vec_of_tab_elms(s)[..]
        );

        let s = "$s$1ένα${2}$τρία$3${4}${5}$6τέσσερα${7}$8πέντε$9$$$10$$${11}$$$12$$$13$$${14}$$${15}";
        assert_eq!(
            &[
                (1, 4),
                (2, 14),
                (3, 25),
                (4, 29),
                (5, 33),
                (6, 35),
                (7, 53),
                (8, 55),
                (9, 67),
                (10, 72),
                (11, 79),
                (12, 84),
                (13, 89),
                (14, 96),
                (15, 103)
            ][..],
            &vec_of_tab_elms(s)[..]
        );
    }

    #[test]
    fn test_extract_placeholder() {
        use super::SnippetElement::*;
        let s1 = "${1:first ${2:second ${3:third ${4:fourth ${5:fifth}}}}}";

        assert_eq!(
            (
                PlaceHolder(
                    1,
                    vec![
                        Text("first ".into()),
                        PlaceHolder(
                            2,
                            vec![
                                Text("second ".into()),
                                PlaceHolder(
                                    3,
                                    vec![
                                        Text("third ".into()),
                                        PlaceHolder(
                                            4,
                                            vec![
                                                Text("fourth ".into()),
                                                PlaceHolder(
                                                    5,
                                                    vec![Text("fifth".into())]
                                                )
                                            ]
                                        )
                                    ]
                                )
                            ]
                        )
                    ]
                ),
                56
            ),
            Snippet::extract_placeholder(s1, 0).unwrap()
        );

        let s1 = "${1:first}${2:second}${3:third }${4:fourth ${5:fifth}}}}}";
        assert_eq!(
            (PlaceHolder(1, vec![Text("first".to_owned())]), 10),
            Snippet::extract_placeholder(s1, 0).unwrap()
        );
        assert_eq!(
            (PlaceHolder(2, vec![Text("second".to_owned())]), 21),
            Snippet::extract_placeholder(s1, 10).unwrap()
        );
        assert_eq!(
            (PlaceHolder(3, vec![Text("third ".to_owned())]), 32),
            Snippet::extract_placeholder(s1, 21).unwrap()
        );

        assert_eq!(
            (
                PlaceHolder(
                    4,
                    vec![
                        Text("fourth ".into()),
                        PlaceHolder(5, vec![Text("fifth".into())])
                    ]
                ),
                54
            ),
            Snippet::extract_placeholder(s1, 32).unwrap()
        );
    }

    #[test]
    fn test_extract_text() {
        use SnippetElement::*;

        // 1. ====================================================================================

        let s = "start $1${2:second ${3:third}} $0";
        let (snip_elm, end) = Snippet::extract_text(s, 0, &['$'], &[]).unwrap();
        assert_eq!((Text("start ".to_owned()), 6), (snip_elm, end));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 1, &['$'], &[]).unwrap();
        assert_eq!((Text("1".to_owned()), 8), (snip_elm, end));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 1, &['$'], &[]).unwrap();
        assert_eq!((Text("{2:second ".to_owned()), 19), (snip_elm, end));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 1, &['$'], &[]).unwrap();
        assert_eq!((Text("{3:third}} ".to_owned()), 31), (snip_elm, end));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 1, &['$'], &[]).unwrap();
        assert_eq!((Text("0".to_owned()), 33), (snip_elm, end));

        // 2. ====================================================================================

        let s = "start $1${2:second ${3:third}} $0";

        let (snip_elm, end) = Snippet::extract_text(s, 0, &['{'], &[]).unwrap();
        assert_eq!((Text("start $1$".to_owned()), 9), (snip_elm, end));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 1, &['{'], &[]).unwrap();
        assert_eq!((Text("2:second $".to_owned()), 20), (snip_elm, end));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 1, &['{'], &[]).unwrap();
        assert_eq!((Text("3:third}} $0".to_owned()), 33), (snip_elm, end));

        // 3. ====================================================================================

        let s = "start $1${2:second ${3:third}} $0";

        let (snip_elm, end) = Snippet::extract_text(s, 0, &['}'], &[]).unwrap();
        assert_eq!(
            (Text("start $1${2:second ${3:third".to_owned()), 28),
            (snip_elm, end)
        );

        assert_eq!(None, Snippet::extract_text(s, end + 1, &['}'], &[]));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 2, &['}'], &[]).unwrap();
        assert_eq!((Text(" $0".to_owned()), 33), (snip_elm, end));

        // 4. ====================================================================================

        let s = "start $1${2:second ${3:third}} $0";

        let (snip_elm, end) = Snippet::extract_text(s, 0, &['\\'], &[]).unwrap();
        assert_eq!((Text(s.to_owned()), 33), (snip_elm, end));

        // 5. ====================================================================================

        let s = "start \\$1${2:second \\${3:third}} $0";

        let (snip_elm, end) =
            Snippet::extract_text(s, 0, &['$', '\\'], &[]).unwrap();
        assert_eq!((Text("start $1".to_owned()), 9), (snip_elm, end));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 1, &['$', '\\'], &[]).unwrap();
        assert_eq!(
            (Text("{2:second ${3:third}} ".to_owned()), 33),
            (snip_elm, end)
        );

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 1, &['$', '\\'], &[]).unwrap();
        assert_eq!((Text("0".to_owned()), 35), (snip_elm, end));

        // 6. ====================================================================================

        let s = "\\{start $1${2:second $\\{3:third}} $0}";

        let (snip_elm, end) =
            Snippet::extract_text(s, 0, &['{', '\\'], &[]).unwrap();
        assert_eq!((Text("{start $1$".to_owned()), 11), (snip_elm, end));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 1, &['{', '\\'], &[]).unwrap();
        assert_eq!(
            (Text("2:second ${3:third}} $0}".to_owned()), 37),
            (snip_elm, end)
        );

        // 7. ====================================================================================

        let s = "{start $1${2}:second $\\{3:third}} $0}";

        let (snip_elm, end) =
            Snippet::extract_text(s, 0, &['}', '\\'], &[]).unwrap();
        assert_eq!((Text("{start $1${2".to_owned()), 12), (snip_elm, end));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 1, &['}', '\\'], &[]).unwrap();
        assert_eq!((Text(":second $".to_owned()), 22), (snip_elm, end));

        assert_eq!(None, Snippet::extract_text(s, end, &['}', '\\'], &[]));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 1, &['}', '\\'], &[]).unwrap();
        assert_eq!((Text("{3:third".to_owned()), 31), (snip_elm, end));

        assert_eq!(None, Snippet::extract_text(s, end + 1, &['}', '\\'], &[]));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 2, &['}', '\\'], &[]).unwrap();
        assert_eq!((Text(" $0".to_owned()), 36), (snip_elm, end));

        // 8. ====================================================================================

        let s = "{start $1${2}:second $\\{3:third}} $0}";

        let (snip_elm, end) =
            Snippet::extract_text(s, 0, &['$', '\\'], &['}']).unwrap();
        assert_eq!((Text("{start ".to_owned()), 7), (snip_elm, end));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 1, &['$', '\\'], &['}']).unwrap();
        assert_eq!((Text("1".to_owned()), 9), (snip_elm, end));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 1, &['$', '\\'], &['}']).unwrap();
        assert_eq!((Text("{2}:second ".to_owned()), 21), (snip_elm, end));

        assert_eq!(
            None,
            Snippet::extract_text(s, end + 1, &['$', '\\'], &['}'])
        );

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 2, &['$', '\\'], &['}']).unwrap();
        assert_eq!((Text("{3:third}} ".to_owned()), 34), (snip_elm, end));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 1, &['$', '\\'], &['}']).unwrap();
        assert_eq!((Text("0}".to_owned()), 37), (snip_elm, end));

        // 9. ====================================================================================

        let s = "{start $1${2}:second $\\{3:third}} $0}";

        let (snip_elm, end) =
            Snippet::extract_text(s, 0, &['$', '}', '\\'], &[]).unwrap();
        assert_eq!((Text("{start ".to_owned()), 7), (snip_elm, end));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 1, &['$', '}', '\\'], &[]).unwrap();
        assert_eq!((Text("1".to_owned()), 9), (snip_elm, end));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 1, &['$', '}', '\\'], &[]).unwrap();
        assert_eq!((Text("{2".to_owned()), 12), (snip_elm, end));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 1, &['$', '}', '\\'], &[]).unwrap();
        assert_eq!((Text(":second ".to_owned()), 21), (snip_elm, end));

        assert_eq!(
            None,
            Snippet::extract_text(s, end + 1, &['$', '}', '\\'], &[])
        );

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 2, &['$', '}', '\\'], &[]).unwrap();
        assert_eq!((Text("{3:third".to_owned()), 31), (snip_elm, end));

        assert_eq!(
            None,
            Snippet::extract_text(s, end + 1, &['$', '}', '\\'], &[])
        );

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 2, &['$', '}', '\\'], &[]).unwrap();
        assert_eq!((Text(" ".to_owned()), 34), (snip_elm, end));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 1, &['$', '}', '\\'], &[]).unwrap();
        assert_eq!((Text("0".to_owned()), 36), (snip_elm, end));

        assert_eq!(
            None,
            Snippet::extract_text(s, end + 1, &['$', '}', '\\'], &[])
        );

        // 10. ====================================================================================

        let s = "{start $1${2}:second $\\{3:third}} $0}";

        assert_eq!(
            None,
            Snippet::extract_text(s, 0, &['$', '{', '}', '\\'], &[])
        );

        let (snip_elm, end) =
            Snippet::extract_text(s, 1, &['$', '{', '}', '\\'], &[]).unwrap();
        assert_eq!((Text("start ".to_owned()), 7), (snip_elm, end));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 1, &['$', '{', '}', '\\'], &[]).unwrap();
        assert_eq!((Text("1".to_owned()), 9), (snip_elm, end));

        assert_eq!(
            None,
            Snippet::extract_text(s, end + 1, &['$', '{', '}', '\\'], &[])
        );

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 2, &['$', '{', '}', '\\'], &[]).unwrap();
        assert_eq!((Text("2".to_owned()), 12), (snip_elm, end));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 1, &['$', '{', '}', '\\'], &[]).unwrap();
        assert_eq!((Text(":second ".to_owned()), 21), (snip_elm, end));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 1, &['$', '{', '}', '\\'], &[]).unwrap();
        assert_eq!((Text("{3:third".to_owned()), 31), (snip_elm, end));

        assert_eq!(
            None,
            Snippet::extract_text(s, end + 1, &['$', '{', '}', '\\'], &[])
        );

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 2, &['$', '{', '}', '\\'], &[]).unwrap();
        assert_eq!((Text(" ".to_owned()), 34), (snip_elm, end));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 1, &['$', '{', '}', '\\'], &[]).unwrap();
        assert_eq!((Text("0".to_owned()), 36), (snip_elm, end));

        assert_eq!(
            None,
            Snippet::extract_text(s, end + 1, &['$', '{', '}', '\\'], &[])
        );

        // 11. ====================================================================================

        let s = "{start\\\\ $1${2}:second\\ $\\{3:third}} $0}";

        assert_eq!(
            None,
            Snippet::extract_text(s, 0, &['$', '{', '}', '\\'], &[])
        );

        let (snip_elm, end) =
            Snippet::extract_text(s, 1, &['$', '{', '}', '\\'], &[]).unwrap();
        assert_eq!((Text("start\\ ".to_owned()), 9), (snip_elm, end));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 1, &['$', '{', '}', '\\'], &[]).unwrap();
        assert_eq!((Text("1".to_owned()), 11), (snip_elm, end));

        assert_eq!(
            None,
            Snippet::extract_text(s, end + 1, &['$', '{', '}', '\\'], &[])
        );

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 2, &['$', '{', '}', '\\'], &[]).unwrap();
        assert_eq!((Text("2".to_owned()), 14), (snip_elm, end));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 1, &['$', '{', '}', '\\'], &[]).unwrap();
        assert_eq!((Text(":second".to_owned()), 22), (snip_elm, end));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 1, &['$', '{', '}', '\\'], &[]).unwrap();
        assert_eq!((Text(" ".to_owned()), 24), (snip_elm, end));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 1, &['$', '{', '}', '\\'], &[]).unwrap();
        assert_eq!((Text("{3:third".to_owned()), 34), (snip_elm, end));

        assert_eq!(
            None,
            Snippet::extract_text(s, end + 1, &['$', '{', '}', '\\'], &[])
        );

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 2, &['$', '{', '}', '\\'], &[]).unwrap();
        assert_eq!((Text(" ".to_owned()), 37), (snip_elm, end));

        let (snip_elm, end) =
            Snippet::extract_text(s, end + 1, &['$', '{', '}', '\\'], &[]).unwrap();
        assert_eq!((Text("0".to_owned()), 39), (snip_elm, end));

        assert_eq!(
            None,
            Snippet::extract_text(s, end + 1, &['$', '{', '}', '\\'], &[])
        );
    }
}
