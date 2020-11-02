use anyhow::{anyhow, Result};
use mio::net::TcpStream as MioTcpStream;
use mio::{Events, Interest, Poll, Token};
use parking_lot::Mutex;
use ssh2::{Channel, Session, Stream};
use std::io::prelude::*;
use std::io::{self, Read, Write};
use std::net::SocketAddr;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;

pub struct SshSession {
    pub session: Session,
    pub poll: Arc<Mutex<Poll>>,
    pub events: Events,
}

pub struct SshStream {
    pub stream: Stream,
    pub poll: Arc<Mutex<Poll>>,
    pub events: Events,
}

pub struct SshPathEntry {
    pub path: PathBuf,
    pub is_dir: bool,
}

impl SshSession {
    pub fn new(host: &str) -> Result<SshSession> {
        let mut session = Session::new()?;
        let addr: SocketAddr = host.parse()?;
        let mut tcp = MioTcpStream::connect(addr)?;
        let poll = Poll::new()?;
        let events = Events::with_capacity(1024);
        poll.registry().register(
            &mut tcp,
            Token(0),
            Interest::READABLE | Interest::WRITABLE,
        )?;
        session.set_tcp_stream(tcp);
        session.set_blocking(false);
        let mut ssh_session = SshSession {
            session,
            poll: Arc::new(Mutex::new(poll)),
            events,
        };
        ssh_session.handshake()?;
        ssh_session.auth()?;
        Ok(ssh_session)
    }

    pub fn handshake(&mut self) -> Result<()> {
        println!("start handshake");
        loop {
            self.poll.lock().poll(&mut self.events, None)?;
            for event in &self.events {
                match event.token() {
                    Token(0) => {
                        if let Err(e) = self.session.handshake() {
                            let e = io::Error::from(e);
                            if e.kind() == io::ErrorKind::WouldBlock {
                                continue;
                            } else {
                                println!("handshake err {}", e);
                                return Err(anyhow!("{}", e));
                            }
                        } else {
                            println!(" handshake success");
                            return Ok(());
                        }
                    }
                    _ => (),
                }
            }
        }
    }

    pub fn auth(&mut self) -> Result<()> {
        println!("start auth");
        let path = PathBuf::from_str("/Users/Lulu/.ssh/id_rsa")?;
        loop {
            if let Err(e) =
                self.session
                    .userauth_pubkey_file("dz", None, path.as_path(), None)
            {
                let e = io::Error::from(e);
                if e.kind() == io::ErrorKind::WouldBlock {
                } else {
                    println!("auth err {}", e);
                    return Err(anyhow!("{}", e));
                }
            } else {
                println!("auth success");
                return Ok(());
            }
        }
    }

    pub fn channel_exec(&mut self, channel: &mut Channel, cmd: &str) -> Result<()> {
        loop {
            if let Err(e) = channel.exec(cmd) {
                let e = io::Error::from(e);
                if e.kind() == io::ErrorKind::WouldBlock {
                } else {
                    return Err(anyhow!("{}", e));
                }
            } else {
                return Ok(());
            }
        }
        //loop {
        //    self.poll.lock().poll(&mut self.events, None)?;
        //    for event in &self.events {
        //        match event.token() {
        //            Token(0) => {
        //                if let Err(e) = channel.exec(cmd) {
        //                    let e = io::Error::from(e);
        //                    if e.kind() == io::ErrorKind::WouldBlock {
        //                        continue;
        //                    } else {
        //                        return Err(anyhow!("{}", e));
        //                    }
        //                } else {
        //                    return Ok(());
        //                }
        //            }
        //            _ => (),
        //        }
        //    }
        //}
    }

    pub fn get_stream(&mut self, channel: &Channel) -> SshStream {
        SshStream::new(channel.stream(0), self.poll.clone())
    }

