use jsonrpc_lite::{Id, JsonRpc, Params};
use parking_lot::Mutex;
use std::{
    io::BufRead, io::BufReader, io::BufWriter, io::Write, process::Command,
    process::Stdio, sync::Arc, thread,
};

pub struct LspClient {
    writer: Box<dyn Write + Send>,
}

impl LspClient {
    pub fn new() -> Arc<Mutex<LspClient>> {
        let mut process = Command::new("rust-anaylzer-mac")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("Error Occurred");

        let writer = Box::new(BufWriter::new(process.stdin.take().unwrap()));

        let lsp_client = Arc::new(Mutex::new(LspClient { writer }));

        let local_lsp_client = lsp_client.clone();
        let mut stdout = process.stdout;
        thread::spawn(move || {
            let mut reader = Box::new(BufReader::new(stdout.take().unwrap()));
            loop {
                match read_message(&mut reader) {
                    Ok(message_str) => {
                        local_lsp_client.lock().handle_message(message_str.as_ref());
                    }
                    Err(err) => eprintln!("Error occurred {:?}", err),
                };
            }
        });

        lsp_client
    }

    pub fn handle_message(&mut self, message: &str) {
        match JsonRpc::parse(message) {
            Ok(JsonRpc::Request(obj)) => {
                // trace!("client received unexpected request: {:?}", obj)
            }
            Ok(value @ JsonRpc::Notification(_)) => {
                //     self.handle_notification(
                //     value.get_method().unwrap(),
                //     value.get_params().unwrap(),
                // );
            }
            Ok(value @ JsonRpc::Success(_)) => {
                // let id = number_from_id(&value.get_id().unwrap());
                // let result = value.get_result().unwrap();
                // self.handle_response(id, Ok(result.clone()));
            }
            Ok(value @ JsonRpc::Error(_)) => {
                // let id = number_from_id(&value.get_id().unwrap());
                // let error = value.get_error().unwrap();
                // self.handle_response(id, Err(error.clone()));
            }
            Err(err) => eprintln!("Error in parsing incoming string: {}", err),
        }
    }
}

const HEADER_CONTENT_LENGTH: &str = "content-length";
const HEADER_CONTENT_TYPE: &str = "content-type";

pub enum LspHeader {
    ContentType,
    ContentLength(usize),
}

/// Type to represent errors occurred while parsing LSP RPCs
#[derive(Debug)]
pub enum ParseError {
    Io(std::io::Error),
    ParseInt(std::num::ParseIntError),
    Utf8(std::string::FromUtf8Error),
    Json(serde_json::Error),
    Unknown(String),
}

impl From<std::io::Error> for ParseError {
    fn from(err: std::io::Error) -> ParseError {
        ParseError::Io(err)
    }
}

impl From<std::string::FromUtf8Error> for ParseError {
    fn from(err: std::string::FromUtf8Error) -> ParseError {
        ParseError::Utf8(err)
    }
}

impl From<serde_json::Error> for ParseError {
    fn from(err: serde_json::Error) -> ParseError {
        ParseError::Json(err)
    }
}

impl From<std::num::ParseIntError> for ParseError {
    fn from(err: std::num::ParseIntError) -> ParseError {
        ParseError::ParseInt(err)
    }
}

impl From<String> for ParseError {
    fn from(s: String) -> ParseError {
        ParseError::Unknown(s)
    }
}

/// parse header from the incoming input string
fn parse_header(s: &str) -> Result<LspHeader, ParseError> {
    let split: Vec<String> =
        s.splitn(2, ": ").map(|s| s.trim().to_lowercase()).collect();
    if split.len() != 2 {
        return Err(ParseError::Unknown("Malformed".to_string()));
    };
    match split[0].as_ref() {
        HEADER_CONTENT_TYPE => Ok(LspHeader::ContentType),
        HEADER_CONTENT_LENGTH => Ok(LspHeader::ContentLength(
            usize::from_str_radix(&split[1], 10)?,
        )),
        _ => Err(ParseError::Unknown(
            "Unknown parse error occurred".to_string(),
        )),
    }
}

/// Blocking call to read a message from the provided BufRead
pub fn read_message<T: BufRead>(reader: &mut T) -> Result<String, ParseError> {
    let mut buffer = String::new();
    let mut content_length: Option<usize> = None;

    loop {
        buffer.clear();
        let _result = reader.read_line(&mut buffer);

        match &buffer {
            s if s.trim().is_empty() => break,
            s => {
                match parse_header(s)? {
                    LspHeader::ContentLength(len) => content_length = Some(len),
                    LspHeader::ContentType => (),
                };
            }
        };
    }

    let content_length = content_length
        .ok_or_else(|| format!("missing content-length header: {}", buffer))?;

    let mut body_buffer = vec![0; content_length];
    reader.read_exact(&mut body_buffer)?;

    let body = String::from_utf8(body_buffer)?;
    Ok(body)
}
