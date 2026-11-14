//! The shared clipboard (MS-RDPECLIP, the `cliprdr` channel): text both ways, files from here to
//! the desktop (read from disk as the desktop asks for them) and, on Windows, files from the
//! desktop to here (`ole.rs`). xtermium decides what may cross: it offers this computer's
//! clipboard with `ToHelper::ClipText` / `ClipFiles`, hears of each copy on the desktop as
//! `FromHelper::ClipOffer` and asks for it with `ToHelper::ClipFetch`.

use crate::proto::FromHelper;
use crate::send;
use ironrdp_cliprdr::backend::CliprdrBackend;
use ironrdp_cliprdr::pdu::{
    ClipboardFileAttributes, ClipboardFormat, ClipboardFormatId, ClipboardFormatName, ClipboardGeneralCapabilityFlags,
    FileContentsFlags, FileContentsRequest, FileContentsResponse, FileDescriptor, FormatDataRequest, FormatDataResponse,
    LockDataId, MAX_FILE_COUNT, OwnedFormatDataResponse,
};
use ironrdp_cliprdr::{Client, CliprdrClient, CliprdrSvcMessages};
use ironrdp_pdu::PduResult;
use ironrdp_session::ActiveStage;
use std::collections::HashMap;
use std::io::{Read as _, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering::Relaxed};
use std::sync::mpsc::Sender;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

/// Text bigger than this doesn't cross (xtermium's side refuses messages over 64 MiB).
pub const MAX_TEXT: usize = 16 << 20;
/// A picture bigger than this doesn't cross (an 8K screenshot is 130 MiB; xtermium's side refuses
/// messages over 64 MiB).
pub const MAX_IMAGE: usize = 60 << 20;

/// What this computer's clipboard offers the desktop. Kept across reconnections.
#[derive(Clone, Debug, Default)]
pub enum Offer {
    #[default]
    Nothing,
    Text(String),
    /// Files and folders, as full paths here.
    Files(Vec<PathBuf>),
    /// A picture, as CF_DIB.
    Image(Vec<u8>),
}

/// A file or folder copied on the desktop.
#[derive(Clone, Debug)]
pub struct RemoteFile {
    /// Its path within what was copied, `folder\name`.
    pub path: String,
    pub dir: bool,
    pub size: Option<u64>,
    /// Last written, as a Windows FILETIME.
    pub written: Option<u64>,
}

/// A read of a desktop file for a paste here: `size` bytes from `position`, or with `size` 0 the
/// file's length as 8 bytes (little-endian). `list` is the file list it belongs to (`Clip::list`):
/// a newer copy on the desktop makes reads of an older list fail.
pub struct Read {
    pub list: u32,
    pub index: u32,
    pub position: u64,
    pub size: u32,
    pub reply: Sender<Option<Vec<u8>>>,
}

/// Numbers the desktop's file lists across connections.
static LIST: AtomicU32 = AtomicU32::new(0);

/// The clipboard's state for the whole run: what is offered, and where reads for pastes here come in.
pub struct Kit {
    pub offer: Offer,
    pub reads: UnboundedReceiver<Read>,
    /// Kept so `reads` never ends.
    _reads_tx: UnboundedSender<Read>,
    #[cfg(windows)]
    ole: Option<crate::ole::Ole>,
}

impl Kit {
    pub fn new() -> Self {
        let (tx, reads) = unbounded_channel();
        Self {
            offer: Offer::Nothing,
            reads,
            #[cfg(windows)]
            ole: crate::ole::Ole::start(tx.clone()),
            _reads_tx: tx,
        }
    }

    /// The channel for a new connection.
    pub fn channel(&self) -> CliprdrClient {
        CliprdrClient::new(Box::new(Clip {
            todo: vec![],
            offer: self.offer.clone(),
            local: vec![],
            remote_text: None,
            remote_files: None,
            remote_image: None,
            asked: None,
            list: 0,
            lock: None,
            reads: HashMap::new(),
            next_stream: 0,
            ready: false,
            #[cfg(windows)]
            ole: self.ole.clone(),
        }))
    }
}

