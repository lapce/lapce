use anyhow::{anyhow, Error, Result};
use core::fmt;
use lapce_core::{directory::Directory, movement::LineCol};
use lapce_rpc::{
    proxy::{ProxyMessage, ProxyNotification},
    RpcMessage,
};
use std::{
    fs,
    path::{Component, PathBuf},
};

#[derive(Default, Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PathObject {
    pub path: PathBuf,
    pub linecol: Option<LineCol>,
}

impl PathObject {
    pub fn new(path: PathBuf, line: usize, column: usize) -> PathObject {
        PathObject {
            path,
            linecol: Some(LineCol { line, column }),
        }
    }

    pub fn from_path(path: PathBuf) -> PathObject {
        PathObject {
            path,
            linecol: None,
        }
    }
}

pub fn parse_file_line_column(path: &str) -> Result<PathObject, Error> {
    let path = PathBuf::from(path);
    // Bail out quickly on existing path
    if let Ok(meta) = fs::metadata(&path) {
        if meta.is_file() || meta.is_dir() {
            return Ok(PathObject::from_path(path));
        }
    };
    let components = path.components();
    // Verify that last component is what could be a filename
    // otherwise bail out since it's an actual path
    match components.last() {
        Some(Component::Normal(_)) => {}
        _ => {
            return Ok(PathObject::from_path(path));
        }
    };
    if let Some(str) = path.to_str() {
        let mut splits = str.rsplit(':');
        if let Some(first_rhs) = splits.next() {
            if let Ok(first_rhs_num) = first_rhs.parse::<usize>() {
                if let Some(second_rhs) = splits.next() {
                    if let Ok(second_rhs_num) = second_rhs.parse::<usize>() {
                        let mut str = String::new();
                        write_text_with_sep_to(splits.rev(), &mut str, ":")?;
                        // NOTE: The last element is ":", and its ok, because even if we use &[&str] we need
                        // to check the length of a slice on each iteration
                        let left_path = PathBuf::from(&str[..str.len() - 1]);
                        if left_path.is_file() {
                            return Ok(PathObject::new(
                                left_path,
                                second_rhs_num,
                                first_rhs_num,
                            ));
                        }

                        str.push_str(second_rhs);
                        let left_path = PathBuf::from(&str);
                        if left_path.is_file() {
                            return Ok(PathObject::new(left_path, first_rhs_num, 1));
                        } else if path.is_file() {
                            return Ok(PathObject::from_path(path));
                        }
                    } else {
                        let mut str = String::new();
                        write_text_with_sep_to(splits.rev(), &mut str, ":")?;
                        // Last char of `str` is ":", so we neen to push only `second_rhs`
                        str.push_str(second_rhs);

                        return Ok(PathObject::new(
                            PathBuf::from(str),
                            first_rhs_num,
                            1,
                        ));
                    }
                } else {
                    return Ok(PathObject::from_path(path));
                }
            } else {
                return Ok(PathObject::from_path(path));
            }
        }
    }
    Ok(PathObject::from_path(path))
}

// FIXME: Unfortunately the last element will be ":", we need to think about how to handle
// it without having to allocate an unnecessary vector
fn write_text_with_sep_to<I, T, Buf>(
    mut iter: I,
    buf: &mut Buf,
    sep: T,
) -> fmt::Result
where
    Buf: fmt::Write,
    I: Iterator<Item = T>,
    T: AsRef<str>,
{
    if let Some(str) = iter.next() {
        buf.write_str(str.as_ref())?;
        buf.write_str(sep.as_ref())?;
        // call ourselves recursively
        write_text_with_sep_to(iter, buf, sep)?
    }
    fmt::Result::Ok(())
}

pub fn try_open_in_existing_process(paths: &[PathObject]) -> Result<()> {
    let local_socket = Directory::local_socket()
        .ok_or_else(|| anyhow!("can't get local socket folder"))?;
    let mut socket =
        interprocess::local_socket::LocalSocketStream::connect(local_socket)?;

    // Split user input into known existing directors and
    // file paths that exist or not
    let (folders, files): (Vec<PathBuf>, Vec<PathBuf>) = paths
        .iter()
        .map(|p| p.path.to_owned())
        .partition(|p| p.is_dir());

    let msg: ProxyMessage =
        RpcMessage::Notification(ProxyNotification::OpenPaths { folders, files });
    lapce_rpc::stdio::write_msg(&mut socket, msg)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::cli::PathObject;

    use super::parse_file_line_column;

    #[test]
    #[cfg(windows)]
    fn test_absolute_path() {
        assert_eq!(
            parse_file_line_column("C:\\Cargo.toml:55").unwrap(),
            PathObject::new(PathBuf::from("C:\\Cargo.toml"), 55, 1),
        );
    }

    #[test]
    #[cfg(windows)]
    fn test_relative_path() {
        assert_eq!(
            parse_file_line_column(".\\..\\Cargo.toml:55").unwrap(),
            PathObject::new(PathBuf::from(".\\..\\Cargo.toml"), 55, 1),
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_absolute_path() {
        assert_eq!(
            parse_file_line_column("/tmp/Cargo.toml:55").unwrap(),
            PathObject::new(PathBuf::from("/tmp/Cargo.toml"), 55, 1),
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_relative_path() {
        assert_eq!(
            parse_file_line_column("./lapce-core/../Cargo.toml").unwrap(),
            PathObject::from_path(PathBuf::from("./lapce-core/../Cargo.toml")),
        );
    }

    #[test]
    fn test_relative_path_with_line() {
        assert_eq!(
            parse_file_line_column("Cargo.toml:55").unwrap(),
            PathObject::new(PathBuf::from("Cargo.toml"), 55, 1),
        );
    }

    #[test]
    fn test_relative_path_with_linecol() {
        assert_eq!(
            parse_file_line_column("Cargo.toml:55:3").unwrap(),
            PathObject::new(PathBuf::from("Cargo.toml"), 55, 3),
        );
    }

    #[test]
    fn test_relative_path_with_none() {
        assert_eq!(
            parse_file_line_column("Cargo.toml:12:623:352").unwrap(),
            PathObject::from_path(PathBuf::from("Cargo.toml:12:623:352")),
        );
    }
}
