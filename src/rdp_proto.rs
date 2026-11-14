//! Wire format between xtermium and the `xtermium-rdp` helper (stdin/stdout).
//! Shared by both binaries via `#[path]`. Tag byte + little-endian fields; strings/bytes are u32-length-prefixed.

use std::io::{self, Read, Write};

#[derive(Debug, PartialEq)]
pub enum ToHelper {
    Connect {
        host: String,
        port: u16,
        user: String,
        domain: String,
        password: String,
        width: u16,
        height: u16,
        /// Picture quality against speed: 0 quality, 1 balanced, 2 speed (see the helper's `config`).
        quality: u8,
    },
    /// Answer to `FromHelper::Certificate`.
    Trust(bool),
    Key {
        scancode: u16,
        pressed: bool,
    },
    Unicode(char),
    MouseMove {
        x: u16,
        y: u16,
    },
    MouseButton {
        button: u8,
        pressed: bool,
    },
    Wheel {
        vertical: bool,
        units: i16,
    },
    Close,
    /// Ask the server for a new desktop size (it may decline or round it).
    Resize {
        width: u16,
        height: u16,
    },
    /// Let go of every key and mouse button held on the desktop (the view lost the keyboard).
    ReleaseAll,
    /// The lock keys' states here, for the desktop to take on.
    Sync {
        caps: bool,
        num: bool,
        scroll: bool,
    },
    /// The clipboard here holds this text: the desktop may paste it.
    ClipText(String),
    /// The clipboard here holds these files and folders (full paths): the desktop may paste them.
    ClipFiles(Vec<String>),
    /// The clipboard here holds a picture, as a device-independent bitmap (CF_DIB: a
    /// BITMAPINFOHEADER, then the pixels): the desktop may paste it.
    ClipImage(Vec<u8>),
    /// Bring the desktop's latest copy here: its text and pictures come back as
    /// `FromHelper::ClipText` / `ClipImage`; its files go on the clipboard here by the helper
    /// itself (Windows).
    ClipFetch,
    /// The window took a frame: another may go (the helper keeps a couple in flight, and merges
    /// the changes in between instead of queueing them).
    Drawn,
}

#[derive(Debug, PartialEq)]
pub enum FromHelper {
    /// Server public key (from its TLS certificate); the helper waits for `ToHelper::Trust`.
    Certificate(Vec<u8>),
    Size {
        width: u16,
        height: u16,
    },
    /// Changed areas since the last frame, back to back: `[x][y][width][height: u16][RGBA pixels]`.
    /// xtermium passes it to the UI as it is.
    Frame(Vec<u8>),
    Closed(String),
    /// The user signed out or disconnected from inside the session: the tab closes.
    Ended,
    /// What the session is doing while it isn't showing the desktop (reconnecting, say).
    Status(String),
    /// Something was copied on the desktop: what it holds. Nothing comes over until `ToHelper::ClipFetch`.
    ClipOffer {
        text: bool,
        files: bool,
        image: bool,
    },
    /// The desktop's copied text, after `ToHelper::ClipFetch`.
    ClipText(String),
    /// The desktop's copied picture (CF_DIB, as `ToHelper::ClipImage`), after `ToHelper::ClipFetch`.
    ClipImage(Vec<u8>),
    /// The desktop's pointer: the system arrow, none, or a picture (RGBA, not premultiplied)
    /// with the point that clicks at (`hot_x`, `hot_y`).
    PointerDefault,
    PointerHidden,
    PointerBitmap {
        width: u16,
        height: u16,
        hot_x: u16,
        hot_y: u16,
        rgba: Vec<u8>,
    },
}

fn put_bytes(out: &mut Vec<u8>, b: &[u8]) {
    out.extend_from_slice(&(b.len() as u32).to_le_bytes());
    out.extend_from_slice(b);
}

fn u8_(r: &mut impl Read) -> io::Result<u8> {
    let mut b = [0u8; 1];
    r.read_exact(&mut b)?;
    Ok(b[0])
}
fn u16_(r: &mut impl Read) -> io::Result<u16> {
    let mut b = [0u8; 2];
    r.read_exact(&mut b)?;
    Ok(u16::from_le_bytes(b))
}
fn u32_(r: &mut impl Read) -> io::Result<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}
fn bytes(r: &mut impl Read) -> io::Result<Vec<u8>> {
    let len = u32_(r)? as usize;
    // 64 MiB covers an 8K RGBA frame; anything larger is a corrupt stream.
    if len > 64 << 20 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "frame too large"));
    }
    let mut v = vec![0u8; len];
    r.read_exact(&mut v)?;
    Ok(v)
}
fn string(r: &mut impl Read) -> io::Result<String> {
    String::from_utf8(bytes(r)?).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}