/// What the backend needs done on the channel: its callbacks run while a PDU is processed and
/// can't send, so the session does these right after (`flush`).
#[derive(Debug)]
enum Todo {
    /// Tell the desktop what this side's clipboard holds.
    Announce,
    Data(OwnedFormatDataResponse),
    Contents(FileContentsResponse<'static>),
    Paste(ClipboardFormatId),
}

/// The channel's backend.
#[derive(Debug)]
struct Clip {
    todo: Vec<Todo>,
    offer: Offer,
    /// The offered files' paths here, by their index in the list the desktop got.
    local: Vec<PathBuf>,
    /// The desktop's latest copy: its text, file list and picture formats, if it holds them.
    remote_text: Option<ClipboardFormatId>,
    remote_files: Option<ClipboardFormatId>,
    remote_image: Option<ClipboardFormatId>,
    /// The format asked for last (`Todo::Paste`).
    asked: Option<ClipboardFormatId>,
    /// The desktop's file list on this computer's clipboard, and the lock that keeps its
    /// contents readable after the desktop's clipboard changes.
    list: u32,
    lock: Option<u32>,
    /// Reads waiting for the desktop's answer, by stream.
    reads: HashMap<u32, Sender<Option<Vec<u8>>>>,
    next_stream: u32,
    /// The channel finished starting: file lists may go.
    ready: bool,
    #[cfg(windows)]
    ole: Option<crate::ole::Ole>,
}

impl ironrdp_core::AsAny for Clip {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

fn backend(c: &mut CliprdrClient) -> &mut Clip {
    c.downcast_backend_mut::<Clip>().expect("the channel's backend is a Clip")
}

/// Turns what the channel wants sent into frames for the connection. A clipboard step the
/// channel refuses (it isn't ready, say) is dropped: the desktop goes on without it.
fn frames(stage: &mut ActiveStage, msgs: Vec<CliprdrSvcMessages<Client>>) -> Vec<Vec<u8>> {
    msgs.into_iter().filter_map(|m| stage.process_svc_processor_messages(m).ok()).collect()
}

/// Does what the backend asked for while the last PDU was processed.
pub fn flush(stage: &mut ActiveStage) -> Vec<Vec<u8>> {
    let Some(c) = stage.get_svc_processor_mut::<CliprdrClient>() else { return vec![] };
    let todo = std::mem::take(&mut backend(c).todo);
    if todo.is_empty() {
        return vec![];
    }
    let msgs = todo
        .into_iter()
        .filter_map(|t| match t {
            Todo::Announce => announce(c),
            Todo::Data(d) => c.submit_format_data(d),
            Todo::Contents(r) => c.submit_file_contents(r),
            Todo::Paste(f) => c.initiate_paste(f),
        }.ok())
        .collect();
    frames(stage, msgs)
}

/// This computer's clipboard changed (as xtermium saw it): the desktop hears of it.
pub fn offer(stage: &mut ActiveStage, offer: Offer) -> Vec<Vec<u8>> {
    if let Some(c) = stage.get_svc_processor_mut::<CliprdrClient>() {
        let b = backend(c);
        b.offer = offer;
        // Before the channel is up, the offer goes when it asks (`on_request_format_list`).
        if b.ready {
            b.todo.push(Todo::Announce);
        }
    }
    flush(stage)
}

/// Brings the desktop's latest copy here: its files on Windows (`ole.rs`), else its text, else
/// its picture (a copied table offers both text and a picture of itself: the text is wanted).
pub fn fetch(stage: &mut ActiveStage) -> Vec<Vec<u8>> {
    if let Some(c) = stage.get_svc_processor_mut::<CliprdrClient>() {
        let b = backend(c);
        #[cfg(windows)]
        let files = b.remote_files.filter(|_| b.ole.is_some());
        #[cfg(not(windows))]
        let files: Option<ClipboardFormatId> = None;
        if let Some(f) = files.or(b.remote_text).or(b.remote_image) {
            b.asked = Some(f);
            b.todo.push(Todo::Paste(f));
        }
    }
    flush(stage)
}

/// Asks the desktop for part of a file pasted here; the answer goes to `r.reply`.
pub fn read(stage: &mut ActiveStage, r: Read) -> Vec<Vec<u8>> {
    let Some(c) = stage.get_svc_processor_mut::<CliprdrClient>() else {
        let _ = r.reply.send(None);
        return vec![];
    };
    let b = backend(c);
    if r.list != b.list {
        let _ = r.reply.send(None);
        return vec![];
    }
    b.next_stream = b.next_stream.wrapping_add(1);
    let stream_id = b.next_stream;
    let (flags, requested_size) = if r.size == 0 { (FileContentsFlags::SIZE, 8) } else { (FileContentsFlags::RANGE, r.size) };
    let position = if r.size == 0 { 0 } else { r.position };
    let req = FileContentsRequest { stream_id, index: r.index as i32, flags, position, requested_size, data_id: b.lock };
    match c.request_file_contents(req) {
        Ok(m) => {
            backend(c).reads.insert(stream_id, r.reply);
            frames(stage, vec![m])
        }
        Err(_) => {
            let _ = r.reply.send(None);
            vec![]
        }
    }
}

/// Lets go of the desktop's locked copies nobody reads any more; call every few seconds.
pub fn tick(stage: &mut ActiveStage) -> Vec<Vec<u8>> {
    let Some(c) = stage.get_svc_processor_mut::<CliprdrClient>() else { return vec![] };
    match c.drive_timeouts() {
        Ok(m) => frames(stage, vec![m]),
        Err(_) => vec![],
    }
}

fn announce(c: &mut CliprdrClient) -> PduResult<CliprdrSvcMessages<Client>> {
    let b = backend(c);
    match &b.offer {
        Offer::Files(paths) if b.ready => {
            let (files, local) = walk(paths);
            b.local = local;
            c.initiate_file_copy(files)
        }
        Offer::Text(_) => c.initiate_copy(&[ClipboardFormat::new(ClipboardFormatId::CF_UNICODETEXT)]),
        Offer::Image(_) => c.initiate_copy(&[ClipboardFormat::new(ClipboardFormatId::CF_DIB)]),
        // Files go once the channel is ready (`on_ready`); the first announcement can't wait.
        _ => c.initiate_copy(&[]),
    }
}

/// What the desktop gets for `paths`: each file, and each folder followed by everything in it,
/// with the path here of each, in the same order.
fn walk(paths: &[PathBuf]) -> (Vec<FileDescriptor>, Vec<PathBuf>) {
    let mut out = (vec![], vec![]);
    for p in paths {
        add(p, None, &mut out);
    }
    out
}

fn add(p: &Path, folder: Option<&str>, out: &mut (Vec<FileDescriptor>, Vec<PathBuf>)) {
    if out.0.len() >= MAX_FILE_COUNT {
        return;
    }
    let (Ok(meta), Some(name)) = (std::fs::metadata(p), p.file_name().and_then(|n| n.to_str())) else { return };
    // The desktop takes 259 characters at most for the path inside what was copied; the channel
    // would drop a longer one, and the files after it would be read for the wrong ones.
    let len = folder.map_or(0, |f| f.chars().count() + 1) + name.chars().count();
    if len > 259 {
        return;
    }
    let attributes = if meta.is_dir() { ClipboardFileAttributes::DIRECTORY } else { ClipboardFileAttributes::NORMAL };
    let mut d = FileDescriptor::new(name).with_attributes(attributes);
    if !meta.is_dir() {
        d = d.with_file_size(meta.len());
    }
    if let Some(t) = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()) {
        // FILETIME: 100 ns steps since 1601.
        d = d.with_last_write_time((t.as_secs() + 11_644_473_600) * 10_000_000 + u64::from(t.subsec_nanos() / 100));
    }
    if let Some(f) = folder {
        d = d.with_relative_path(f);
    }
    out.0.push(d);
    out.1.push(p.to_path_buf());
    if meta.is_dir() {
        let inside = folder.map_or_else(|| name.to_string(), |f| format!("{f}\\{name}"));
        let mut entries: Vec<PathBuf> = std::fs::read_dir(p).into_iter().flatten().flatten().map(|e| e.path()).collect();
        entries.sort();
        for e in entries {
            add(&e, Some(&inside), out);
        }
    }
}

/// Up to `size` bytes of `path` from `position`.
fn read_at(path: &Path, position: u64, size: u32) -> std::io::Result<Vec<u8>> {
    let mut f = std::fs::File::open(path)?;
    f.seek(SeekFrom::Start(position))?;
    let mut v = vec![];
    f.take(u64::from(size)).read_to_end(&mut v)?;
    Ok(v)
}

impl CliprdrBackend for Clip {
    fn temporary_directory(&self) -> &str {
        // Only used for file paths, which aren't sent (`FILECLIP_NO_FILE_PATHS`).
        ""
    }

