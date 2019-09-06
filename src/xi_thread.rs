use std::io::{self, BufRead, ErrorKind, Read, Write};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;
use std::time::Duration;

// Needed for semaphore, currently disabled
//use winapi::um::synchapi::{CreateSemaphoreW, ReleaseSemaphore};
//use winapi::shared::ntdef::HANDLE;

use serde_json::{self, Value};

use xi_core_lib::XiCore;
use xi_rpc::RpcLoop;

pub struct XiPeer {
    tx: Sender<String>,
}

impl XiPeer {
    pub fn send(&self, s: String) {
        let _ = self.tx.send(s);
    }

    pub fn send_json(&self, v: &Value) {
        self.send(serde_json::to_string(v).unwrap());
    }
}

pub fn start_xi_thread() -> (XiPeer, Receiver<Value>) {
    let (to_core_tx, to_core_rx) = channel();
    let to_core_rx = ChanReader(to_core_rx);
    let (from_core_tx, from_core_rx) = channel();
    let from_core_tx = ChanWriter {
        sender: from_core_tx,
    };
    let mut state = XiCore::new();
    let mut rpc_looper = RpcLoop::new(from_core_tx);
    thread::spawn(move || {
        let r = rpc_looper.mainloop(|| to_core_rx, &mut state);
        println!("xi thread stopped {:?}", r);
    });
    let peer = XiPeer { tx: to_core_tx };
    (peer, from_core_rx)
}

struct ChanReader(Receiver<String>);

impl Read for ChanReader {
    fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
        unreachable!("didn't expect xi-rpc to call read");
    }
}

// Note: we don't properly implement BufRead, only the stylized call patterns
// used by xi-rpc.
impl BufRead for ChanReader {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        unreachable!("didn't expect xi-rpc to call fill_buf");
    }

    fn consume(&mut self, _amt: usize) {
        unreachable!("didn't expect xi-rpc to call consume");
    }

    fn read_line(&mut self, buf: &mut String) -> io::Result<usize> {
        match self.0.recv() {
            Ok(s) => {
                buf.push_str(&s);
                Ok(s.len())
            }
            Err(_) => Ok(0),
        }
    }
}

struct ChanWriter {
    sender: Sender<Value>,
}

impl Write for ChanWriter {
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
        unreachable!("didn't expect xi-rpc to call write");
    }

    fn flush(&mut self) -> io::Result<()> {
        unreachable!("didn't expect xi-rpc to call flush");
    }

    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        let json = serde_json::from_slice::<Value>(buf).unwrap();
        //thread::sleep(Duration::from_secs(1));
        self.sender
            .send(json)
            .map_err(|_| io::Error::new(ErrorKind::BrokenPipe, "rpc rx thread lost"))
    }
}