fn bad() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "unknown message")
}

impl ToHelper {
    pub fn write(&self, w: &mut impl Write) -> io::Result<()> {
        let mut o = vec![];
        match self {
            Self::Connect { host, port, user, domain, password, width, height, quality } => {
                o.push(1);
                for s in [host, user, domain, password] {
                    put_bytes(&mut o, s.as_bytes());
                }
                for n in [port, width, height] {
                    o.extend_from_slice(&n.to_le_bytes());
                }
                o.push(*quality);
            }
            Self::Trust(t) => o.extend_from_slice(&[2, *t as u8]),
            Self::Key { scancode, pressed } => {
                o.push(3);
                o.extend_from_slice(&scancode.to_le_bytes());
                o.push(*pressed as u8);
            }
            Self::Unicode(c) => {
                o.push(4);
                o.extend_from_slice(&(*c as u32).to_le_bytes());
            }
            Self::MouseMove { x, y } => {
                o.push(5);
                o.extend_from_slice(&x.to_le_bytes());
                o.extend_from_slice(&y.to_le_bytes());
            }
            Self::MouseButton { button, pressed } => o.extend_from_slice(&[6, *button, *pressed as u8]),
            Self::Wheel { vertical, units } => {
                o.extend_from_slice(&[7, *vertical as u8]);
                o.extend_from_slice(&units.to_le_bytes());
            }
            Self::Close => o.push(8),
            Self::Resize { width, height } => {
                o.push(9);
                o.extend_from_slice(&width.to_le_bytes());
                o.extend_from_slice(&height.to_le_bytes());
            }
            Self::ReleaseAll => o.push(10),
            Self::Sync { caps, num, scroll } => o.extend_from_slice(&[11, *caps as u8 | (*num as u8) << 1 | (*scroll as u8) << 2]),
            Self::ClipText(t) => {
                o.push(12);
                put_bytes(&mut o, t.as_bytes());
            }
            Self::ClipFiles(paths) => {
                o.push(13);
                o.extend_from_slice(&(paths.len() as u32).to_le_bytes());
                for p in paths {
                    put_bytes(&mut o, p.as_bytes());
                }
            }
            Self::ClipFetch => o.push(14),
            Self::Drawn => o.push(15),
            Self::ClipImage(dib) => {
                o.push(16);
                put_bytes(&mut o, dib);
            }
        }
        w.write_all(&o)?;
        w.flush()
    }

    pub fn read(r: &mut impl Read) -> io::Result<Self> {
        Ok(match u8_(r)? {
            1 => {
                let (host, user, domain, password) = (string(r)?, string(r)?, string(r)?, string(r)?);
                let (port, width, height) = (u16_(r)?, u16_(r)?, u16_(r)?);
                Self::Connect { host, user, domain, password, port, width, height, quality: u8_(r)? }
            }
            2 => Self::Trust(u8_(r)? != 0),
            3 => Self::Key { scancode: u16_(r)?, pressed: u8_(r)? != 0 },
            4 => Self::Unicode(char::from_u32(u32_(r)?).ok_or_else(bad)?),
            5 => Self::MouseMove { x: u16_(r)?, y: u16_(r)? },
            6 => Self::MouseButton { button: u8_(r)?, pressed: u8_(r)? != 0 },
            7 => Self::Wheel { vertical: u8_(r)? != 0, units: u16_(r)? as i16 },
            8 => Self::Close,
            9 => Self::Resize { width: u16_(r)?, height: u16_(r)? },
            10 => Self::ReleaseAll,
            11 => {
                let b = u8_(r)?;
                Self::Sync { caps: b & 1 != 0, num: b & 2 != 0, scroll: b & 4 != 0 }
            }
            12 => Self::ClipText(string(r)?),
            13 => {
                let n = u32_(r)?;
                // A clipboard of more files than this is a corrupt stream.
                if n > 100_000 {
                    return Err(bad());
                }
                Self::ClipFiles((0..n).map(|_| string(r)).collect::<io::Result<_>>()?)
            }
            14 => Self::ClipFetch,
            15 => Self::Drawn,
            16 => Self::ClipImage(bytes(r)?),
            _ => return Err(bad()),
        })
    }
}