    fn client_capabilities(&self) -> ClipboardGeneralCapabilityFlags {
        ClipboardGeneralCapabilityFlags::USE_LONG_FORMAT_NAMES
            | ClipboardGeneralCapabilityFlags::STREAM_FILECLIP_ENABLED
            | ClipboardGeneralCapabilityFlags::FILECLIP_NO_FILE_PATHS
            | ClipboardGeneralCapabilityFlags::CAN_LOCK_CLIPDATA
            | ClipboardGeneralCapabilityFlags::HUGE_FILE_SUPPORT_ENABLED
    }

    fn on_ready(&mut self) {
        self.ready = true;
        if matches!(self.offer, Offer::Files(_)) {
            self.todo.push(Todo::Announce);
        }
    }

    fn on_request_format_list(&mut self) {
        self.todo.push(Todo::Announce);
    }

    fn on_process_negotiated_capabilities(&mut self, _: ClipboardGeneralCapabilityFlags) {}

    fn on_remote_copy(&mut self, formats: &[ClipboardFormat]) {
        let has = |id| formats.iter().any(|f| f.id == id).then_some(id);
        self.remote_text = has(ClipboardFormatId::CF_UNICODETEXT).or(has(ClipboardFormatId::CF_TEXT));
        self.remote_image = has(ClipboardFormatId::CF_DIB);
        self.remote_files = formats
            .iter()
            .find(|f| f.name.as_ref().is_some_and(|n| n.value() == ClipboardFormatName::FILE_LIST.value()))
            .map(|f| f.id);
        let (text, files, image) = (self.remote_text.is_some(), self.remote_files.is_some(), self.remote_image.is_some());
        let _ = send(&FromHelper::ClipOffer { text, files, image });
    }

