use std::{
    borrow::Cow,
    ffi::OsString,
    fs,
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::SystemTime,
};

use anyhow::{anyhow, bail, Result};
use encoding_rs::Encoding;
use floem_editor_core::buffer::rope_text::CharIndicesJoin;
use lapce_core::encoding::offset_utf8_to_utf16;
use lapce_rpc::buffer::BufferId;
use lapce_xi_rope::{interval::IntervalBounds, rope::Rope, RopeDelta};
use lsp_types::*;
use tracing::{trace, TraceLevel};

#[derive(Clone)]
pub struct Buffer {
    pub language_id: &'static str,
    pub read_only: bool,
    pub id: BufferId,
    pub rope: Rope,
    pub path: PathBuf,
    pub rev: u64,
    pub mod_time: Option<SystemTime>,
    pub encoding: String,
}

pub const ENCODING_UTF8: &str = "UTF-8";

impl Buffer {
    pub fn new(id: BufferId, path: PathBuf) -> Buffer {
        let (content, encoding, read_only) = match load_file(&path) {
            Ok((string, encoding)) => (string, encoding, false),
            Err(err) => match err.downcast_ref::<std::io::Error>() {
                Some(err) => match err.kind() {
                    std::io::ErrorKind::PermissionDenied => (
                        "Permission Denied".to_string(),
                        ENCODING_UTF8.to_owned(),
                        true,
                    ),
                    std::io::ErrorKind::NotFound => {
                        ("".to_string(), ENCODING_UTF8.to_owned(), false)
                    }
                    _ => {
                        ("Not Supported".to_string(), ENCODING_UTF8.to_owned(), true)
                    }
                },
                None => {
                    ("Not Supported".to_string(), ENCODING_UTF8.to_owned(), true)
                }
            },
        };
        let rope = Rope::from(content);
        let rev = u64::from(!rope.is_empty());
        let language_id = language_id_from_path(&path).unwrap_or("");
        let mod_time = get_mod_time(&path);
        Buffer {
            id,
            rope,
            read_only,
            path,
            language_id,
            rev,
            mod_time,
            encoding,
        }
    }

    pub fn save(&mut self, rev: u64, create_parents: bool) -> Result<()> {
        if self.read_only {
            return Err(anyhow!("can't save to read only file"));
        }

        if self.rev != rev {
            return Err(anyhow!("not the right rev"));
        }
        let bak_extension = self.path.extension().map_or_else(
            || OsString::from("bak"),
            |ext| {
                let mut ext = ext.to_os_string();
                ext.push(".bak");
                ext
            },
        );
        let path = if self.path.is_symlink() {
            self.path.canonicalize()?
        } else {
            self.path.clone()
        };
        let new_file = !path.exists();

        let bak_file_path = &path.with_extension(bak_extension);
        if !new_file {
            fs::copy(&path, bak_file_path)?;
        }

        if create_parents {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
        }

        let mut f = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&path)?;

        match self.encoding.as_str() {
            "UTF-8" => {
                for chunk in self.rope.iter_chunks(..self.rope.len()) {
                    f.write_all(chunk.as_bytes())?;
                }
            }
            v => {
                use encoding_rs::CoderResult;

                let Some(enc) = Encoding::for_label(v.as_bytes()) else {
                    bail!("Failed to obtain valid encoding to use for saving file");
                };
                let mut encoder = enc.new_encoder();

                let mut buf = vec![];
                let mut last = false;
                let mut i = 0;
                for chunk in self.rope.iter_chunks(..self.rope.len()) {
                    i += chunk.len();
                    if i >= self.rope.len() {
                        last = true
                    }
                    let (result, units, replaced) =
                        encoder.encode_from_utf8_to_vec(chunk, &mut buf, last);
                    trace!(TraceLevel::TRACE, "{units} processed");
                    if replaced {
                        trace!(TraceLevel::WARN, "replacement occured in `{chunk}`");
                    }
                    match result {
                        CoderResult::InputEmpty => match last {
                            true => f.write_all(buf.as_slice())?,
                            false => continue,
                        },
                        CoderResult::OutputFull => f.write_all(buf.as_slice())?,
                    }
                }
            }
        }

