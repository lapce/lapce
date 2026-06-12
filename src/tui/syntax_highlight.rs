//! Syntax Highlighting V2 - Enhanced code highlighting
//!
//! Provides comprehensive syntax highlighting for multiple languages with:
//! - Token-based highlighting
//! - Multi-language support
//! - Theme-aware colors
//! - Custom token styling

use ratatui::style::{Color, Modifier, Style};
use std::collections::HashMap;

/// Syntax token types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenType {
    Keyword,
    String,
    Number,
    Comment,
    Function,
    Type,
    Variable,
    Operator,
    Punctuation,
    Property,
    Constant,
    Module,
    Attribute,
    Tag,
    Namespace,
    Regex,
}

/// Theme colors for syntax highlighting
#[derive(Debug, Clone)]
pub struct SyntaxTheme {
    pub keyword: Style,
    pub string: Style,
    pub number: Style,
    pub comment: Style,
    pub function: Style,
    pub type_name: Style,
    pub variable: Style,
    pub operator: Style,
    pub punctuation: Style,
    pub property: Style,
    pub constant: Style,
    pub module: Style,
    pub attribute: Style,
    pub tag: Style,
    pub namespace: Style,
    pub regex: Style,
    pub default: Style,
}

impl Default for SyntaxTheme {
    fn default() -> Self {
        Self::nord()
    }
}

impl SyntaxTheme {
    /// Nord theme - Cool blue tones
    pub fn nord() -> Self {
        Self {
            keyword: Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
            string: Style::default().fg(Color::Yellow),
            number: Style::default().fg(Color::Cyan),
            comment: Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
            function: Style::default().fg(Color::Blue),
            type_name: Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
            variable: Style::default().fg(Color::White),
            operator: Style::default().fg(Color::Red),
            punctuation: Style::default().fg(Color::White),
            property: Style::default().fg(Color::Cyan),
            constant: Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            module: Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD),
            attribute: Style::default().fg(Color::Yellow).add_modifier(Modifier::ITALIC),
            tag: Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            namespace: Style::default().fg(Color::Blue),
            regex: Style::default().fg(Color::Magenta),
            default: Style::default().fg(Color::White),
        }
    }

    /// One Dark theme - Dark with vibrant colors
    pub fn one_dark() -> Self {
        Self {
            keyword: Style::default().fg(Color(170)).add_modifier(Modifier::BOLD), // Purple
            string: Style::default().fg(Color(140)).add_modifier(Modifier::BOLD), // Orange
            number: Style::default().fg(Color(140)), // Orange
            comment: Style::default().fg(Color(110)).add_modifier(Modifier::ITALIC), // Gray
            function: Style::default().fg(Color(97)), // Blue
            type_name: Style::default().fg(Color(86)), // Teal
            variable: Style::default().fg(Color(255)), // White
            operator: Style::default().fg(Color(197)), // Pink
            punctuation: Style::default().fg(Color(255)), // White
            property: Style::default().fg(Color(86)), // Teal
            constant: Style::default().fg(Color(209)), // Yellow
            module: Style::default().fg(Color(81)), // Blue
            attribute: Style::default().fg(Color(140)), // Orange
            tag: Style::default().fg(Color(203)), // Red
            namespace: Style::default().fg(Color(81)), // Blue
            regex: Style::default().fg(Color(140)), // Orange
            default: Style::default().fg(Color(255)),
        }
    }

    /// Get style for token type
    pub fn get_style(&self, token_type: TokenType) -> Style {
        match token_type {
            TokenType::Keyword => self.keyword,
            TokenType::String => self.string,
            TokenType::Number => self.number,
            TokenType::Comment => self.comment,
            TokenType::Function => self.function,
            TokenType::Type => self.type_name,
            TokenType::Variable => self.variable,
            TokenType::Operator => self.operator,
            TokenType::Punctuation => self.punctuation,
            TokenType::Property => self.property,
            TokenType::Constant => self.constant,
            TokenType::Module => self.module,
            TokenType::Attribute => self.attribute,
            TokenType::Tag => self.tag,
            TokenType::Namespace => self.namespace,
            TokenType::Regex => self.regex,
        }
    }
}

/// Language configuration
#[derive(Debug, Clone)]
pub struct LanguageConfig {
    pub keywords: Vec<&'static str>,
    pub type_keywords: Vec<&'static str>,
    pub builtin_functions: Vec<&'static str>,
    pub string_delimiters: Vec<char>,
    pub comment_single: Option<&'static str>,
    pub comment_multi_start: Option<&'static str>,
    pub comment_multi_end: Option<&'static str>,
}

