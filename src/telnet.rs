//! Telnet sessions: a TCP connection with option negotiation (RFC 854). The server learns the
//! window size (NAWS) and terminal type, and may echo and suppress go-ahead; everything else is
//! refused. Data passes through as it is, apart from IAC escaping and CR.

use crate::ssh::{Ctl, Lost};
use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::time::Duration;
use tokio::sync::mpsc::UnboundedReceiver;

const IAC: u8 = 255;
const DONT: u8 = 254;
const DO: u8 = 253;
const WONT: u8 = 252;
const WILL: u8 = 251;
const SB: u8 = 250;
const SE: u8 = 240;

const ECHO: u8 = 1;
const SGA: u8 = 3;
const TTYPE: u8 = 24;
const NAWS: u8 = 31;

#[derive(Clone, Copy, PartialEq)]
enum State {
    Data,
    Iac,
    /// After IAC WILL / WONT / DO / DONT: the option comes next.
    Verb(u8),
    /// Inside IAC SB … IAC SE.
    Sub,
    SubIac,
}

/// The negotiating side of a connection: what the server sends goes through `input`.
pub struct Telnet {
    state: State,
    sub: Vec<u8>,
    /// The server asked for the window size: resizes are sent.
    naws: bool,
    size: (u16, u16),
    term: String,
    /// Replies already sent, so a server repeating itself doesn't start a loop.
    said: Vec<(u8, u8)>,
}

impl Telnet {
    pub fn new(term: &str, size: (u16, u16)) -> Self {
        Self { state: State::Data, sub: vec![], naws: false, size, term: term.to_uppercase(), said: vec![] }
    }

    /// Splits what the server sent into terminal output (`out`) and replies to send (`reply`).
    pub fn input(&mut self, bytes: &[u8], out: &mut Vec<u8>, reply: &mut Vec<u8>) {
        for &b in bytes {
            self.state = match (self.state, b) {
                (State::Data, IAC) => State::Iac,
                (State::Data, b) => {
                    out.push(b);
                    State::Data
                }
                (State::Iac, IAC) => {
                    out.push(IAC);
                    State::Data
                }
                (State::Iac, v @ (WILL | WONT | DO | DONT)) => State::Verb(v),
                (State::Iac, SB) => {
                    self.sub.clear();
                    State::Sub
                }
                (State::Iac, _) => State::Data, // NOP, GA and the like
                (State::Verb(v), opt) => {
                    self.negotiate(v, opt, reply);
                    State::Data
                }
                (State::Sub, IAC) => State::SubIac,
                (State::Sub, b) => {
                    self.keep(b);
                    State::Sub
                }
                (State::SubIac, SE) => {
                    self.subnegotiate(reply);
                    State::Data
                }
                (State::SubIac, b) => {
                    self.keep(b);
                    State::Sub
                }
            };
        }
    }

    /// A byte of a subnegotiation. Only short ones are answered (TTYPE SEND), so a server that
    /// never ends one can't make it grow without end.
    fn keep(&mut self, b: u8) {
        if self.sub.len() < 64 {
            self.sub.push(b);
        }
    }

    fn say(&mut self, verb: u8, opt: u8, reply: &mut Vec<u8>) {
        if !self.said.contains(&(verb, opt)) {
            self.said.push((verb, opt));
            reply.extend_from_slice(&[IAC, verb, opt]);
        }
    }

    fn negotiate(&mut self, verb: u8, opt: u8, reply: &mut Vec<u8>) {
        match (verb, opt) {
            (DO, NAWS) => {
                self.say(WILL, NAWS, reply);
                self.naws = true;
                reply.extend(self.window());
            }
            (DO, TTYPE | SGA) => self.say(WILL, opt, reply),
            (DO, _) => self.say(WONT, opt, reply),
            (WILL, ECHO | SGA) => self.say(DO, opt, reply),
            (WILL, _) => self.say(DONT, opt, reply),
            (DONT, NAWS) => self.naws = false,
            _ => {} // WONT / DONT: nothing to answer
        }
    }

    fn subnegotiate(&mut self, reply: &mut Vec<u8>) {
        // TTYPE SEND: answer TTYPE IS <name>.
        if self.sub == [TTYPE, 1] {
            reply.extend_from_slice(&[IAC, SB, TTYPE, 0]);
            reply.extend_from_slice(self.term.as_bytes());
            reply.extend_from_slice(&[IAC, SE]);
        }
    }

    /// IAC SB NAWS <width> <height> IAC SE, with 255 bytes doubled.
    fn window(&self) -> Vec<u8> {
        let mut v = vec![IAC, SB, NAWS];
        for b in [self.size.0.to_be_bytes(), self.size.1.to_be_bytes()].concat() {
            v.push(b);
            if b == IAC {
                v.push(IAC);
            }
        }
        v.extend_from_slice(&[IAC, SE]);
        v
    }

    /// The window changed: what to tell the server, if it asked to know.
    pub fn resize(&mut self, cols: u16, rows: u16) -> Option<Vec<u8>> {
        self.size = (cols, rows);
        self.naws.then(|| self.window())
    }
}

/// Typed bytes as the server expects them: IAC doubled, CR followed by NUL.
pub fn encode(data: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(data.len() + 4);
    for &b in data {
        v.push(b);
        match b {
            IAC => v.push(IAC),
            b'\r' => v.push(0),
            _ => {}
        }
    }
    v
}

