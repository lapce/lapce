use anyhow::{anyhow, Result};
use ssh2::Session;
use std::io::prelude::*;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::str::FromStr;

pub struct SshSession {
    pub session: Session,
    pub pwd: PathBuf,
}

pub struct SshPathEntry {
    pub path: PathBuf,
    pub is_dir: bool,
}

impl SshSession {
    pub fn new(host: &str) -> Result<SshSession> {
        let tcp = TcpStream::connect(host)?;
        let mut session = Session::new()?;
        session.set_tcp_stream(tcp);
        session.handshake()?;
        let path = PathBuf::from_str("/Users/Lulu/.ssh/id_rsa")?;
        session.userauth_pubkey_file("dz", None, path.as_path(), None)?;

        let mut channel = session.channel_session()?;
        channel.exec("pwd")?;
        let mut s = String::new();
        channel.read_to_string(&mut s)?;
        let pwd = PathBuf::from(&s.split("\n").collect::<Vec<&str>>()[0]);

        Ok(SshSession { session, pwd })
    }

    pub fn read_file(&self, path: &str) -> Result<Vec<u8>> {
        let (mut remote_file, stat) = self.session.scp_recv(Path::new(path))?;
        let mut contents = Vec::new();
        remote_file.read_to_end(&mut contents)?;
        Ok(contents)
    }

    pub fn read_dirs(&self, path: &PathBuf) -> Result<Vec<PathBuf>> {
        let mut channel = self.session.channel_session()?;
        channel.exec(&format!("ls -p {}", path.to_str().unwrap()))?;
        let mut s = String::new();
        channel.read_to_string(&mut s)?;
        let mut paths = Vec::new();
        for part in s.split("\n") {
            let part = part.trim();
            if part.ends_with("/") {
                paths.push(path.join(part));
            }
        }
        Ok(paths)
    }

    pub fn read_dir(&self, path: &str) -> Result<Vec<String>> {
        let mut channel = self.session.channel_session()?;
        channel.exec(&format!("ls -URp {}", path))?;
        let mut s = String::new();
        channel.read_to_string(&mut s)?;
        let mut base_path = PathBuf::new();
        let mut paths = Vec::new();
        for part in s.split("\n") {
            let part = part.trim();
            if part == "" {
                continue;
            }
            if part.ends_with(":") {
                let str_len = part.len();
                base_path = PathBuf::from(&part[..str_len - 1]);
            } else if part.ends_with("/") {
                continue;
            } else {
                if let Some(path) = base_path.join(part).to_str() {
                    paths.push(path.to_string());
                }
            }
        }
        Ok(paths)
    }
}
