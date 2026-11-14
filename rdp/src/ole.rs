//! Files copied on the desktop, on this computer's clipboard (Windows). They go there as an OLE
//! data object holding virtual files (FileGroupDescriptorW and FileContents, as a mail program's
//! attachments do), so pasting in Explorer copies them with its own progress window and each
//! file's bytes come from the desktop only as Explorer reads them.
//!
//! The object lives on a thread of its own (OLE wants a single-threaded apartment that pumps
//! messages); Explorer's calls arrive there, and a read waits for the session to bring the bytes.

use crate::clip::{Read, RemoteFile};
use std::ffi::c_void;
use std::mem::ManuallyDrop;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;
use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::DataExchange::RegisterClipboardFormatW;
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::System::Ole::{DROPEFFECT_COPY, OleInitialize, OleSetClipboard};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Shell::{
    FD_ATTRIBUTES, FD_FILESIZE, FD_PROGRESSUI, FD_WRITESTIME, FILEDESCRIPTORW, SHCreateStdEnumFmtEtc,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, MSG, PM_NOREMOVE, PeekMessageW, PostThreadMessageW, TranslateMessage, WM_APP,
};
use windows::core::{BOOL, Error, HRESULT, Ref, Result, implement, w};

/// One read asks the desktop for this much at most.
const CHUNK: u32 = 1 << 20;
/// A read the desktop hasn't answered in this long fails (the paste stops with an error).
const WAIT: Duration = Duration::from_secs(60);
/// A file's size before it is known.
const UNKNOWN: u64 = u64::MAX;
/// FILE_ATTRIBUTE_DIRECTORY and FILE_ATTRIBUTE_NORMAL (the file system's feature, for two numbers).
const DIRECTORY: u32 = 0x10;
const NORMAL: u32 = 0x80;

/// The next file list to put on the clipboard, with its number (`clip::Read::list`).
type Next = Arc<Mutex<Option<(u32, Vec<RemoteFile>)>>>;

/// The clipboard thread: `offer` puts a desktop's files on the clipboard.
#[derive(Clone, Debug)]
pub struct Ole {
    thread: u32,
    next: Next,
}

impl Ole {
    /// Starts the thread; reads of the files it offers go to `reads`. None if OLE won't start.
    pub fn start(reads: UnboundedSender<Read>) -> Option<Self> {
        let next = Next::default();
        let (tx, rx) = mpsc::channel();
        let slot = next.clone();
        std::thread::spawn(move || {
            // SAFETY: this thread's own apartment and message queue; the loop below pumps it.
            unsafe {
                if OleInitialize(None).is_err() {
                    return;
                }
                let mut msg = MSG::default();
                // Makes the queue, so `offer`'s posts aren't lost.
                let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
                let _ = tx.send(GetCurrentThreadId());
                let formats = Formats::get();
                while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
                    if msg.message == WM_APP && msg.hwnd.is_invalid() {
                        if let Some((list, files)) = slot.lock().unwrap().take() {
                            let obj: IDataObject = Files { list, files, formats, reads: reads.clone() }.into();
                            let _ = OleSetClipboard(&obj);
                        }
                        continue;
                    }
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        });
        Some(Self { thread: rx.recv().ok()?, next })
    }

    /// Puts the desktop's file list `list` on the clipboard here.
    pub fn offer(&self, list: u32, files: Vec<RemoteFile>) {
        *self.next.lock().unwrap() = Some((list, files));
        // SAFETY: posting to a thread that has a message queue (made before `start` returned).
        unsafe {
            let _ = PostThreadMessageW(self.thread, WM_APP, WPARAM(0), LPARAM(0));
        }
    }
}

/// The registered clipboard formats a virtual file paste uses.
#[derive(Clone, Copy)]
struct Formats {
    descriptor: u16,
    contents: u16,
    effect: u16,
}

impl Formats {
    fn get() -> Self {
        // SAFETY: registering format names; the same name always gives the same number.
        unsafe {
            Self {
                descriptor: RegisterClipboardFormatW(w!("FileGroupDescriptorW")) as u16,
                contents: RegisterClipboardFormatW(w!("FileContents")) as u16,
                effect: RegisterClipboardFormatW(w!("Preferred DropEffect")) as u16,
            }
        }
    }