impl LanguageConfig {
    pub fn rust() -> Self {
        Self {
            keywords: vec![
                "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type", "unsafe", "use", "where", "while",
            ],
            type_keywords: vec![
                "i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16", "u32", "u64", "u128", "usize", "f32", "f64", "bool", "char", "str", "String", "Vec", "Option", "Result", "Box", "Rc", "Arc", "Cell", "RefCell", "HashMap", "HashSet", "BTreeMap", "BTreeSet",
            ],
            builtin_functions: vec![
                "println", "print", "eprintln", "format", "panic", "assert", "assert_eq", "assert_ne", "dbg", "vec", "panic", "todo", "unimplemented", "unreachable", "require", "cfg", "env", "file", "line", "column", "module_path",
            ],
            string_delimiters: vec!['"', '\''],
            comment_single: Some("//"),
            comment_multi_start: Some("/*"),
            comment_multi_end: Some("*/"),
        }
    }

    pub fn javascript() -> Self {
        Self {
            keywords: vec![
                "break", "case", "catch", "continue", "debugger", "default", "delete", "do", "else", "export", "extends", "finally", "for", "function", "if", "import", "in", "instanceof", "new", "return", "super", "switch", "this", "throw", "try", "typeof", "var", "void", "while", "with", "yield", "class", "const", "enum", "let", "static", "implements", "interface", "package", "private", "protected", "public", "async", "await",
            ],
            type_keywords: vec![
                "Array", "Boolean", "Date", "Error", "Function", "JSON", "Map", "Math", "Number", "Object", "Promise", "Proxy", "RegExp", "Set", "String", "Symbol", "WeakMap", "WeakSet", "any", "boolean", "never", "null", "object", "string", "symbol", "undefined", "void",
            ],
            builtin_functions: vec![
                "console", "alert", "confirm", "prompt", "setTimeout", "setInterval", "clearTimeout", "clearInterval", "fetch", "require", "module", "exports", "process", "Buffer", "setImmediate", "queueMicrotask",
            ],
            string_delimiters: vec!['"', '\'', '`'],
            comment_single: Some("//"),
            comment_multi_start: Some("/*"),
            comment_multi_end: Some("*/"),
        }
    }

    pub fn python() -> Self {
        Self {
            keywords: vec![
                "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del", "elif", "else", "except", "finally", "for", "from", "global", "if", "import", "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while", "with", "yield",
            ],
            type_keywords: vec![
                "int", "float", "str", "bool", "list", "dict", "set", "tuple", "bytes", "bytearray", "frozenset", "range", "slice", "type", "object", "None", "True", "False", "Ellipsis", "NotImplemented",
            ],
            builtin_functions: vec![
                "print", "input", "len", "range", "str", "int", "float", "list", "dict", "set", "tuple", "bool", "abs", "all", "any", "bin", "chr", "dir", "divmod", "enumerate", "eval", "exec", "filter", "format", "getattr", "hasattr", "hash", "help", "hex", "id", "input", "isinstance", "issubclass", "iter", "map", "max", "min", "next", "oct", "open", "ord", "pow", "repr", "reversed", "round", "setattr", "slice", "sorted", "sum", "super", "type", "vars", "zip",
            ],
            string_delimiters: vec!['"', '\'', '"', '"'],
            comment_single: Some("#"),
            comment_multi_start: Some("\"\"\""),
            comment_multi_end: Some("\"\"\""),
        }
    }

    pub fn typescript() -> Self {
        Self {
            keywords: vec![
                "break", "case", "catch", "continue", "debugger", "default", "delete", "do", "else", "export", "extends", "finally", "for", "function", "if", "import", "in", "instanceof", "new", "return", "super", "switch", "this", "throw", "try", "typeof", "var", "void", "while", "with", "yield", "class", "const", "enum", "let", "static", "implements", "interface", "package", "private", "protected", "public", "async", "await", "declare", "type", "namespace", "abstract", "as", "constructor", "get", "set", "readonly", "keyof", "infer", "typeof", "from", "of", "require", "module",
            ],
            type_keywords: vec![
                "Array", "Boolean", "Date", "Error", "Function", "JSON", "Map", "Math", "Number", "Object", "Promise", "Proxy", "RegExp", "Set", "String", "Symbol", "WeakMap", "WeakSet", "any", "boolean", "never", "null", "object", "string", "symbol", "undefined", "void", "unknown", "never", "enum", "tuple", "bigint",
            ],
            builtin_functions: vec![
                "console", "alert", "confirm", "prompt", "setTimeout", "setInterval", "clearTimeout", "clearInterval", "fetch", "require", "module", "exports", "process", "Buffer", "setImmediate", "queueMicrotask", "ArrayBuffer", "DataView", "Float32Array", "Float64Array", "Int8Array", "Int16Array", "Int32Array", "Uint8Array", "Uint16Array", "Uint32Array", "Uint8ClampedArray",
            ],
            string_delimiters: vec!['"', '\'', '`'],
            comment_single: Some("//"),
            comment_multi_start: Some("/*"),
            comment_multi_end: Some("*/"),
        }
    }

