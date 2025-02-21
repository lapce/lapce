use std::{
    io::{self, BufRead, Write},
    thread,
};

use anyhow::Result;
use crossbeam_channel::{Receiver, Sender};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

use crate::{RpcError, RpcMessage, RpcObject};

pub fn stdio_transport<W, R, Req1, Notif1, Resp1, Req2, Notif2, Resp2>(
    mut writer: W,
    writer_receiver: Receiver<RpcMessage<Req2, Notif2, Resp2>>,
    mut reader: R,
    reader_sender: Sender<RpcMessage<Req1, Notif1, Resp1>>,
) where
    W: 'static + Write + Send,
    R: 'static + BufRead + Send,
    Req1: 'static + Serialize + DeserializeOwned + Send + Sync,
    Notif1: 'static + Serialize + DeserializeOwned + Send + Sync,
    Resp1: 'static + Serialize + DeserializeOwned + Send + Sync,
    Req2: 'static + Serialize + DeserializeOwned + Send + Sync,
    Notif2: 'static + Serialize + DeserializeOwned + Send + Sync,
    Resp2: 'static + Serialize + DeserializeOwned + Send + Sync,
{
    thread::spawn(move || {
        for value in writer_receiver {
            if write_msg(&mut writer, value).is_err() {
                return;
            };
        }
    });
    thread::spawn(move || -> Result<()> {
        loop {
            if let Some(msg) = read_msg(&mut reader)? {
                reader_sender.send(msg)?;
            }
        }
    });
}

pub fn write_msg<W, Req, Notif, Resp>(
    out: &mut W,
    msg: RpcMessage<Req, Notif, Resp>,
) -> io::Result<()>
where
    W: Write,
    Req: Serialize,
    Notif: Serialize,
    Resp: Serialize,
{
    let value = match msg {
        RpcMessage::Request(id, req) => {
            let mut msg = serde_json::to_value(&req)?;
            msg.as_object_mut()
                .ok_or(io::ErrorKind::NotFound)?
                .insert("id".into(), id.into());
            msg
        }
        RpcMessage::Response(id, resp) => {
            json!({
                "id": id,
                "result": resp,
            })
        }
        RpcMessage::Notification(n) => serde_json::to_value(n)?,
        RpcMessage::Error(id, err) => {
            json!({
                "id": id,
                "error": err,
            })
        }
    };
    let msg = format!("{}\n", serde_json::to_string(&value)?);
    out.write_all(msg.as_bytes())?;
    out.flush()?;
    Ok(())
}

pub fn read_msg<R, Req, Notif, Resp>(
    inp: &mut R,
) -> io::Result<Option<RpcMessage<Req, Notif, Resp>>>
where
    R: BufRead,
    Req: DeserializeOwned,
    Notif: DeserializeOwned,
    Resp: DeserializeOwned,
{
    let mut buf = String::new();
    let _ = inp.read_line(&mut buf)?;
    let value: Value = serde_json::from_str(&buf)?;

    match parse_value(value) {
        Ok(msg) => Ok(Some(msg)),
        Err(e) => {
            tracing::error!("receive rpc from stdio error: {e:#}");
            Ok(None)
        }
    }
}

fn parse_value<Req, Notif, Resp>(
    value: Value,
) -> io::Result<RpcMessage<Req, Notif, Resp>>
where
    Req: DeserializeOwned,
    Notif: DeserializeOwned,
    Resp: DeserializeOwned,
{
    let object = RpcObject(value);
    let is_response = object.is_response();
    let msg = if is_response {
        let id = object.get_id().ok_or(io::ErrorKind::NotFound)?;
        let resp = object
            .into_response()
            .map_err(|_| io::ErrorKind::NotFound)?;
        match resp {
            Ok(value) => {
                let resp: Resp = serde_json::from_value(value)?;
                RpcMessage::Response(id, resp)
            }
            Err(value) => {
                let err: RpcError = serde_json::from_value(value)?;
                RpcMessage::Error(id, err)
            }
        }
    } else {
        match object.get_id() {
            Some(id) => {
                let req: Req = serde_json::from_value(object.0)?;
                RpcMessage::Request(id, req)
            }
            None => {
                let notif: Notif = serde_json::from_value(object.0)?;
                RpcMessage::Notification(notif)
            }
        }
    };
    Ok(msg)
}
