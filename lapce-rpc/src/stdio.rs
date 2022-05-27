use anyhow::Result;
use crossbeam_channel::{Receiver, Sender};
use serde_json::Value;
use std::{
    io::{self, BufRead, Write},
    thread,
};

pub fn stdio_transport<W, R>(
    mut writer: W,
    writer_receiver: Receiver<Value>,
    mut reader: R,
    reader_sender: Sender<Value>,
) where
    W: 'static + Write + Send,
    R: 'static + BufRead + Send,
{
    thread::spawn(move || -> Result<()> {
        writer_receiver
            .into_iter()
            .try_for_each(|it| write_msg(&mut writer, &it))?;
        Ok(())
    });
    thread::spawn(move || -> Result<()> {
        loop {
            let msg = read_msg(&mut reader)?;
            reader_sender.send(msg)?;
        }
    });
}

fn write_msg<W>(out: &mut W, msg: &Value) -> io::Result<()>
where
    W: Write,
{
    let msg = format!("{}\n", serde_json::to_string(msg)?);
    out.write_all(msg.as_bytes())?;
    out.flush()?;
    Ok(())
}

fn read_msg<R>(inp: &mut R) -> io::Result<Value>
where
    R: BufRead,
{
    let mut buf = String::new();
    let _s = inp.read_line(&mut buf)?;
    let value: Value = serde_json::from_str(&buf)?;
    Ok(value)
}