    pub fn get_channel(&mut self) -> Result<Channel> {
        loop {
            match self.session.channel_session() {
                Ok(c) => return Ok(c),
                Err(e) => {
                    let e = io::Error::from(e);
                    if e.kind() == io::ErrorKind::WouldBlock {
                    } else {
                        return Err(anyhow!("{}", e));
                    }
                }
            };

            //            self.poll.lock().poll(&mut self.events, None)?;
            //            for event in &self.events {
            //                match event.token() {
            //                    Token(0) => match self.session.channel_session() {
            //                        Ok(c) => return Ok(c),
            //                        Err(e) => {
            //                            let e = io::Error::from(e);
            //                            if e.kind() == io::ErrorKind::WouldBlock {
            //                                continue;
            //                            } else {
            //                                return Err(anyhow!("{}", e));
            //                            }
            //                        }
            //                    },
            //                    _ => (),
            //                }
            //            }
        }
    }

    pub fn exec(&mut self, cmd: &str) -> Result<String> {
        let mut channel = self.get_channel()?;
        self.channel_exec(&mut channel, cmd)?;

        let bytes = self.channel_read_bytes(&mut channel)?;
        Ok(String::from_utf8(bytes)?)
    }

    pub fn get_pwd(&mut self) -> Result<PathBuf> {
        let s = self.exec("pwd")?;
        let pwd = PathBuf::from(&s.split("\n").collect::<Vec<&str>>()[0]);
        Ok(pwd)
    }

    pub fn send(&mut self, path: &str, mode: i32, size: u64) -> Result<Channel> {
        let path = Path::new(path);
        match self.session.scp_send(path, mode, size, None) {
            Ok(c) => return Ok(c),
            Err(e) => {
                let e = io::Error::from(e);
                if e.kind() == io::ErrorKind::WouldBlock {
                } else {
                    return Err(anyhow!("{}", e));
                }
            }
        };
        loop {
            self.poll.lock().poll(&mut self.events, None)?;
            for event in &self.events {
                match event.token() {
                    Token(0) => {
                        match self.session.scp_send(path, mode, size, None) {
                            Ok(c) => return Ok(c),
                            Err(e) => {
                                let e = io::Error::from(e);
                                if e.kind() == io::ErrorKind::WouldBlock {
                                    continue;
                                } else {
                                    return Err(anyhow!("{}", e));
                                }
                            }
                        }
                    }
                    _ => (),
                }
            }
        }
    }

    pub fn recv(&mut self, path: &str) -> Result<Channel> {
        match self.session.scp_recv(Path::new(path)) {
            Ok((c, _)) => return Ok(c),
            Err(e) => {
                let e = io::Error::from(e);
                if e.kind() == io::ErrorKind::WouldBlock {
                } else {
                    return Err(anyhow!("{}", e));
                }
            }
        };
        loop {
            self.poll.lock().poll(&mut self.events, None)?;
            for event in &self.events {
                match event.token() {
                    Token(0) => match self.session.scp_recv(Path::new(path)) {
                        Ok((c, _)) => return Ok(c),
                        Err(e) => {
                            let e = io::Error::from(e);
                            if e.kind() == io::ErrorKind::WouldBlock {
                                continue;
                            } else {
                                return Err(anyhow!("{}", e));
                            }
                        }
                    },
                    _ => (),
                }
            }
        }
    }

    pub fn read_file(&mut self, path: &str) -> Result<Vec<u8>> {
        let mut remote_file = self.recv(path)?;
        self.channel_read_bytes(&mut remote_file)
    }

    pub fn channel_write(
        &mut self,
        channel: &mut Channel,
        buf: &[u8],
    ) -> Result<usize> {
        loop {
            match channel.write(buf) {
                Ok(n) => return Ok(n),
                Err(e) => {
                    if e.kind() != io::ErrorKind::WouldBlock {
                        return Err(anyhow!(e));
                    }
                }
            };
        }
    }

