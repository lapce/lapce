use anyhow::Result;
use crossbeam_channel::{Receiver, Sender};
use serde::{de::DeserializeOwned, Deserialize, Serialize, Serializer};
use serde_json::Value;
use std::{
    io::{self, BufRead, Write},
    thread,
};

pub fn stdio_transport<W, R, S, D>(
    mut writer: W,
    writer_receiver: Receiver<S>,
    mut reader: R,
    reader_sender: Sender<D>,
) where
    W: 'static + Write + Send,
    R: 'static + BufRead + Send,
    S: 'static + Serialize + Send + Sync,
    D: 'static + DeserializeOwned + Send + Sync,
{
    thread::spawn(move || {
        for value in writer_receiver {
            if write_msg(&mut writer, &value).is_err() {
                return;
            };
        }
    });
    thread::spawn(move || -> Result<()> {
        loop {
            let msg = read_msg(&mut reader)?;
            reader_sender.send(msg)?;
        }
    });
}

fn write_msg<W, S>(out: &mut W, msg: S) -> io::Result<()>
where
    W: Write,
    S: Serialize,
{
    let msg = format!("{}\n", serde_json::to_string(&msg)?);
    out.write_all(msg.as_bytes())?;
    out.flush()?;
    Ok(())
}

fn read_msg<R, D>(inp: &mut R) -> io::Result<D>
where
    R: BufRead,
    D: DeserializeOwned,
{
    let mut buf = String::new();
    let _s = inp.read_line(&mut buf)?;
    let value: D = serde_json::from_str(&buf)?;
    Ok(value)
}