    fn etc(format: u16, tymed: TYMED, lindex: i32) -> FORMATETC {
        FORMATETC { cfFormat: format, ptd: std::ptr::null_mut(), dwAspect: DVASPECT_CONTENT.0, lindex, tymed: tymed.0 as u32 }
    }
}

#[implement(IDataObject)]
struct Files {
    list: u32,
    files: Vec<RemoteFile>,
    formats: Formats,
    reads: UnboundedSender<Read>,
}

impl Files_Impl {
    /// Which of the formats `f` asks for, if it's one of them in a medium offered.
    fn which(&self, f: *const FORMATETC) -> Option<(u16, i32)> {
        // SAFETY: COM hands a valid FORMATETC or null.
        let f = unsafe { f.as_ref()? };
        let fm = self.formats;
        let medium = if f.cfFormat == fm.contents { TYMED_ISTREAM } else { TYMED_HGLOBAL };
        ([fm.descriptor, fm.contents, fm.effect].contains(&f.cfFormat) && f.tymed & medium.0 as u32 != 0).then_some((f.cfFormat, f.lindex))
    }
}

/// An HGLOBAL medium holding `bytes`.
fn hglobal(bytes: &[u8]) -> Result<STGMEDIUM> {
    // SAFETY: a fresh block of the right size, locked while it's written.
    unsafe {
        let h = GlobalAlloc(GMEM_MOVEABLE, bytes.len())?;
        let p = GlobalLock(h);
        if p.is_null() {
            return Err(Error::from_thread());
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), p.cast::<u8>(), bytes.len());
        let _ = GlobalUnlock(h);
        Ok(STGMEDIUM { tymed: TYMED_HGLOBAL.0 as u32, u: STGMEDIUM_0 { hGlobal: h }, pUnkForRelease: ManuallyDrop::new(None) })
    }
}

/// FILEGROUPDESCRIPTORW: the count, then one FILEDESCRIPTORW per file.
fn descriptors(files: &[RemoteFile]) -> Vec<u8> {
    let mut out = (files.len() as u32).to_le_bytes().to_vec();
    for f in files {
        let mut d = FILEDESCRIPTORW {
            dwFlags: (FD_ATTRIBUTES.0 | FD_PROGRESSUI.0) as u32,
            dwFileAttributes: if f.dir { DIRECTORY } else { NORMAL },
            ..Default::default()
        };
        if let Some(s) = f.size.filter(|_| !f.dir) {
            d.dwFlags |= FD_FILESIZE.0 as u32;
            (d.nFileSizeHigh, d.nFileSizeLow) = ((s >> 32) as u32, s as u32);
        }
        if let Some(t) = f.written {
            d.dwFlags |= FD_WRITESTIME.0 as u32;
            d.ftLastWriteTime = FILETIME { dwLowDateTime: t as u32, dwHighDateTime: (t >> 32) as u32 };
        }
        // The struct is packed: its name is filled whole, not through a reference into it.
        let mut name = [0u16; 260];
        for (slot, c) in name.iter_mut().zip(f.path.encode_utf16().take(259)) {
            *slot = c;
        }
        d.cFileName = name;
        // SAFETY: a plain C struct, read as its bytes.
        out.extend_from_slice(unsafe {
            std::slice::from_raw_parts((&raw const d).cast::<u8>(), std::mem::size_of::<FILEDESCRIPTORW>())
        });
    }
    out
}

impl IDataObject_Impl for Files_Impl {
    fn GetData(&self, f: *const FORMATETC) -> Result<STGMEDIUM> {
        let Some((format, lindex)) = self.which(f) else { return Err(DV_E_FORMATETC.into()) };
        let fm = self.formats;
        if format == fm.descriptor {
            return hglobal(&descriptors(&self.files));
        }
        if format == fm.effect {
            return hglobal(&DROPEFFECT_COPY.0.to_le_bytes());
        }
        let index = usize::try_from(lindex).ok().filter(|&i| self.files.get(i).is_some_and(|f| !f.dir));
        let Some(index) = index else { return Err(DV_E_LINDEX.into()) };
        let stream: IStream = Stream {
            list: self.list,
            index: index as u32,
            size: AtomicU64::new(self.files[index].size.unwrap_or(UNKNOWN)),
            pos: AtomicU64::new(0),
            reads: self.reads.clone(),
        }
        .into();
        Ok(STGMEDIUM { tymed: TYMED_ISTREAM.0 as u32, u: STGMEDIUM_0 { pstm: ManuallyDrop::new(Some(stream)) }, pUnkForRelease: ManuallyDrop::new(None) })
    }

    fn GetDataHere(&self, _: *const FORMATETC, _: *mut STGMEDIUM) -> Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn QueryGetData(&self, f: *const FORMATETC) -> HRESULT {
        if self.which(f).is_some() { S_OK } else { DV_E_FORMATETC }
    }

    fn GetCanonicalFormatEtc(&self, _: *const FORMATETC, out: *mut FORMATETC) -> HRESULT {
        // SAFETY: COM hands a FORMATETC to fill, or null.
        if let Some(out) = unsafe { out.as_mut() } {
            out.ptd = std::ptr::null_mut();
        }
        DATA_S_SAMEFORMATETC
    }

    fn SetData(&self, _: *const FORMATETC, _: *const STGMEDIUM, _: BOOL) -> Result<()> {
        // Explorer reports how the paste went; nothing here needs it.
        Err(E_NOTIMPL.into())
    }

    fn EnumFormatEtc(&self, direction: u32) -> Result<IEnumFORMATETC> {
        if direction != DATADIR_GET.0 as u32 {
            return Err(E_NOTIMPL.into());
        }
        let fm = self.formats;
        let all = [
            Formats::etc(fm.descriptor, TYMED_HGLOBAL, -1),
            Formats::etc(fm.contents, TYMED_ISTREAM, -1),
            Formats::etc(fm.effect, TYMED_HGLOBAL, -1),
        ];
        // SAFETY: the shell copies the array into the enumerator it makes.
        unsafe { SHCreateStdEnumFmtEtc(&all) }
    }