        self.mod_time = get_mod_time(&path);
        if !new_file {
            fs::remove_file(bak_file_path)?;
        }

        Ok(())
    }

    pub fn update(
        &mut self,
        delta: &RopeDelta,
        rev: u64,
    ) -> Option<TextDocumentContentChangeEvent> {
        if self.rev + 1 != rev {
            return None;
        }
        self.rev += 1;
        let content_change = get_document_content_changes(delta, self);
        self.rope = delta.apply(&self.rope);
        Some(
            content_change.unwrap_or_else(|| TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: self.get_document(),
            }),
        )
    }

    pub fn get_document(&self) -> String {
        self.rope.to_string()
    }

    pub fn offset_of_line(&self, offset: usize) -> usize {
        self.rope.offset_of_line(offset)
    }

    pub fn line_of_offset(&self, offset: usize) -> usize {
        self.rope.line_of_offset(offset)
    }

    pub fn offset_to_line_col(&self, offset: usize) -> (usize, usize) {
        let line = self.line_of_offset(offset);
        (line, offset - self.offset_of_line(line))
    }

    /// Converts a UTF8 offset to a UTF16 LSP position  
    pub fn offset_to_position(&self, offset: usize) -> Position {
        let (line, col) = self.offset_to_line_col(offset);
        // Get the offset of line to make the conversion cheaper, rather than working
        // from the very start of the document to `offset`
        let line_offset = self.offset_of_line(line);
        let utf16_col =
            offset_utf8_to_utf16(self.char_indices_iter(line_offset..), col);

        Position {
            line: line as u32,
            character: utf16_col as u32,
        }
    }

    pub fn slice_to_cow<T: IntervalBounds>(&self, range: T) -> Cow<str> {
        self.rope.slice_to_cow(range)
    }

    /// Iterate over (utf8_offset, char) values in the given range  
    /// This uses `iter_chunks` and so does not allocate, compared to `slice_to_cow` which can
    pub fn char_indices_iter<T: IntervalBounds>(
        &self,
        range: T,
    ) -> impl Iterator<Item = (usize, char)> + '_ {
        CharIndicesJoin::new(self.rope.iter_chunks(range).map(str::char_indices))
    }

    pub fn len(&self) -> usize {
        self.rope.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

pub fn load_file(path: &Path) -> Result<(String, String)> {
    read_path_to_string(path)
}

pub fn read_path_to_string<P: AsRef<Path>>(path: P) -> Result<(String, String)> {
    let path = path.as_ref();
    let mut encoding = ENCODING_UTF8.to_string();

    let mut file = File::open(path)?;
    // Read the file in as bytes
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;

    let mut contents = String::new();
    // Parse the file contents as utf8
    match String::from_utf8(buffer.clone()) {
        Ok(s) => {
            contents = s;
        }
        Err(e) => {
            trace!(TraceLevel::ERROR, "Failed to decode from `utf-8`: {e}");

            use charset_normalizer_rs as chardet;
            use encoding_rs_io::DecodeReaderBytesBuilder;

            let mut enc_reader = DecodeReaderBytesBuilder::new();

            let chardet = chardet::from_bytes(&buffer, None);

            if let Some(charset) = chardet.get_best() {
                let enc = charset.encoding();
                trace!(TraceLevel::INFO, "Detected encoding: {enc}");
                enc_reader.encoding(Encoding::for_label(enc.as_bytes()));
                enc.clone_into(&mut encoding);
            };

            let mut decoder = enc_reader.build(buffer.as_slice());

            decoder.read_to_string(&mut contents)?;
        }
    };

    Ok((contents.to_owned(), encoding))
}

pub fn language_id_from_path(path: &Path) -> Option<&'static str> {
    // recommended language_id values
    // https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#textDocumentItem
    Some(match path.extension() {
        Some(ext) => {
            match ext.to_str()? {
                "C" | "H" => "cpp",
                "M" => "objective-c",
                // stop case-sensitive matching
                ext => match ext.to_lowercase().as_str() {
                    "bat" => "bat",
                    "clj" | "cljs" | "cljc" | "edn" => "clojure",
                    "coffee" => "coffeescript",
                    "c" | "h" => "c",
                    "cpp" | "hpp" | "cxx" | "hxx" | "c++" | "h++" | "cc" | "hh" => {
                        "cpp"
                    }
                    "cs" | "csx" => "csharp",
                    "css" => "css",
                    "d" | "di" | "dlang" => "dlang",
                    "diff" | "patch" => "diff",
                    "dart" => "dart",
                    "dockerfile" => "dockerfile",
                    "elm" => "elm",
                    "ex" | "exs" => "elixir",
                    "erl" | "hrl" => "erlang",
                    "fs" | "fsi" | "fsx" | "fsscript" => "fsharp",
                    "git-commit" | "git-rebase" => "git",
                    "go" => "go",
                    "groovy" | "gvy" | "gy" | "gsh" => "groovy",
                    "hbs" => "handlebars",
                    "htm" | "html" | "xhtml" => "html",
                    "ini" => "ini",
                    "java" | "class" => "java",
                    "js" => "javascript",
                    "jsx" => "javascriptreact",
                    "json" => "json",
                    "jl" => "julia",
                    "kt" | "kts" => "kotlin",
                    "less" => "less",
                    "lua" => "lua",
                    "makefile" | "gnumakefile" => "makefile",
                    "md" | "markdown" => "markdown",
                    "m" => "objective-c",
                    "mm" => "objective-cpp",
                    "plx" | "pl" | "pm" | "xs" | "t" | "pod" | "cgi" => "perl",
                    "p6" | "pm6" | "pod6" | "t6" | "raku" | "rakumod"
                    | "rakudoc" | "rakutest" => "perl6",
                    "php" | "phtml" | "pht" | "phps" => "php",
                    "proto" => "proto",
                    "ps1" | "ps1xml" | "psc1" | "psm1" | "psd1" | "pssc"
                    | "psrc" => "powershell",
                    "py" | "pyi" | "pyc" | "pyd" | "pyw" => "python",
                    "r" => "r",
                    "rb" => "ruby",
                    "rs" => "rust",
                    "scss" | "sass" => "scss",
                    "sc" | "scala" => "scala",
                    "sh" | "bash" | "zsh" => "shellscript",
                    "sql" => "sql",
                    "swift" => "swift",
                    "svelte" => "svelte",
                    "thrift" => "thrift",
                    "toml" => "toml",
                    "ts" => "typescript",
                    "tsx" => "typescriptreact",
                    "tex" => "tex",
                    "vb" => "vb",
                    "xml" | "csproj" => "xml",
                    "xsl" => "xsl",
                    "yml" | "yaml" => "yaml",
                    "zig" => "zig",
                    "vue" => "vue",
                    _ => return None,
                },
            }
        }
        // Handle paths without extension
        #[allow(clippy::match_single_binding)]
        None => match path.file_name()?.to_str()? {
            // case-insensitive matching
            filename => match filename.to_lowercase().as_str() {
                "dockerfile" => "dockerfile",
                "makefile" | "gnumakefile" => "makefile",
                _ => return None,
            },
        },
    })
}

