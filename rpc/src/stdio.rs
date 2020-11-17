use crossbeam_channel::{bounded, Receiver, Sender};
use jsonrpc_lite::JsonRpc;
use serde_json::Value;
use std::{
    io::{self, stdin, stdout, BufRead, Read, Write},
    thread,
};

use crate::parse::RpcObject;

pub(crate) fn stdio_transport() -> (Sender<Value>, Receiver<Value>, IoThreads) {
    let (writer_sender, writer_receiver) = bounded(0);
    let writer = thread::spawn(move || {
        let stdout = stdout();
        let mut stdout = stdout.lock();
        writer_receiver
            .into_iter()
            .try_for_each(|it| write_msg(&mut stdout, &it))?;
        Ok(())
    });
    let (reader_sender, reader_receiver) = bounded(0);
    let reader = thread::spawn(move || {
        let stdin = stdin();
        let mut stdin = stdin.lock();
        loop {
            let msg = read_msg(&mut stdin)?;
            reader_sender.send(msg).unwrap();
        }
        Ok(())
    });
    let threads = IoThreads { reader, writer };
    (writer_sender, reader_receiver, threads)
}

fn write_msg(out: &mut dyn Write, msg: &Value) -> io::Result<()> {
    let msg = format!("{}\n", serde_json::to_string(msg)?);
    out.write_all(msg.as_bytes())?;
    out.flush()?;
    Ok(())
}

fn read_msg(inp: &mut dyn BufRead) -> io::Result<Value> {
    let mut buf = String::new();
    let s = inp.read_line(&mut buf)?;
    let value: Value = serde_json::from_str(&buf)?;
    Ok(value)
}

pub(crate) fn make_io_threads(
    reader: thread::JoinHandle<io::Result<()>>,
    writer: thread::JoinHandle<io::Result<()>>,
) -> IoThreads {
    IoThreads { reader, writer }
}

pub struct IoThreads {
    reader: thread::JoinHandle<io::Result<()>>,
    writer: thread::JoinHandle<io::Result<()>>,
}

impl IoThreads {
    pub fn join(self) -> io::Result<()> {
        match self.reader.join() {
            Ok(r) => r?,
            Err(err) => {
                println!("reader panicked!");
                panic!(err);
            }
        }
        match self.writer.join() {
            Ok(r) => r,
            Err(err) => {
                println!("reader panicked!");
                panic!(err)
            }
        }
    }
}