    pub fn go() -> Self {
        Self {
            keywords: vec![
                "break", "case", "chan", "const", "continue", "default", "defer", "else", "fallthrough", "for", "func", "go", "goto", "if", "import", "interface", "map", "package", "range", "return", "select", "struct", "switch", "type", "var",
            ],
            type_keywords: vec![
                "bool", "byte", "complex64", "complex128", "error", "float32", "float64", "int", "int8", "int16", "int32", "int64", "rune", "string", "uint", "uint8", "uint16", "uint32", "uint64", "uintptr", "any", "comparable",
            ],
            builtin_functions: vec![
                "append", "cap", "close", "complex", "copy", "delete", "imag", "len", "make", "new", "panic", "print", "println", "real", "recover", "append", "clear", "min", "max",
            ],
            string_delimiters: vec!['"', '\'', '`'],
            comment_single: Some("//"),
            comment_multi_start: Some("/*"),
            comment_multi_end: Some("*/"),
        }
    }
}

/// Token with position and style
#[derive(Debug, Clone)]
pub struct StyledToken {
    pub text: String,
    pub token_type: TokenType,
    pub style: Style,
    pub start_col: usize,
    pub end_col: usize,
}

/// Syntax highlighter
pub struct SyntaxHighlighterV2 {
    themes: HashMap<String, SyntaxTheme>,
    languages: HashMap<String, LanguageConfig>,
    current_theme: String,
}

impl SyntaxHighlighterV2 {
    pub fn new() -> Self {
        let mut themes = HashMap::new();
        themes.insert("nord".to_string(), SyntaxTheme::nord());
        themes.insert("one-dark".to_string(), SyntaxTheme::one_dark());
        themes.insert("default".to_string(), SyntaxTheme::default());

        let mut languages = HashMap::new();
        languages.insert("rust".to_string(), LanguageConfig::rust());
        languages.insert("javascript".to_string(), LanguageConfig::javascript());
        languages.insert("js".to_string(), LanguageConfig::javascript());
        languages.insert("typescript".to_string(), LanguageConfig::typescript());
        languages.insert("ts".to_string(), LanguageConfig::typescript());
        languages.insert("python".to_string(), LanguageConfig::python());
        languages.insert("py".to_string(), LanguageConfig::python());
        languages.insert("go".to_string(), LanguageConfig::go());

        Self {
            themes,
            languages,
            current_theme: "nord".to_string(),
        }
    }

    /// Set current theme
    pub fn set_theme(&mut self, theme_name: &str) {
        if self.themes.contains_key(theme_name) {
            self.current_theme = theme_name.to_string();
        }
    }

    /// Get current theme
    pub fn current_theme(&self) -> &str {
        &self.current_theme
    }

    /// List available themes
    pub fn available_themes(&self) -> Vec<&str> {
        self.themes.keys().map(|s| s.as_str()).collect()
    }

    /// Highlight a line of code
    pub fn highlight_line(&self, line: &str, language: &str) -> Vec<StyledToken> {
        let lang_config = self.languages.get(language).or_else(|| self.languages.get("javascript")).expect("unwrap failed: syntax_highlight.rs:295");
        let theme = self.themes.get(&self.current_theme).expect("unwrap failed: syntax_highlight.rs:296");
        let mut tokens = Vec::new();
        let mut remaining = line;
        let mut col = 0;

        while !remaining.is_empty() {
            let (token_text, token_type) = self.tokenize_token(remaining, lang_config, col);
            let token_len = token_text.len();
            let style = theme.get_style(token_type);

            tokens.push(StyledToken {
                text: token_text.clone(),
                token_type,
                style,
                start_col: col,
                end_col: col + token_len,
            });

            remaining = &remaining[token_len..];
            col += token_len;
        }

        tokens
    }