fn get_document_content_changes(
    delta: &RopeDelta,
    buffer: &Buffer,
) -> Option<TextDocumentContentChangeEvent> {
    let (interval, _) = delta.summary();
    let (start, end) = interval.start_end();

    // TODO: Handle more trivial cases like typing when there's a selection or transpose
    if let Some(node) = delta.as_simple_insert() {
        let (start, end) = interval.start_end();
        let start = buffer.offset_to_position(start);

        let end = buffer.offset_to_position(end);

        Some(TextDocumentContentChangeEvent {
            range: Some(Range { start, end }),
            range_length: None,
            text: String::from(node),
        })
    }
    // Or a simple delete
    else if delta.is_simple_delete() {
        let end_position = buffer.offset_to_position(end);

        let start = buffer.offset_to_position(start);

        Some(TextDocumentContentChangeEvent {
            range: Some(Range {
                start,
                end: end_position,
            }),
            range_length: None,
            text: String::new(),
        })
    } else {
        None
    }
}

/// Returns the modification timestamp for the file at a given path,
/// if present.
pub fn get_mod_time<P: AsRef<Path>>(path: P) -> Option<SystemTime> {
    File::open(path)
        .and_then(|f| f.metadata())
        .and_then(|meta| meta.modified())
        .ok()
}