    fn on_format_data_request(&mut self, r: FormatDataRequest) {
        let res = match &self.offer {
            Offer::Text(t) if r.format == ClipboardFormatId::CF_UNICODETEXT => OwnedFormatDataResponse::new_unicode_string(t),
            Offer::Text(t) if r.format == ClipboardFormatId::CF_TEXT => OwnedFormatDataResponse::new_string(t),
            Offer::Image(dib) if r.format == ClipboardFormatId::CF_DIB => OwnedFormatDataResponse::new_data(dib.clone()),
            _ => OwnedFormatDataResponse::new_error(),
        };
        self.todo.push(Todo::Data(res));
    }

    fn on_format_data_response(&mut self, r: FormatDataResponse<'_>) {
        let asked = self.asked.take();
        if r.is_error() || asked.is_none() {
            return;
        }
        if asked == self.remote_image {
            if r.data().len() <= MAX_IMAGE {
                let _ = send(&FromHelper::ClipImage(r.data().to_vec()));
            }
            return;
        }
        if asked != self.remote_text {
            return;
        }
        let text = if asked == Some(ClipboardFormatId::CF_UNICODETEXT) { r.to_unicode_string() } else { r.to_string() };
        if let Ok(t) = text
            && t.len() <= MAX_TEXT
        {
            let _ = send(&FromHelper::ClipText(t));
        }
    }

    fn on_file_contents_request(&mut self, r: FileContentsRequest) {
        let path = usize::try_from(r.index).ok().and_then(|i| self.local.get(i));
        let res = path
            .and_then(|p| {
                if r.flags.contains(FileContentsFlags::SIZE) {
                    Some(FileContentsResponse::new_size_response(r.stream_id, std::fs::metadata(p).ok()?.len()))
                } else {
                    Some(FileContentsResponse::new_data_response(r.stream_id, read_at(p, r.position, r.requested_size).ok()?))
                }
            })
            .unwrap_or_else(|| FileContentsResponse::new_error(r.stream_id));
        self.todo.push(Todo::Contents(res));
    }

    fn on_file_contents_response(&mut self, r: FileContentsResponse<'_>) {
        if let Some(tx) = self.reads.remove(&r.stream_id()) {
            let _ = tx.send((!r.is_error()).then(|| r.data().to_vec()));
        }
    }

    fn on_lock(&mut self, _: LockDataId) {}

    fn on_unlock(&mut self, _: LockDataId) {}

    fn on_remote_file_list(&mut self, files: &[FileDescriptor], lock: Option<u32>) {
        self.list = LIST.fetch_add(1, Relaxed) + 1;
        self.lock = lock;
        #[cfg(windows)]
        if let Some(ole) = &self.ole {
            let files = files
                .iter()
                .map(|f| RemoteFile {
                    path: f.relative_path.as_ref().map_or_else(|| f.name.clone(), |p| format!("{p}\\{}", f.name)),
                    dir: f.attributes.is_some_and(|a| a.contains(ClipboardFileAttributes::DIRECTORY)),
                    size: f.file_size,
                    written: f.last_write_time,
                })
                .collect();
            ole.offer(self.list, files);
        }
        #[cfg(not(windows))]
        let _ = files;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder goes before what's in it, nested paths are `folder\name`, and each descriptor's
    /// path here sits at the same index: the desktop reads files by that index.
    #[test]
    fn folders_walk_in_order() {
        let root = std::env::temp_dir().join(format!("xtermium-rdp-walk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("top/sub")).unwrap();
        std::fs::write(root.join("top/a.txt"), b"hello").unwrap();
        std::fs::write(root.join("top/sub/b.txt"), b"hi").unwrap();
        std::fs::write(root.join("c.txt"), b"x").unwrap();
        let (files, local) = walk(&[root.join("top"), root.join("c.txt"), root.join("missing")]);
        let names: Vec<_> = files.iter().map(|f| (f.relative_path.clone(), f.name.as_str(), f.file_size)).collect();
        assert_eq!(
            names,
            [
                (None, "top", None),
                (Some("top".into()), "a.txt", Some(5)),
                (Some("top".into()), "sub", None),
                (Some("top\\sub".into()), "b.txt", Some(2)),
                (None, "c.txt", Some(1)),
            ]
        );
        assert_eq!(local[3], root.join("top/sub/b.txt"));
        assert_eq!(read_at(&local[1], 1, 3).unwrap(), b"ell");
        assert_eq!(read_at(&local[1], 4, 100).unwrap(), b"o");
        std::fs::remove_dir_all(&root).unwrap();
    }
}