    fn DAdvise(&self, _: *const FORMATETC, _: u32, _: Ref<IAdviseSink>) -> Result<u32> {
        Err(OLE_E_ADVISENOTSUPPORTED.into())
    }

    fn DUnadvise(&self, _: u32) -> Result<()> {
        Err(OLE_E_ADVISENOTSUPPORTED.into())
    }

    fn EnumDAdvise(&self) -> Result<IEnumSTATDATA> {
        Err(OLE_E_ADVISENOTSUPPORTED.into())
    }
}

/// One desktop file's bytes, read from the desktop as they're asked for.
#[implement(IStream)]
struct Stream {
    list: u32,
    index: u32,
    size: AtomicU64,
    pos: AtomicU64,
    reads: UnboundedSender<Read>,
}

impl Stream_Impl {
    /// Asks the session for a part of the file (`size` 0: its length); None if that failed.
    fn ask(&self, position: u64, size: u32) -> Option<Vec<u8>> {
        let (tx, rx) = mpsc::channel();
        self.reads.send(Read { list: self.list, index: self.index, position, size, reply: tx }).ok()?;
        rx.recv_timeout(WAIT).ok().flatten()
    }

    fn size(&self) -> Option<u64> {
        let s = self.size.load(Relaxed);
        if s != UNKNOWN {
            return Some(s);
        }
        let s = u64::from_le_bytes(self.ask(0, 0)?.get(..8)?.try_into().ok()?);
        self.size.store(s, Relaxed);
        Some(s)
    }
}

impl ISequentialStream_Impl for Stream_Impl {
    fn Read(&self, pv: *mut c_void, cb: u32, pcbread: *mut u32) -> HRESULT {
        let Some(size) = self.size() else { return E_FAIL };
        let mut got = 0u32;
        while got < cb {
            let pos = self.pos.load(Relaxed);
            let n = (cb - got).min(CHUNK).min(size.saturating_sub(pos).min(u64::from(u32::MAX)) as u32);
            if n == 0 {
                break;
            }
            let Some(data) = self.ask(pos, n) else {
                if got == 0 {
                    return E_FAIL;
                }
                break;
            };
            let k = data.len().min(n as usize);
            if k == 0 {
                break;
            }
            // SAFETY: the caller's buffer holds `cb` bytes and `got + k <= cb`.
            unsafe { std::ptr::copy_nonoverlapping(data.as_ptr(), pv.cast::<u8>().add(got as usize), k) };
            got += k as u32;
            self.pos.store(pos + k as u64, Relaxed);
        }
        // SAFETY: COM hands a count to fill, or null.
        if let Some(r) = unsafe { pcbread.as_mut() } {
            *r = got;
        }
        S_OK
    }

    fn Write(&self, _: *const c_void, _: u32, _: *mut u32) -> HRESULT {
        STG_E_ACCESSDENIED
    }
}

impl IStream_Impl for Stream_Impl {
    fn Seek(&self, by: i64, origin: STREAM_SEEK, new: *mut u64) -> Result<()> {
        let base = match origin {
            STREAM_SEEK_SET => 0,
            STREAM_SEEK_CUR => self.pos.load(Relaxed),
            STREAM_SEEK_END => self.size().ok_or(Error::from(E_FAIL))?,
            _ => return Err(STG_E_INVALIDFUNCTION.into()),
        };
        let to = base.checked_add_signed(by).ok_or(Error::from(STG_E_INVALIDFUNCTION))?;
        self.pos.store(to, Relaxed);
        // SAFETY: COM hands a position to fill, or null.
        if let Some(n) = unsafe { new.as_mut() } {
            *n = to;
        }
        Ok(())
    }

    fn SetSize(&self, _: u64) -> Result<()> {
        Err(STG_E_ACCESSDENIED.into())
    }

    fn CopyTo(&self, _: Ref<IStream>, _: u64, _: *mut u64, _: *mut u64) -> Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn Commit(&self, _: &STGC) -> Result<()> {
        Ok(())
    }

    fn Revert(&self) -> Result<()> {
        Ok(())
    }

    fn LockRegion(&self, _: u64, _: u64, _: &LOCKTYPE) -> Result<()> {
        Err(STG_E_INVALIDFUNCTION.into())
    }

    fn UnlockRegion(&self, _: u64, _: u64, _: u32) -> Result<()> {
        Err(STG_E_INVALIDFUNCTION.into())
    }

    fn Stat(&self, st: *mut STATSTG, _: &STATFLAG) -> Result<()> {
        let size = self.size().ok_or(Error::from(E_FAIL))?;
        // SAFETY: COM hands a STATSTG to fill; no name is given (null, as STATFLAG_NONAME would).
        if let Some(st) = unsafe { st.as_mut() } {
            *st = STATSTG { r#type: STGTY_STREAM.0 as u32, cbSize: size, ..Default::default() };
        }
        Ok(())
    }

    fn Clone(&self) -> Result<IStream> {
        Err(E_NOTIMPL.into())
    }
}