    pub fn channel_read_bytes(&mut self, channel: &mut Channel) -> Result<Vec<u8>> {
        let mut contents = Vec::new();
        let mut buf = [0; 1500];
        loop {
            match channel.read(&mut buf) {
                Ok(0) => return Ok(contents),
                Ok(n) => contents.extend_from_slice(&buf[..n]),
                Err(e) => {
                    if e.kind() != io::ErrorKind::WouldBlock {
                        return Err(anyhow!(e));
                    }
                }
            };
            //self.poll.lock().poll(&mut self.events, None)?;
            //for event in &self.events {
            //    match event.token() {
            //        Token(0) => {
            //            if event.is_readable() {
            //                match channel.read(&mut buf) {
            //                    Ok(0) => return Ok(contents),
            //                    Ok(n) => contents.extend_from_slice(&buf[..n]),
            //                    Err(e) => {
            //                        if e.kind() != io::ErrorKind::WouldBlock {
            //                            return Err(anyhow!(e));
            //                        }
            //                    }
            //                };
            //            }
            //        }
            //        _ => (),
            //    }
            //}
        }
    }

    pub fn read_dirs(&mut self, path: &PathBuf) -> Result<Vec<PathBuf>> {
        let s = self.exec(&format!("ls -p {}", path.to_str().unwrap()))?;
        let mut paths = Vec::new();
        for part in s.split("\n") {
            let part = part.trim();
            if part.ends_with("/") {
                paths.push(path.join(part));
            }
        }
        Ok(paths)
    }

    pub fn read_dir(&mut self, path: &str) -> Result<Vec<String>> {
        let s = self.exec(&format!("ls -URp {}", path))?;
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

impl SshStream {
    pub fn new(stream: Stream, poll: Arc<Mutex<Poll>>) -> SshStream {
        let events = Events::with_capacity(1024);
        SshStream {
            stream,
            poll,
            events,
        }
    }
}

impl Write for SshStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        loop {
            match self.stream.write(buf) {
                Ok(n) => return Ok(n),
                Err(e) => {
                    if e.kind() != io::ErrorKind::WouldBlock {
                        return Err(e);
                    } else {
                        println!("first wirte would block");
                    }
                }
            };
            self.poll.lock().poll(&mut self.events, None)?;
            for event in &self.events {
                match event.token() {
                    Token(0) => {
                        match self.stream.write(buf) {
                            Ok(n) => return Ok(n),
                            Err(e) => {
                                if e.kind() != io::ErrorKind::WouldBlock {
                                    return Err(e);
                                } else {
                                    println!("write would block");
                                }
                            }
                        };
                    }
                    _ => (),
                }
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        loop {
            match self.stream.flush() {
                Ok(_) => return Ok(()),
                Err(e) => {
                    if e.kind() != io::ErrorKind::WouldBlock {
                        return Err(e);
                    } else {
                        println!("flush would block");
                    }
                }
            };
            self.poll.lock().poll(&mut self.events, None)?;
            for event in &self.events {
                match event.token() {
                    Token(0) => {
                        match self.stream.flush() {
                            Ok(_) => return Ok(()),
                            Err(e) => {
                                if e.kind() != io::ErrorKind::WouldBlock {
                                    return Err(e);
                                } else {
                                    println!("flush would block");
                                }
                            }
                        };
                    }
                    _ => (),
                }
            }
        }
    }
}

impl Read for SshStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            match self.stream.read(buf) {
                Ok(n) => return Ok(n),
                Err(e) => {
                    if e.kind() != io::ErrorKind::WouldBlock {
                        return Err(e);
                    } else {
                    }
                }
            };
            self.poll.lock().poll(&mut self.events, None)?;
            for event in &self.events {
                match event.token() {
                    Token(0) => {
                        if event.is_readable() {
                            match self.stream.read(buf) {
                                Ok(n) => return Ok(n),
                                Err(e) => {
                                    if e.kind() != io::ErrorKind::WouldBlock {
                                        return Err(e);
                                    } else {
                                    }
                                }
                            };
                        }
                    }
                    _ => (),
                }
            }
        }
    }
}