    /// Tokenize a single token from remaining input
    fn tokenize_token(&self, input: &str, config: &LanguageConfig, _col: usize) -> (String, TokenType) {
        let chars: Vec<char> = input.chars().collect();
        
        // Empty check
        if chars.is_empty() {
            return (String::new(), TokenType::default_token_type());
        }

        // Single character tokens
        let c = chars[0];

        // Whitespace
        if c.is_whitespace() {
            let mut len = 1;
            while len < chars.len() && chars[len].is_whitespace() {
                len += 1;
            }
            return (input[..len].to_string(), TokenType::Punctuation);
        }

        // Operators
        if "+-*/%=<>!&|^~?:".contains(c) {
            let mut len = 1;
            while len < chars.len() && "+-*/%=<>!&|^~?:".contains(chars[len]) {
                len += 1;
            }
            return (input[..len].to_string(), TokenType::Operator);
        }

        // Punctuation
        if "{}[]();,.".contains(c) {
            return (c.to_string(), TokenType::Punctuation);
        }

        // Numbers
        if c.is_numeric() || (c == '.' && chars.len() > 1 && chars[1].is_numeric()) {
            let mut len = 1;
            let mut has_dot = c == '.';
            while len < chars.len() {
                if chars[len].is_numeric() {
                    len += 1;
                } else if chars[len] == '.' && !has_dot {
                    has_dot = true;
                    len += 1;
                } else if chars[len] == 'e' || chars[len] == 'E' {
                    len += 1;
                    if len < chars.len() && (chars[len] == '+' || chars[len] == '-') {
                        len += 1;
                    }
                } else if chars[len] == 'x' || chars[len] == 'X' || chars[len] == 'b' || chars[len] == 'B' {
                    len += 1;
                } else if chars[len].is_ascii_hexdigit() {
                    len += 1;
                } else {
                    break;
                }
            }
            return (input[..len].to_string(), TokenType::Number);
        }

        // Identifiers and keywords
        if c.is_alphabetic() || c == '_' {
            let mut len = 1;
            while len < chars.len() && (chars[len].is_alphanumeric() || chars[len] == '_') {
                len += 1;
            }
            let word = &input[..len];

            // Check keywords
            if config.keywords.contains(&word.as_str()) {
                return (word.to_string(), TokenType::Keyword);
            }

            // Check type keywords
            if config.type_keywords.contains(&word.as_str()) {
                return (word.to_string(), TokenType::Type);
            }

            // Check builtin functions
            if config.builtin_functions.contains(&word.as_str()) {
                return (word.to_string(), TokenType::Function);
            }

            // Check if starts with uppercase (likely a type)
            if word.chars().next().map_or(false, |c| c.is_uppercase()) {
                return (word.to_string(), TokenType::Type);
            }

            // Check if followed by ( (likely a function)
            if len < input.len() && input[len..].starts_with('(') {
                return (word.to_string(), TokenType::Function);
            }

            return (word.to_string(), TokenType::Variable);
        }

        // Strings
        if config.string_delimiters.contains(&c) {
            let quote = c;
            let mut len = 1;
            
            // Handle triple quotes
            let is_triple = len + 2 <= input.len() && input[len..len+2] == format!("{}{}", quote, quote);
            if is_triple {
                let triple = format!("{}{}{}", quote, quote, quote);
                if let Some(end) = input[3..].find(&triple) {
                    return (input[..end + 6].to_string(), TokenType::String);
                }
                return (input.to_string(), TokenType::String);
            }

            // Single quote string
            while len < chars.len() {
                if chars[len] == '\\' && len + 1 < chars.len() {
                    len += 2;
                } else if chars[len] == quote {
                    len += 1;
                    break;
                } else {
                    len += 1;
                }
            }
            return (input[..len].to_string(), TokenType::String);
        }

        // Comments
        if let Some(single_comment) = config.comment_single {
            if input.starts_with(single_comment) {
                return (input.to_string(), TokenType::Comment);
            }
        }

        if let (Some(start), Some(end)) = (config.comment_multi_start, config.comment_multi_end) {
            if input.starts_with(start) {
                if let Some(end_pos) = input.find(end) {
                    return (input[..end_pos + end.len()].to_string(), TokenType::Comment);
                }
                return (input.to_string(), TokenType::Comment);
            }
        }

        // Default: single character
        (c.to_string(), TokenType::default_token_type())
    }

    /// Highlight multiple lines
    pub fn highlight_lines(&self, code: &str, language: &str) -> Vec<Vec<StyledToken>> {
        code.lines().map(|line| self.highlight_line(line, language)).collect()
    }
}

impl Default for SyntaxHighlighterV2 {
    fn default() -> Self {
        Self::new()
    }
}

impl TokenType {
    pub fn default_token_type() -> Self {
        TokenType::Variable
    }
}