/// Runs a session until the server closes it (Ok: the tab closes), the connection fails
/// (`ssh::Lost`: the tab may reconnect) or the tab closes.
pub fn run(
    host: &str,
    port: u16,
    term: &str,
    size: (u16, u16),
    mut rx: UnboundedReceiver<Ctl>,
    mut feed: impl FnMut(&[u8]),
) -> anyhow::Result<()> {
    // Unanswered, the connect takes up to 15 s: say what's going on meanwhile.
    feed(format!("\x1b[2mConnecting to {host}:{port}…\x1b[0m\r\n").as_bytes());
    let addr = (host, port)
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| anyhow::anyhow!("{host} has no address"))?;
    let sock = TcpStream::connect_timeout(&addr, Duration::from_secs(15)).map_err(|e| match e.kind() {
        std::io::ErrorKind::TimedOut => anyhow::anyhow!("{host}:{port} didn't answer within 15 s"),
        _ => anyhow::anyhow!("connecting to {host}:{port}: {e}"),
    })?;
    sock.set_nodelay(true)?;
    let mut writer = sock.try_clone()?;
    let mut reader = sock.try_clone()?;
    let proto = std::sync::Arc::new(std::sync::Mutex::new(Telnet::new(term, size)));
    let p2 = proto.clone();
    std::thread::spawn(move || {
        while let Some(c) = rx.blocking_recv() {
            let bytes = match c {
                Ctl::Input(b) => encode(&b),
                Ctl::Resize(cols, rows) => match p2.lock().unwrap().resize(cols, rows) {
                    Some(b) => b,
                    None => continue,
                },
                Ctl::Sftp(_) => continue,
                Ctl::Close => break,
            };
            if writer.write_all(&bytes).is_err() {
                break;
            }
        }
        // The tab closed: the reader below sees the end.
        let _ = sock.shutdown(Shutdown::Both);
    });
    let mut buf = vec![0u8; 64 * 1024];
    let (mut out, mut reply) = (vec![], vec![]);
    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) => return Ok(()),
            Ok(n) => n,
            Err(_) => return Err(Lost.into()),
        };
        out.clear();
        reply.clear();
        proto.lock().unwrap().input(&buf[..n], &mut out, &mut reply);
        if !reply.is_empty() && reader.write_all(&reply).is_err() {
            return Err(Lost.into());
        }
        if !out.is_empty() {
            feed(&out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negotiates() {
        let mut t = Telnet::new("xterm-256color", (80, 24));
        let (mut out, mut reply) = (vec![], vec![]);
        // Login banner around: DO NAWS, DO TTYPE, WILL ECHO, DO LINEMODE(34), WILL STATUS(5).
        let server = [
            b"hi".as_slice(),
            &[IAC, DO, NAWS, IAC, DO, TTYPE, IAC, WILL, ECHO, IAC, DO, 34, IAC, WILL, 5],
            &[IAC, SB, TTYPE, 1, IAC, SE],
            b" a\xff\xffb",
            &[IAC, DO, NAWS],
        ]
        .concat();
        // Arrives in pieces, split inside commands.
        for chunk in server.chunks(3) {
            t.input(chunk, &mut out, &mut reply);
        }
        assert_eq!(out, b"hi a\xffb");
        let expect = [
            &[IAC, WILL, NAWS][..],
            &[IAC, SB, NAWS, 0, 80, 0, 24, IAC, SE],
            &[IAC, WILL, TTYPE],
            &[IAC, DO, ECHO],
            &[IAC, WONT, 34],
            &[IAC, DONT, 5],
            &[IAC, SB, TTYPE, 0],
            b"XTERM-256COLOR",
            &[IAC, SE],
            // The repeated DO NAWS: no second WILL, the size again.
            &[IAC, SB, NAWS, 0, 80, 0, 24, IAC, SE],
        ]
        .concat();
        assert_eq!(reply, expect);
        // 255 columns: the byte is doubled.
        assert_eq!(t.resize(255, 50).unwrap(), [IAC, SB, NAWS, 0, 255, 255, 0, 50, IAC, SE]);
        assert_eq!(encode(b"ls\r\xff"), b"ls\r\0\xff\xff");
    }

    /// Against a server on a local port: it asks for the window size and greets, gets the size
    /// and a typed line, then hangs up, which ends the session cleanly (the tab closes).
    #[test]
    fn talks_to_a_server() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            s.write_all(&[IAC, DO, NAWS]).unwrap();
            s.write_all(b"login: ").unwrap();
            let mut got = vec![];
            let mut buf = [0u8; 256];
            while !got.ends_with(b"\r\0") {
                let n = s.read(&mut buf).unwrap();
                got.extend_from_slice(&buf[..n]);
            }
            got // dropping `s` hangs up
        });
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let mut shown = vec![];
        let tx2 = tx.clone();
        let r = run("127.0.0.1", port, "xterm", (100, 30), rx, |b| {
            shown.extend_from_slice(b);
            if shown.ends_with(b"login: ") {
                let _ = tx2.send(Ctl::Input(b"root\r".to_vec()));
            }
        });
        assert!(r.is_ok(), "{r:?}");
        assert!(shown.ends_with(b"\x1b[0m\r\nlogin: "), "{:?}", String::from_utf8_lossy(&shown));
        let got = server.join().unwrap();
        assert_eq!(got, [&[IAC, WILL, NAWS, IAC, SB, NAWS, 0, 100, 0, 30, IAC, SE][..], b"root\r\0"].concat());
        drop(tx);
    }
}
