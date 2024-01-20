use core::str::FromStr;
use std::collections::HashMap;

/// If the character is an opening bracket return Some(true), if closing, return Some(false)
pub fn matching_pair_direction(c: char) -> Option<bool> {
    Some(match c {
        '{' => true,
        '}' => false,
        '(' => true,
        ')' => false,
        '[' => true,
        ']' => false,
        _ => return None,
    })
}

pub fn matching_char(c: char) -> Option<char> {
    Some(match c {
        '{' => '}',
        '}' => '{',
        '(' => ')',
        ')' => '(',
        '[' => ']',
        ']' => '[',
        _ => return None,
    })
}

/// If the given character is a parenthesis, returns its matching bracket
pub fn matching_bracket_general<R: ToStaticTextType>(char: char) -> Option<R>
where
    &'static str: ToStaticTextType<R>,
{
    let pair = match char {
        '{' => "}",
        '}' => "{",
        '(' => ")",
        ')' => "(",
        '[' => "]",
        ']' => "[",
        _ => return None,
    };
    Some(pair.to_static())
}

pub trait ToStaticTextType<R: 'static = Self>: 'static {
    fn to_static(self) -> R;
}

impl ToStaticTextType for &'static str {
    #[inline]
    fn to_static(self) -> &'static str {
        self
    }
}

impl ToStaticTextType<char> for &'static str {
    #[inline]
    fn to_static(self) -> char {
        char::from_str(self).unwrap()
    }
}

impl ToStaticTextType<String> for &'static str {
    #[inline]
    fn to_static(self) -> String {
        self.to_string()
    }
}

impl ToStaticTextType for char {
    #[inline]
    fn to_static(self) -> char {
        self
    }
}

impl ToStaticTextType for String {
    #[inline]
    fn to_static(self) -> String {
        self
    }
}

pub fn has_unmatched_pair(line: &str) -> bool {
    let mut count = HashMap::new();
    let mut pair_first = HashMap::new();
    for c in line.chars().rev() {
        if let Some(left) = matching_pair_direction(c) {
            let key = if left { c } else { matching_char(c).unwrap() };
            let pair_count = *count.get(&key).unwrap_or(&0i32);
            pair_first.entry(key).or_insert(left);
            if left {
                count.insert(key, pair_count - 1);
            } else {
                count.insert(key, pair_count + 1);
            }
        }
    }
    for (_, pair_count) in count.iter() {
        if *pair_count < 0 {
            return true;
        }
    }
    for (_, left) in pair_first.iter() {
        if *left {
            return true;
        }
    }
    false
}

pub fn str_is_pair_left(c: &str) -> bool {
    if c.chars().count() == 1 {
        let c = c.chars().next().unwrap();
        if matching_pair_direction(c).unwrap_or(false) {
            return true;
        }
    }
    false
}

pub fn str_matching_pair(c: &str) -> Option<char> {
    if c.chars().count() == 1 {
        let c = c.chars().next().unwrap();
        return matching_char(c);
    }
    None
}