impl FromHelper {
    pub fn write(&self, w: &mut impl Write) -> io::Result<()> {
        let mut o = vec![];
        match self {
            Self::Certificate(k) => {
                o.push(1);
                put_bytes(&mut o, k);
            }
            Self::Size { width, height } => {
                o.push(2);
                o.extend_from_slice(&width.to_le_bytes());
                o.extend_from_slice(&height.to_le_bytes());
            }
            Self::Frame(body) => {
                o.push(3);
                put_bytes(&mut o, body);
            }
            Self::Closed(msg) => {
                o.push(4);
                put_bytes(&mut o, msg.as_bytes());
            }
            Self::Ended => o.push(5),
            Self::Status(msg) => {
                o.push(6);
                put_bytes(&mut o, msg.as_bytes());
            }
            Self::ClipOffer { text, files, image } => o.extend_from_slice(&[7, *text as u8 | (*files as u8) << 1 | (*image as u8) << 2]),
            Self::ClipText(t) => {
                o.push(8);
                put_bytes(&mut o, t.as_bytes());
            }
            Self::PointerDefault => o.push(9),
            Self::PointerHidden => o.push(10),
            Self::PointerBitmap { width, height, hot_x, hot_y, rgba } => {
                o.push(11);
                for n in [width, height, hot_x, hot_y] {
                    o.extend_from_slice(&n.to_le_bytes());
                }
                put_bytes(&mut o, rgba);
            }
            Self::ClipImage(dib) => {
                o.push(12);
                put_bytes(&mut o, dib);
            }
        }
        w.write_all(&o)?;
        w.flush()
    }

    pub fn read(r: &mut impl Read) -> io::Result<Self> {
        Ok(match u8_(r)? {
            1 => Self::Certificate(bytes(r)?),
            2 => Self::Size { width: u16_(r)?, height: u16_(r)? },
            3 => Self::Frame(bytes(r)?),
            4 => Self::Closed(string(r)?),
            5 => Self::Ended,
            6 => Self::Status(string(r)?),
            7 => {
                let b = u8_(r)?;
                Self::ClipOffer { text: b & 1 != 0, files: b & 2 != 0, image: b & 4 != 0 }
            }
            8 => Self::ClipText(string(r)?),
            9 => Self::PointerDefault,
            10 => Self::PointerHidden,
            11 => {
                let (width, height, hot_x, hot_y) = (u16_(r)?, u16_(r)?, u16_(r)?, u16_(r)?);
                Self::PointerBitmap { width, height, hot_x, hot_y, rgba: bytes(r)? }
            }
            12 => Self::ClipImage(bytes(r)?),
            _ => return Err(bad()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let to = [
            ToHelper::Connect {
                host: "h".into(),
                port: 3389,
                user: "u".into(),
                domain: "".into(),
                password: "p\tw".into(),
                width: 1920,
                height: 1080,
                quality: 1,
            },
            ToHelper::Trust(true),
            ToHelper::Key { scancode: 0xE048, pressed: true },
            ToHelper::Unicode('é'),
            ToHelper::MouseMove { x: 5, y: 6 },
            ToHelper::MouseButton { button: 2, pressed: false },
            ToHelper::Wheel { vertical: true, units: -120 },
            ToHelper::Close,
            ToHelper::Resize { width: 2560, height: 1440 },
            ToHelper::ReleaseAll,
            ToHelper::Sync { caps: true, num: false, scroll: true },
            ToHelper::Sync { caps: false, num: true, scroll: false },
            ToHelper::ClipText("a\r\nb".into()),
            ToHelper::ClipFiles(vec!["C:\\a b\\c.txt".into(), "/srv/x/é".into()]),
            ToHelper::ClipFiles(vec![]),
            ToHelper::ClipFetch,
            ToHelper::Drawn,
            ToHelper::ClipImage(vec![40, 0, 0, 0, 7]),
        ];
        let mut buf = vec![];
        for m in &to {
            m.write(&mut buf).unwrap();
        }
        let mut r = &buf[..];
        for m in &to {
            assert_eq!(&ToHelper::read(&mut r).unwrap(), m);
        }

        let from = [
            FromHelper::Certificate(vec![1, 2, 3]),
            FromHelper::Size { width: 800, height: 600 },
            FromHelper::Frame(vec![1, 0, 2, 0, 1, 0, 1, 0, 9, 9, 9, 255]),
            FromHelper::Closed("bye".into()),
            FromHelper::Ended,
            FromHelper::Status("reconnecting…".into()),
            FromHelper::ClipOffer { text: true, files: false, image: false },
            FromHelper::ClipOffer { text: false, files: true, image: true },
            FromHelper::ClipText("x\ty".into()),
            FromHelper::PointerDefault,
            FromHelper::PointerHidden,
            FromHelper::PointerBitmap { width: 1, height: 2, hot_x: 0, hot_y: 1, rgba: vec![1, 2, 3, 4, 5, 6, 7, 8] },
            FromHelper::ClipImage(vec![9, 8, 7]),
        ];
        let mut buf = vec![];
        for m in &from {
            m.write(&mut buf).unwrap();
        }
        let mut r = &buf[..];
        for m in &from {
            assert_eq!(&FromHelper::read(&mut r).unwrap(), m);
        }
    }
}
