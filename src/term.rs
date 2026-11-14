//! A terminal tab: wezterm-term VT engine + a backend (SSH / local PTY / serial / telnet).
//! The UI draws it: each change goes out as a frame holding only the rows that differ from the
//! last frame, plus any image the UI hasn't been sent yet.

use crate::config::{Kind, Session, Style};
use crate::prompt::Prompter;
use crate::settings::{Cursor, Scheme, rgb};
use crate::ssh::{self, Ctl};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::ipc::{Channel, InvokeResponseBody};
use tokio::sync::Notify;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use wezterm_surface::CursorVisibility;
use wezterm_term::color::{ColorAttribute, ColorPalette, SrgbaTuple};
use wezterm_term::image::{ImageData, ImageDataType};
use wezterm_term::{
    CellAttributes, Intensity, KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind, StableRowIndex,
    Terminal, TerminalConfiguration, TerminalSize, Underline,
};

#[derive(Debug)]
struct TermConfig(ColorPalette, usize);

impl TerminalConfiguration for TermConfig {
    fn color_palette(&self) -> ColorPalette {
        self.0.clone()
    }
    fn scrollback_size(&self) -> usize {
        self.1
    }
    fn enable_kitty_graphics(&self) -> bool {
        true
    }
}

/// The terminal's answerbacks and our key encodings go to the backend through this.
/// Shared with the tab, which points it at the new connection on reconnect: the terminal (and
/// its scrollback) stays.
type Writer = Arc<Mutex<UnboundedSender<Ctl>>>;
struct ChanWriter(Writer);

impl Write for ChanWriter {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        let _ = self.0.lock().unwrap().send(Ctl::Input(b.to_vec()));
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

type Cell = (StableRowIndex, usize);

/// Matches of the find bar's text: first and last cell of each, oldest first.
struct Find {
    hits: Vec<(Cell, Cell)>,
    /// The one shown and highlighted strongest.
    current: Option<usize>,
}

/// More are not counted or highlighted (a one-letter search in a long scrollback).
const MAX_HITS: usize = 10_000;
/// Match highlights: every match, and the current one.
const HIT_BG: u32 = 0xe5c07b;
const HIT_NOW_BG: u32 = 0xff8f3f;
const HIT_FG: u32 = 0x1a1a1a;

/// Byte ranges of the web addresses in `text`: from `http://` or `https://` to the next space or
/// character that can't be in one, without trailing punctuation or an unmatched closing bracket
/// (`(see https://x/y)`, `https://x/y.`).
fn urls(text: &str) -> Vec<std::ops::Range<usize>> {
    let mut out = vec![];
    let mut from = 0;
    while let Some(at) = text[from..].find("http").map(|i| i + from) {
        let rest = &text[at..];
        let scheme = ["https://", "http://"].into_iter().find(|s| rest.starts_with(s)).map_or(0, str::len);
        // Not the middle of a word ("xhttp://").
        let inside = text[..at].chars().next_back().is_some_and(char::is_alphanumeric);
        if scheme == 0 || inside {
            from = at + 4;
            continue;
        }
        let len = rest.find(|c: char| c.is_whitespace() || c.is_control() || "<>\"'`{}|\\^".contains(c)).unwrap_or(rest.len());
        let mut url = &rest[..len];
        loop {
            let unmatched = |open, close| url.ends_with(close) && url.matches(open).count() < url.matches(close).count();
            if url.ends_with(['.', ',', ';', ':', '!', '?']) || unmatched('(', ')') || unmatched('[', ']') {
                url = &url[..url.len() - 1];
            } else {
                break;
            }
        }
        if url.len() > scheme {
            out.push(at..at + url.len());
        }
        from = at + len.max(scheme);
    }
    out
}

/// The web address at a cell, and the cells it covers: an OSC 8 hyperlink a program printed, or
/// an address in the text (the whole line, rows the terminal wrapped included).
fn link_at(term: &Terminal, at: Cell) -> Option<(String, (Cell, Cell))> {
    let screen = term.screen();
    let mut phys = screen.stable_row_to_phys(at.0)?;
    let line = |p: usize| screen.lines_in_phys_range(p..p + 1).pop();
    let web = |u: &str| u.starts_with("https://") || u.starts_with("http://");
    // OSC 8: the cells around this one carrying the same link.
    let here = line(phys)?;
    let explicit = here.visible_cells().find(|c| c.cell_index() <= at.1 && at.1 < c.cell_index() + c.width().max(1));
    if let Some(link) = explicit.and_then(|c| c.attrs().hyperlink().cloned())
        && !link.is_implicit()
    {
        if !web(link.uri()) {
            return None;
        }
        let same: Vec<usize> =
            here.visible_cells().filter(|c| c.attrs().hyperlink() == Some(&link)).map(|c| c.cell_index()).collect();
        let (a, b) = (*same.first()?, *same.last()?);
        return Some((link.uri().to_string(), ((at.0, a), (at.0, b))));
    }
    // Plain text: the logical line this row is part of.
    while phys > 0 && line(phys - 1).is_some_and(|l| l.last_cell_was_wrapped()) {
        phys -= 1;
    }
    let (mut text, mut cells) = (String::new(), vec![]);
    for p in phys..screen.scrollback_rows() {
        let l = line(p)?;
        let row = screen.phys_to_stable_row_index(p);
        for c in l.visible_cells() {
            cells.push((text.len(), (row, c.cell_index())));
            text.push_str(c.str());
        }
        if !l.last_cell_was_wrapped() {
            break;
        }
    }
    let cell = |offset: usize| cells[cells.partition_point(|c| c.0 <= offset).saturating_sub(1)].1;
    urls(&text).into_iter().map(|r| (cell(r.start), cell(r.end - 1), r)).find_map(|(a, b, r)| {
        (at >= a && at <= b).then(|| (text[r].to_string(), (a, b)))
    })
}

/// Every place `query` occurs in the scrollback and screen. Lines wrapped by the terminal are
/// searched as one, so a match may span rows. `case`: match case exactly.
fn search(term: &Terminal, query: &str, case: bool) -> Vec<(Cell, Cell)> {
    if query.is_empty() {
        return vec![];
    }
    let fold = |s: &str| if case { s.to_string() } else { s.to_lowercase() };
    let query = fold(query);
    let screen = term.screen();
    let total = screen.scrollback_rows();
    let mut hits = vec![];
    // One logical line: its text, and where each cell starts in it (offset, row, column, width).
    let mut text = String::new();
    let mut cells: Vec<(usize, StableRowIndex, usize, usize)> = vec![];
    let flush = |text: &mut String, cells: &mut Vec<(usize, StableRowIndex, usize, usize)>, hits: &mut Vec<_>| {
        let at = |offset: usize| cells[cells.partition_point(|c| c.0 <= offset).saturating_sub(1)];
        for (start, m) in text.match_indices(&query) {
            let (a, b) = (at(start), at(start + m.len() - 1));
            if hits.len() < MAX_HITS {
                hits.push(((a.1, a.2), (b.1, b.2 + b.3.max(1) - 1)));
            }
        }
        text.clear();
        cells.clear();
    };
    // In slices: the scrollback can hold hundreds of thousands of rows.
    for chunk in (0..total).step_by(1024) {
        for (p, line) in screen.lines_in_phys_range(chunk..(chunk + 1024).min(total)).iter().enumerate() {
            let row = screen.phys_to_stable_row_index(chunk + p);
            for cell in line.visible_cells() {
                cells.push((text.len(), row, cell.cell_index(), cell.width()));
                text.push_str(&fold(cell.str()));
            }
            if !line.last_cell_was_wrapped() {
                flush(&mut text, &mut cells, &mut hits);
            }
        }
    }
    flush(&mut text, &mut cells, &mut hits);
    hits
}

/// How a tab looks and behaves; derived from settings (and the session's own style).
#[derive(Clone)]
pub struct Look {
    /// Font family; empty = the UI's default monospace.
    pub font: String,
    pub font_size: f32,
    pub palette: ColorPalette,
    pub scrollback: usize,
    /// Output brings a screen scrolled back to the bottom; else the lines on screen stay there.
    pub scroll_on_output: bool,
    pub cursor: Cursor,
    pub cursor_blink: bool,
    /// Line height as a multiple of the font's.
    pub line_height: f32,
    pub copy_on_select: bool,
    pub right_click_paste: bool,
    /// Trim whitespace (a trailing newline runs the command) off the start and end of pastes.
    pub strip_paste: bool,
    /// Save the output to a file (Settings > Terminal); decided when a tab opens.
    pub log: bool,
    /// Highlight rules (Settings > Terminal), compiled: pattern and colour, first rule first.
    pub highlights: Arc<Vec<(regex::Regex, u32)>>,
}

/// Columns a highlight rule colours: from, to (exclusive), colour.
type Span = (usize, usize, u32);

/// The columns of `line` that highlight rules colour, the first rule's matches first. Only
/// called while there are rules.
fn highlight_spans(line: &wezterm_term::Line, rules: &[(regex::Regex, u32)]) -> Vec<Span> {
    // The row's text, and where each cell starts in it (byte, column).
    let mut text = String::new();
    let mut at: Vec<(usize, usize)> = vec![];
    for cell in line.visible_cells() {
        at.push((text.len(), cell.cell_index()));
        text.push_str(cell.str());
    }
    if text.trim().is_empty() {
        return vec![];
    }
    let col = |byte: usize| at.get(at.partition_point(|(b, _)| *b < byte)).map_or(usize::MAX, |c| c.1);
    let mut spans = vec![];
    for (re, color) in rules {
        for m in re.find_iter(&text).filter(|m| !m.is_empty()) {
            spans.push((col(m.start()), col(m.end()), *color));
        }
    }
    spans
}

/// A keystroke or paste. Broadcast replays these (not bytes) so each tab encodes them for
/// its own terminal modes, e.g. application cursor keys in vim vs. a plain shell.
#[derive(Clone, Debug)]
pub enum Typed {
    Key(KeyCode, KeyModifiers),
    Paste(String),
}

/// Text typed key by key: a line break presses Enter, encoded for the terminal's mode like a typed
/// one (snippets, and a Co-op viewer's typing).
pub fn typed(text: &str) -> Vec<Typed> {
    text.replace("\r\n", "\n")
        .chars()
        .map(|c| match c {
            '\n' | '\r' => Typed::Key(KeyCode::Enter, KeyModifiers::NONE),
            c => Typed::Key(KeyCode::Char(c), KeyModifiers::NONE),
        })
        .collect()
}

impl Look {
    /// The clipboard's text, ready to paste.
    fn pasted(&self) -> Option<String> {
        clipboard(|c| c.get_text()).map(|s| self.strip(s))
    }

    /// Text to paste, trimmed when Settings › Terminal says so.
    fn strip(&self, s: String) -> String {
        if self.strip_paste { s.trim().to_owned() } else { s }
    }
}

fn apply(term: &mut Terminal, ev: &Typed) {
    let _ = match ev {
        Typed::Key(k, m) => term.key_down(*k, *m),
        Typed::Paste(s) => term.send_paste(s),
    };
}

/// A key press from the UI (DOM `KeyboardEvent.key` plus modifiers). Serialised too: a Co-op
/// viewer's keys travel to the tab's owner (coop.rs).
#[derive(Serialize, Deserialize)]
pub struct Key {
    pub key: String,
    #[serde(default)]
    pub ctrl: bool,
    #[serde(default)]
    pub alt: bool,
    #[serde(default)]
    pub shift: bool,
    #[serde(default)]
    pub meta: bool,
}

/// A pointer event over the grid. `x`/`y` are device pixels from the grid's top-left corner.
#[derive(Serialize, Deserialize)]
pub struct Mouse {
    pub kind: MouseKind,
    /// 0 left, 1 middle, 2 right.
    #[serde(default)]
    pub button: u8,
    pub x: f32,
    pub y: f32,
    /// Wheel steps, positive = up.
    #[serde(default)]
    pub lines: i32,
    #[serde(default)]
    pub ctrl: bool,
    #[serde(default)]
    pub alt: bool,
    #[serde(default)]
    pub shift: bool,
    /// Presses in a row: 2 selects a word, 3 a line.
    #[serde(default)]
    pub clicks: u8,
}

#[derive(Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum MouseKind {
    Down,
    Up,
    Move,
    Wheel,
}

/// Run flags.
const BOLD: u8 = 1;
const ITALIC: u8 = 2;
const UNDERLINE: u8 = 4;
const STRIKE: u8 = 8;
const DIM: u8 = 16;

/// Cells drawn alike: (column, width in cells, text, fg, bg or None for the default, flags).
/// Plain ASCII merges into one run; anything else gets its own so odd glyph advances can't
/// shift the grid.
#[derive(Serialize, PartialEq, Clone)]
struct Run(u16, u16, String, u32, Option<u32>, u8);

/// One image placement: destination in cells (x0, y0, x1, y1) and normalized texture coords.
#[derive(Serialize, PartialEq, Clone)]
struct Placement {
    hash: String,
    dst: [f32; 4],
    uv: [f32; 4],
}

#[derive(Serialize, PartialEq, Clone)]
struct Meta {
    cursor: Option<(usize, usize)>,
    title: String,
    alive: bool,
    /// The session finished on its own (logout, exit): the UI closes the tab.
    ended: bool,
    /// The connection dropped after it was up: the UI may reconnect by itself.
    lost: bool,
    /// The shell's folder, as it reports it (OSC 7); "" until it does. The file panel follows it.
    cwd: String,
    /// Ctrl is over a link: the pointer becomes a hand.
    link: bool,
    scrolled: bool,
    /// For the scroll bar: rows scrolled back from the bottom, and rows of scrollback above the screen.
    scrollback: (usize, usize),
    images: Vec<Placement>,
    /// While Co-op shares the tab: the stable index of the top row on screen (a viewer keeps its
    /// own selection in stable rows), and whether the program takes the mouse (a viewer's drag
    /// goes to it then, else it selects).
    #[serde(skip_serializing_if = "Option::is_none")]
    shared: Option<(StableRowIndex, bool)>,
}

/// What a receiver of frames has: the rows, meta and images the next frame is a difference from.
#[derive(Default)]
struct Sent {
    rows: Vec<Vec<Run>>,
    meta: Option<Meta>,
    images: HashSet<[u8; 32]>,
}

#[derive(Serialize)]
struct StyleMsg<'a> {
    font: &'a str,
    size: f32,
    cursor: Cursor,
    blink: bool,
    line: f32,
    fg: u32,
    bg: u32,
    /// Cursor colour.
    cc: u32,
}

#[derive(Serialize)]
struct Frame<'a> {
    rows: usize,
    /// The screen moved up this many rows (down if negative) before `lines` apply.
    #[serde(skip_serializing_if = "is_zero")]
    scroll: i32,
    lines: Vec<(usize, &'a Vec<Run>)>,
    #[serde(skip_serializing_if = "Option::is_none")]
    meta: Option<&'a Meta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    style: Option<StyleMsg<'a>>,
}

/// Message types on a tab's channel (first byte).
const MSG_FRAME: u8 = 0;
const MSG_IMAGE: u8 = 1;
/// On a Co-op sink only (coop.rs `fan_out` takes it off, it never reaches a window): `[7]` and the
/// owner's selection as JSON, `[[row, col], [row, col], block]` in stable rows, or `null`.
pub const MSG_SELECTION: u8 = 7;

/// A selection's ends (stable row, column) and whether it is a rectangle.
pub type Picked = (Cell, Cell, bool);

struct Inner {
    term: Terminal,
    /// Where the terminal's answers (and encoded keys) go: the current connection.
    writer: Writer,
    /// Bumped on reconnect so the old backend's late output is ignored.
    generation: u64,
    alive: bool,
    ended: bool,
    /// The connection dropped (ssh::Lost): the UI may reconnect by itself.
    lost: bool,
    closed: bool,
    look: Look,
    size: (usize, usize),
    /// Cell size in device pixels, for pixel-precise mouse reports.
    cell: (f32, f32),
    scroll: usize,
    sel: Option<(Cell, Cell)>,
    /// The selection is the rectangle between its two cells (Alt+drag), not the text between them.
    block: bool,
    /// The find bar's matches, highlighted while it's open.
    find: Option<Find>,
    /// The link under the pointer while Ctrl is held: underlined, opened by a click.
    hover: Option<(Cell, Cell)>,
    /// Some(moved) while the left button is selecting.
    drag: Option<bool>,
    /// A double (false) or triple (true) click's word or line: dragging on selects whole words
    /// or lines from it.
    unit: Option<(bool, (Cell, Cell))>,
    /// What the UI has: rows, meta (None = send everything again) and images.
    sent: Vec<Vec<Run>>,
    meta: Option<Meta>,
    images: HashSet<[u8; 32]>,
    /// While shared, what the Co-op viewers have, when it isn't what the window has: their frames
    /// leave out this side's selection. None: they get the window's very bytes.
    theirs: Option<Sent>,
    /// The selection the Co-op sink last heard of (`MSG_SELECTION`); None: not told yet.
    told: Option<Option<Picked>>,
    /// Highlight spans of the rows on screen, by row, with the row's change number: a row that
    /// didn't change isn't matched again (the frame builder visits every row on every frame).
    highlighted: HashMap<StableRowIndex, (wezterm_surface::SequenceNo, Vec<Span>)>,
    /// When output last arrived (automation waits for it to go quiet).
    last_output: std::time::Instant,
    log: Option<Log>,
    /// Terminal Co-op (coop.rs): a second place the frames go, while the tab is shared.
    share: Option<UnboundedSender<Vec<u8>>>,
}

/// `%41` → `A`: a `file://` path's segments as the shell wrote them.
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        if b[i] == b'%'
            && let (Some(h), Some(l)) = (b.get(i + 1).copied().and_then(hex), b.get(i + 2).copied().and_then(hex))
        {
            out.push((h * 16 + l) as u8);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A tab's output as plain text in the logs folder (Settings > Terminal > Logging). Lines are
/// written once finished (the cursor has left them), as the terminal shows them: colours,
/// cursor moves, backspaces and redrawn progress bars never reach the file. Full-screen
/// programs (the alternate screen) aren't logged. A thread writes, off the terminal's lock.
struct Log {
    tx: std::sync::mpsc::Sender<String>,
    /// The next stable row to write.
    next: StableRowIndex,
    /// What was written for the rows still on screen: a row the cursor comes back to is written
    /// again only if it changed (a clear, a multi-line progress display), never twice the same.
    written: std::collections::BTreeMap<StableRowIndex, String>,
}

impl Log {
    fn new(tx: std::sync::mpsc::Sender<String>) -> Self {
        Self { tx, next: 0, written: Default::default() }
    }
}

/// In the data folder.
pub fn logs_dir() -> std::path::PathBuf {
    crate::config::dir().join("logs")
}

/// Where a session connects, for the log's first line.
fn target(k: &Kind) -> String {
    match k {
        Kind::Ssh(s) => s.target.label(),
        Kind::Shell { program, .. } if program.is_empty() => "local shell".into(),
        Kind::Shell { program, .. } => program.here().into(),
        Kind::Serial { port, baud } => format!("{port} at {baud} baud"),
        Kind::Rdp { host, .. } => host.clone(),
        Kind::Browser { url, .. } => url.clone(),
        Kind::Telnet { host, port } => format!("telnet {host}:{port}"),
        Kind::Winrm { host, port, .. } => format!("winrm {host}:{port}"),
        Kind::Vnc { host, port, .. } => format!("vnc {host}:{port}"),
        Kind::Kvm { host, port, .. } => format!("kvm {host}:{port}"),
        Kind::Agent { tool, args, mode, effort, model, resume, .. } => Kind::agent_command(tool, args, mode, effort, model, resume),
    }
}

/// A new file in the logs folder, `<local date and time> <session><ext>`, not overwriting one.
fn new_file(session: &Session, ext: &str) -> Option<(std::path::PathBuf, std::io::BufWriter<std::fs::File>)> {
    let dir = logs_dir();
    std::fs::create_dir_all(&dir).ok()?;
    let now = chrono::Local::now();
    let name = crate::clean::file_name(&format!("{} {}", now.format("%Y-%m-%d %H-%M-%S"), session.name));
    let path = (1..100)
        .map(|n| dir.join(if n == 1 { format!("{name}{ext}") } else { format!("{name} ({n}){ext}") }))
        .find(|p| !p.exists())?;
    let file = std::io::BufWriter::with_capacity(1 << 16, std::fs::File::create(&path).ok()?);
    Some((path, file))
}

/// A thread writing what's sent to `file`, off the terminal's lock. It flushes when things go
/// quiet, and finishes the file once every sender is gone.
fn writer(mut file: std::io::BufWriter<std::fs::File>) -> std::sync::mpsc::Sender<String> {
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        use std::sync::mpsc::RecvTimeoutError;
        loop {
            match rx.recv_timeout(std::time::Duration::from_millis(500)) {
                Ok(text) => {
                    if file.write_all(text.as_bytes()).is_err() {
                        return;
                    }
                }
                // Quiet for a moment: what's written so far reaches the disk.
                Err(RecvTimeoutError::Timeout) => {
                    let _ = file.flush();
                }
                Err(RecvTimeoutError::Disconnected) => {
                    let _ = file.flush();
                    return;
                }
            }
        }
    });
    tx
}

impl Log {
    /// A new file `<local date and time> <session>.log.txt`; None if it can't be made.
    fn open(session: &Session) -> Option<Self> {
        let (_, mut file) = new_file(session, ".log.txt")?;
        let when = chrono::Local::now().format("%Y-%m-%d %H:%M:%S (UTC%:z)");
        writeln!(file, "xtermium log of {} ({}), started {when}\n", session.name, target(&session.kind)).ok()?;
        Some(Log::new(writer(file)))
    }
}

impl Inner {
    /// The find bar: searches for `query` again (output may have arrived), then moves `step`
    /// matches (negative: older, up) and scrolls the current one into view. Returns the number
    /// of matches and the current one's position (1-based, 0 if none). An empty query clears.
    fn find(&mut self, query: &str, case: bool, step: i32) -> (usize, usize) {
        let i = self;
        let hits = search(&i.term, query, case);
        if hits.is_empty() {
            i.find = (!query.is_empty()).then_some(Find { hits, current: None });
            return (0, 0);
        }
        let (cols, rows) = i.size;
        let total = i.term.screen().scrollback_rows();
        // Keep the current match if it's still there; else start from the bottom of the view.
        let before = i.find.as_ref().and_then(|f| f.current.map(|c| f.hits[c]));
        let bottom = i.term.screen().phys_to_stable_row_index(total.saturating_sub(i.scroll + 1));
        let mut current = match before.and_then(|b| hits.iter().position(|h| h.0 >= b.0)) {
            Some(c) => c,
            None => hits.iter().rposition(|h| h.0.0 <= bottom).unwrap_or(hits.len() - 1),
        };
        current = (current as i64 + step as i64).rem_euclid(hits.len() as i64) as usize;
        // Scroll so the match shows, a third of the way down, if it's off screen.
        if cols > 0
            && let Some(phys) = i.term.screen().stable_row_to_phys(hits[current].0.0)
        {
            let top = total.saturating_sub(rows + i.scroll);
            if phys < top || phys >= top + rows {
                let start = phys.saturating_sub(rows / 3);
                i.scroll = total.saturating_sub(rows).saturating_sub(start);
            }
        }
        let n = hits.len();
        i.find = Some(Find { hits, current: Some(current) });
        (n, current + 1)
    }

    /// The session starts again in the same terminal: what it showed stays above. Modes the old
    /// session may have left on are undone (full-screen program, mouse reporting, hidden cursor,
    /// scroll region, bracketed paste, application keys), a line marks the reconnect, and the
    /// terminal's answers go to the new connection.
    fn reconnecting(&mut self, tx: UnboundedSender<Ctl>) {
        *self.writer.lock().unwrap() = tx;
        // Leaving the alternate screen restores the saved cursor, and resetting the scroll region
        // homes it: only the first when it's on, the second between save and restore, or the
        // cursor jumps up and the new session overwrites the old output.
        if self.term.is_alt_screen_active() {
            self.term.advance_bytes("\x1b[?1049l");
        }
        let when = chrono::Local::now().format("%H:%M:%S");
        self.term.advance_bytes(format!(
            "\x1b[0m\x1b7\x1b[r\x1b8\x1b[?25h\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l\x1b[?1015l\x1b[?2004l\x1b[?1l\x1b>\
             \r\n\x1b[2m── reconnecting at {when} ──\x1b[0m\r\n"
        ));
    }

    /// Writes the lines finished since the last call; `all` writes the rest too (the session
    /// ended or the tab closed).
    fn log_lines(&mut self, all: bool) {
        let Some(log) = &mut self.log else { return };
        if self.term.is_alt_screen_active() {
            return;
        }
        let screen = self.term.screen();
        let top = screen.visible_row_to_stable_row(0);
        let cursor = screen.visible_row_to_stable_row(self.term.cursor_pos().y);
        // The cursor went back up (a clear, a redraw): the screen's rows are looked at again as
        // they're finished, and written again where they changed.
        if cursor < log.next {
            log.next = top;
        }
        log.written.retain(|r, _| *r >= top);
        let total = screen.scrollback_rows();
        let end = if all { screen.phys_to_stable_row_index(total) } else { cursor };
        if end <= log.next {
            return;
        }
        let mut text = String::new();
        let first = screen.phys_to_stable_row_index(0);
        if log.next < first {
            text.push_str(&format!("[… {} lines scrolled away before they could be saved]\n", first - log.next));
            log.next = first;
        }
        let Some(from) = screen.stable_row_to_phys(log.next) else { return };
        let to = (from + (end - log.next) as usize).min(total);
        for (row, line) in (log.next..).zip(screen.lines_in_phys_range(from..to)) {
            let s = line.as_str();
            let row_text = if line.last_cell_was_wrapped() { s.to_string() } else { format!("{}\n", s.trim_end()) };
            if row >= top {
                // Unchanged, or wiped by a clear: nothing new to write.
                let before = log.written.insert(row, row_text.clone());
                if before.as_ref() == Some(&row_text) || (before.is_some() && row_text == "\n") {
                    continue;
                }
            }
            text.push_str(&row_text);
        }
        if all {
            // The screen's empty rows below the last output.
            text.truncate(text.trim_end().len());
            text.push('\n');
        }
        log.next = end;
        if !text.is_empty() {
            let _ = log.tx.send(text);
        }
    }
}

pub struct Tab {
    pub session: Session,
    inner: Arc<Mutex<Inner>>,
    tx: UnboundedSender<Ctl>,
    notify: Arc<Notify>,
    /// Where frames go: the window showing the tab (it can move to another, `attach`).
    out: Arc<Mutex<Channel>>,
    prompt: Prompter,
    /// The SFTP panel, once opened.
    pub files: Option<crate::files::Browser>,
    /// What the SSH connection negotiated, and its round trip (the status bar).
    link: Arc<Mutex<ssh::Link>>,
    /// When the session last started (opened or restarted).
    since: std::time::SystemTime,
}

/// The status bar's facts about a terminal tab.
#[derive(Serialize)]
pub struct Info {
    /// Seconds since the Unix epoch.
    since: u64,
    alive: bool,
    cols: usize,
    rows: usize,
    /// SSH tabs: cipher, key exchange, host key, compression, round trip.
    link: Option<ssh::Link>,
}

fn new_term(look: &Look, tx: Writer) -> Terminal {
    Terminal::new(
        TerminalSize::default(),
        Arc::new(TermConfig(look.palette.clone(), look.scrollback)),
        "xtermium",
        env!("CARGO_PKG_VERSION"),
        Box::new(ChanWriter(tx)),
    )
}

/// System clipboard, kept open: on X11 the copied text lives only as long as its owner.
pub fn clipboard<R>(f: impl FnOnce(&mut arboard::Clipboard) -> Result<R, arboard::Error>) -> Option<R> {
    static CB: Mutex<Option<arboard::Clipboard>> = Mutex::new(None);
    let mut cb = CB.lock().unwrap();
    if cb.is_none() {
        *cb = arboard::Clipboard::new().ok();
    }
    f(cb.as_mut()?).ok()
}

impl Inner {
    /// A fresh terminal, not yet sized.
    fn new(look: Look, writer: Writer, log: Option<Log>) -> Self {
        let term = new_term(&look, writer.clone());
        Self {
            log,
            term,
            writer,
            generation: 0,
            alive: true,
            ended: false,
            lost: false,
            find: None,
            hover: None,
            closed: false,
            look,
            size: (0, 0),
            cell: (1.0, 1.0),
            scroll: 0,
            sel: None,
            block: false,
            drag: None,
            unit: None,
            sent: vec![],
            meta: None,
            images: HashSet::new(),
            theirs: None,
            told: None,
            highlighted: HashMap::new(),
            last_output: std::time::Instant::now(),
            share: None,
        }
    }
}

impl Tab {
    pub fn open(session: Session, look: Look, prompt: Prompter, out: Channel) -> Self {
        let (tx, rx) = unbounded_channel();
        let writer: Writer = Arc::new(Mutex::new(tx.clone()));
        let log = if look.log { Log::open(&session) } else { None };
        let inner = Inner::new(look, writer, log);
        let tab = Self {
            session,
            inner: Arc::new(Mutex::new(inner)),
            tx,
            notify: Arc::new(Notify::new()),
            out: Arc::new(Mutex::new(out)),
            prompt,
            files: None,
            link: Default::default(),
            since: std::time::SystemTime::now(),
        };
        tab.start(rx, 0);
        tauri::async_runtime::spawn(frames(tab.inner.clone(), tab.notify.clone(), tab.out.clone()));
        tab.notify.notify_one(); // the UI needs the font before it can size the grid
        tab
    }

    fn start(&self, rx: UnboundedReceiver<Ctl>, generation: u64) {
        let feed = {
            let (inner, notify) = (self.inner.clone(), self.notify.clone());
            move |b: &[u8]| {
                let mut i = inner.lock().unwrap();
                if i.generation == generation {
                    i.output(b);
                    i.last_output = std::time::Instant::now();
                    i.log_lines(false);
                }
                drop(i);
                notify.notify_one();
            }
        };
        let done = {
            let (inner, notify) = (self.inner.clone(), self.notify.clone());
            move |r: anyhow::Result<()>| {
                let ended = r.is_ok();
                let lost = r.as_ref().err().is_some_and(|e| e.is::<ssh::Lost>());
                let why = match r {
                    Ok(()) => String::new(),
                    Err(e) => format!("\r\n\x1b[31m{e:#}\x1b[0m"),
                };
                let mut i = inner.lock().unwrap();
                if i.generation == generation {
                    i.ended = ended;
                    i.lost = lost;
                    // The keys are handled by the UI (Return, Esc, Backspace, Ctrl+C, S) and by
                    // key() below (R).
                    i.term.advance_bytes(format!(
                        "{why}\r\n\r\n\x1b[31mSession stopped\x1b[0m\r\n\
                         \x20   - Press \x1b[35m<Return>\x1b[0m, \x1b[35m<Esc>\x1b[0m, \x1b[35m<Backspace>\x1b[0m or \x1b[35m<Ctrl+C>\x1b[0m to exit tab\r\n\
                         \x20   - Press \x1b[1mR\x1b[0m to restart session\r\n\
                         \x20   - Press \x1b[1mS\x1b[0m to save terminal output to file\r\n"
                    ));
                    i.alive = false;
                    i.log_lines(true);
                }
                drop(i);
                notify.notify_one();
            }
        };

        let style_term = self.session.style.term.clone();
        let size = match self.inner.lock().unwrap().size {
            (0, _) | (_, 0) => (80, 24),
            (c, r) => (c as u16, r as u16),
        };
        match self.session.kind.clone() {
            Kind::Ssh(s) => {
                let (prompt, link) = (self.prompt.clone(), self.link.clone());
                tauri::async_runtime::spawn(
                    async move { done(ssh::shell(&s, &style_term, size, &prompt, link, rx, feed).await) },
                );
            }
            Kind::Shell { program, dir } => {
                std::thread::spawn(move || done(run_pty(program.here(), dir.here(), &style_term, rx, feed)));
            }
            Kind::Agent { tool, args, dir, mode, effort, model, mcp_off, skills_off, env, resume, prompt } => {
                let (argv, env) = crate::automation::agent_launch(&tool, Kind::agent_argv(&tool, &args, &mode, &effort, &model, &resume), &mcp_off, &skills_off, dir.here(), &env, &prompt);
                std::thread::spawn(move || done(run_argv(argv, env, dir.here(), &style_term, rx, feed)));
            }
            Kind::Serial { port, baud } => {
                std::thread::spawn(move || done(run_serial(&port, baud, rx, feed)));
            }
            Kind::Telnet { host, port } => {
                std::thread::spawn(move || done(crate::telnet::run(&host, port, &style_term, size, rx, feed)));
            }
            Kind::Winrm { host, port, user, https, insecure, password, .. } => {
                let prompt = self.prompt.clone();
                std::thread::spawn(move || {
                    let argv = winrm_argv(&host, port, &user, https, insecure);
                    if user.is_empty() {
                        return done(run_argv(argv, vec![], "", &style_term, rx, feed));
                    }
                    let id = format!("winrm:{user}@{host}:{port}");
                    let (pass, saved) = match winrm_password(&id, &user, &host, password, &prompt) {
                        Ok(p) => p,
                        Err(e) => return done(Err(e)),
                    };
                    let r = run_argv(argv, vec![(WINRM_PASSWORD.into(), pass)], "", &style_term, rx, feed);
                    // A saved password the host refuses is forgotten: the next try asks again.
                    if saved && r.as_ref().err().and_then(|e| e.downcast_ref::<Exited>()).is_some_and(|e| e.0 == WINRM_REFUSED) {
                        crate::config::forget(&id);
                    }
                    done(r)
                });
            }
            Kind::Rdp { .. } | Kind::Vnc { .. } | Kind::Browser { .. } | Kind::Kvm { .. } => {
                unreachable!("remote desktops are rdp::RdpTab, vnc::VncTab; websites and consoles are web.rs views")
            }
        }
    }

    /// Start the session again in this tab, on a fresh terminal.
    fn restart(&mut self) {
        let (tx, rx) = unbounded_channel();
        let generation = {
            let mut i = self.inner.lock().unwrap();
            i.log_lines(true);
            if let Some(log) = &mut i.log {
                let _ = log.tx.send("\n[reconnecting]\n\n".into());
            }
            i.reconnecting(tx.clone());
            i.generation += 1;
            i.alive = true;
            i.ended = false;
            i.lost = false;
            (i.scroll, i.sel, i.drag, i.find, i.hover) = (0, None, None, None, None);
            i.resend();
            i.generation
        };
        self.tx = tx;
        self.files = None;
        self.since = std::time::SystemTime::now();
        *self.link.lock().unwrap() = Default::default();
        self.start(rx, generation);
        self.notify.notify_one();
    }

    /// Another window shows the tab from now on: it gets everything again, on `out`.
    pub fn attach(&self, out: Channel) {
        *self.out.lock().unwrap() = out;
        let mut i = self.inner.lock().unwrap();
        i.meta = None;
        i.images.clear();
        drop(i);
        self.notify.notify_one();
    }

    /// Terminal Co-op: from now on the frames go to `sink` as well (None stops it): the window's
    /// bytes, but without the owner's selection (`next_frames`). Setting a sink, and calling this
    /// again for a viewer who joins, sends viewers everything again.
    pub fn share(&self, sink: Option<UnboundedSender<Vec<u8>>>) {
        let mut i = self.inner.lock().unwrap();
        let on = sink.is_some();
        i.set_share(sink);
        drop(i);
        if on {
            self.notify.notify_one();
        }
    }

    /// Settings changed: new colours, font and scrollback apply immediately.
    pub fn set_look(&self, look: Look) {
        let mut i = self.inner.lock().unwrap();
        i.highlighted.clear();
        i.term.set_config(Arc::new(TermConfig(look.palette.clone(), look.scrollback)));
        i.look = look;
        i.resend();
        drop(i);
        self.notify.notify_one();
    }

    pub fn is_ssh(&self) -> bool {
        matches!(self.session.kind, Kind::Ssh(_))
    }


    pub fn info(&self) -> Info {
        let i = self.inner.lock().unwrap();
        Info {
            since: self.since.duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()),
            alive: i.alive,
            cols: i.size.0,
            rows: i.size.1,
            link: self.is_ssh().then(|| self.link.lock().unwrap().clone()),
        }
    }

    pub fn alive(&self) -> bool {
        self.inner.lock().unwrap().alive
    }

    /// The title the program set (empty if none).
    pub fn title(&self) -> String {
        let t = self.inner.lock().unwrap().term.get_title().to_string();
        if t == "wezterm" { String::new() } else { t }
    }

    /// How long since output last arrived.
    pub fn quiet_for(&self) -> std::time::Duration {
        self.inner.lock().unwrap().last_output.elapsed()
    }

    /// The last `lines` lines (screen and scrollback) as plain text, without trailing blanks.
    pub fn text(&self, lines: usize) -> String {
        rows_text(self.inner.lock().unwrap().term.screen(), 0, lines)
    }

    /// The cursor's row, as a stable index (it keeps naming the same line as output scrolls):
    /// automation's `run … until` looks for its pattern only in what comes after it.
    pub fn cursor_row(&self) -> StableRowIndex {
        let i = self.inner.lock().unwrap();
        i.term.screen().visible_row_to_stable_row(i.term.cursor_pos().y)
    }

    /// The text from row `from` on (at most the last `max` lines), without trailing blanks.
    pub fn text_from(&self, from: StableRowIndex, max: usize) -> String {
        let i = self.inner.lock().unwrap();
        let screen = i.term.screen();
        // A row that scrolled out of the scrollback: everything there is.
        rows_text(screen, screen.stable_row_to_phys(from).unwrap_or(0), max)
    }

    /// Size in cells, and the cursor (column, row on screen).
    pub fn grid(&self) -> ((usize, usize), (usize, i64)) {
        let i = self.inner.lock().unwrap();
        let c = i.term.cursor_pos();
        (i.size, (c.x, c.y))
    }

    /// Automation: a click on a cell (column and row on screen, from 0), as the mouse would make
    /// it: to the program if it takes the mouse, else it moves the selection.
    pub fn click_cell(&self, col: usize, row: usize, button: u8) {
        let (cw, ch) = self.inner.lock().unwrap().cell;
        let (x, y) = ((col as f32 + 0.5) * cw, (row as f32 + 0.5) * ch);
        for kind in [MouseKind::Move, MouseKind::Down, MouseKind::Up] {
            let m = Mouse { kind, button, x, y, lines: 0, ctrl: false, alt: false, shift: false, clicks: 1 };
            self.mouse(&m);
        }
    }

    /// The find bar; see `Inner::find`.
    pub fn find(&self, query: &str, case: bool, step: i32) -> (usize, usize) {
        let r = self.inner.lock().unwrap().find(query, case, step);
        self.notify.notify_one();
        r
    }

    pub fn tx(&self) -> &UnboundedSender<Ctl> {
        &self.tx
    }

    /// Type into the terminal (the user's keys, or broadcast from another tab).
    pub fn apply(&self, evs: &[Typed]) {
        let mut i = self.inner.lock().unwrap();
        if !i.alive {
            return;
        }
        i.scroll = 0;
        for e in evs {
            apply(&mut i.term, e);
        }
        drop(i);
        self.notify.notify_one();
    }

    /// Handles local shortcuts (scrollback, copy/paste, reconnect) and returns what to type.
    pub fn key(&mut self, k: &Key) -> Option<Typed> {
        let mut i = self.inner.lock().unwrap();
        if !i.alive {
            if k.key.eq_ignore_ascii_case("r") && !k.ctrl && !k.alt && !k.meta {
                drop(i);
                self.restart();
            }
            return None;
        }
        let ch = |c: &str| k.key.eq_ignore_ascii_case(c);
        if k.shift && matches!(k.key.as_str(), "PageUp" | "PageDown") {
            let rows = i.size.1;
            let max = i.term.screen().scrollback_rows().saturating_sub(rows);
            i.scroll =
                if k.key == "PageUp" { (i.scroll + rows / 2).min(max) } else { i.scroll.saturating_sub(rows / 2) };
            drop(i);
            self.notify.notify_one();
            return None;
        }
        if (k.ctrl && k.shift && ch("c")) || (k.meta && ch("c")) {
            if let Some(s) = selection_text(&i) {
                clipboard(|c| c.set_text(s));
            }
            return None;
        }
        if (k.ctrl && k.shift && ch("v")) || (k.shift && k.key == "Insert") || (k.meta && ch("v")) {
            return i.look.pasted().map(Typed::Paste);
        }
        keycode(k).map(|code| Typed::Key(code, kmods(k.ctrl, k.alt, k.shift)))
    }

    /// The scroll bar was dragged: `back` rows up from the bottom (0 follows the output).
    pub fn scroll_to(&self, back: usize) {
        self.inner.lock().unwrap().scroll = back; // frame() keeps it inside the scrollback
        self.notify.notify_one();
    }

    pub fn resize(&self, cols: usize, rows: usize, cell: (f32, f32)) {
        let mut i = self.inner.lock().unwrap();
        let (cols, rows) = (cols.max(2), rows.max(1));
        i.cell = cell;
        if (cols, rows) == i.size {
            return;
        }
        i.size = (cols, rows);
        let size = terminal_size(&i);
        i.term.resize(size);
        i.highlighted.clear(); // rows rewrap
        drop(i);
        let _ = self.tx.send(Ctl::Resize(cols as u16, rows as u16));
        self.notify.notify_one();
    }

    /// A pointer event; Ctrl+click on a link returns its address for the UI to open.
    pub fn mouse(&self, m: &Mouse) -> Option<String> {
        self.pointer(m, None)
    }

    /// A Co-op viewer's pointer event, in pixels of their cells (`theirs`, width and height): it
    /// reaches the program at the same cell as the owner's would, and scrolls, but never selects
    /// (a viewer's selection is their own, see `pick`) nor reads or writes the owner's clipboard.
    pub fn mouse_remote(&self, m: &Mouse, theirs: (f32, f32)) {
        self.pointer(m, Some(theirs));
    }

    /// A Co-op viewer's selection from `a` to `b` (stable row, column; `block`: the rectangle
    /// between them), widened to whole words (`unit` 1) or lines (2) as a double or triple click
    /// widens one: its ends and its text. This tab's own selection is left alone.
    pub fn pick(&self, a: Cell, b: Cell, block: bool, unit: u8) -> ((Cell, Cell), Option<String>) {
        self.inner.lock().unwrap().pick(a, b, block, unit)
    }

    /// A Co-op viewer's paste of their own clipboard's text: a paste like the owner's own (one
    /// bracketed paste when the program asked for that, trimmed when Settings › Terminal says so),
    /// into this tab alone.
    pub fn paste(&self, text: &str) {
        let s = self.inner.lock().unwrap().look.strip(text.to_owned());
        self.apply(&[Typed::Paste(s)]);
    }

    fn pointer(&self, m: &Mouse, theirs: Option<(f32, f32)>) -> Option<String> {
        let (link, redraw) = self.inner.lock().unwrap().pointer(m, theirs);
        if redraw {
            self.notify.notify_one();
        }
        link
    }
}

impl Inner {
    /// `Tab::pick`.
    fn pick(&self, a: Cell, b: Cell, block: bool, unit: u8) -> ((Cell, Cell), Option<String>) {
        let i = self;
        let screen = i.term.screen();
        let total = screen.scrollback_rows();
        // Inside the rows that exist: whatever a viewer sends, the walk over rows stays short.
        let (first, last) = (screen.phys_to_stable_row_index(0), screen.phys_to_stable_row_index(total.max(1) - 1));
        let cols = screen.physical_cols.max(1);
        let fit = |(r, c): Cell| (r.clamp(first, last), c.min(cols - 1));
        let (mut a, mut b) = (fit(a), fit(b));
        if !block {
            if b < a {
                std::mem::swap(&mut a, &mut b);
            }
            match unit {
                1 => (a, b) = (word_at(&i.term, a).0, word_at(&i.term, b).1),
                2 => (a, b) = (line_at(&i.term, a.0).0, line_at(&i.term, b.0).1),
                _ => {}
            }
        }
        ((a, b), sel_range(Some((a, b)), block).and_then(|r| range_text(&i.term, r)))
    }

    /// `theirs`: a Co-op viewer's cell size, else None. Returns the link a Ctrl+click opens, and
    /// whether the screen needs drawing again.
    fn pointer(&mut self, m: &Mouse, theirs: Option<(f32, f32)>) -> (Option<String>, bool) {
        let remote = theirs.is_some();
        let i = self;
        let (cols, rows) = i.size;
        if cols == 0 {
            return (None, false);
        }
        let (cw, ch) = i.cell;
        let (mx, my) = in_our_cells((m.x, m.y), (cw, ch), theirs);
        let (x, y) = (((mx.max(0.0) / cw) as usize).min(cols - 1), ((my.max(0.0) / ch) as usize).min(rows - 1));
        // Offset inside the cell in pixels (SGR-pixel reports, used by e.g. terminal-browser).
        let (px, py) = ((mx - x as f32 * cw).max(0.0) as isize, (my - y as f32 * ch).max(0.0) as isize);
        let ev = |kind, button| MouseEvent {
            kind,
            x,
            y: y as i64,
            x_pixel_offset: px,
            y_pixel_offset: py,
            button,
            modifiers: kmods(m.ctrl, m.alt, m.shift),
        };
        let button = match m.button {
            0 => MouseButton::Left,
            1 => MouseButton::Middle,
            _ => MouseButton::Right,
        };
        // Shift (and Alt, for a rectangle) selects even when the program captures the mouse.
        let grabbed = i.term.is_mouse_grabbed() && !m.shift && !m.alt;
        let total = i.term.screen().scrollback_rows();
        let max_scroll = total.saturating_sub(rows);
        let stable = i.term.screen().phys_to_stable_row_index(total.saturating_sub(rows + i.scroll) + y);
        // Links (even in programs that take the mouse, and after the session ended): Ctrl shows
        // them, Ctrl+click opens them.
        let link = if m.ctrl && i.drag.is_none() && m.kind != MouseKind::Wheel { link_at(&i.term, (stable, x)) } else { None };
        let span = link.as_ref().map(|l| l.1);
        let hover_changed = span != i.hover;
        i.hover = span;
        if m.kind == MouseKind::Down && m.button == 0 && link.is_some() {
            return (link.map(|l| l.0), true);
        }
        if !i.alive || (m.ctrl && m.kind == MouseKind::Up && link.is_some()) {
            return (None, hover_changed);
        }
        match m.kind {
            MouseKind::Wheel => {
                for _ in 0..m.lines.unsigned_abs() {
                    if grabbed {
                        let b = if m.lines > 0 { MouseButton::WheelUp(1) } else { MouseButton::WheelDown(1) };
                        let _ = i.term.mouse_event(ev(MouseEventKind::Press, b));
                    } else if i.term.is_alt_screen_active() {
                        let k = if m.lines > 0 { KeyCode::UpArrow } else { KeyCode::DownArrow };
                        let _ = i.term.key_down(k, KeyModifiers::NONE);
                    } else if m.lines > 0 {
                        i.scroll = (i.scroll + 1).min(max_scroll);
                    } else {
                        i.scroll = i.scroll.saturating_sub(1);
                    }
                }
            }
            MouseKind::Down if grabbed => {
                let _ = i.term.mouse_event(ev(MouseEventKind::Press, button));
            }
            MouseKind::Up if grabbed => {
                let _ = i.term.mouse_event(ev(MouseEventKind::Release, button));
            }
            MouseKind::Move if grabbed => {
                let _ = i.term.mouse_event(ev(MouseEventKind::Move, MouseButton::None));
            }
            // Everyone in a ring has a selection of their own: a viewer's is kept and drawn by
            // their window (coop.rs `Msg::Copy` reads its text), never this one, nor the clipboard.
            MouseKind::Down | MouseKind::Up | MouseKind::Move if remote => {}
            MouseKind::Down if m.button == 0 && m.clicks >= 2 => {
                let unit = if m.clicks == 2 { word_at(&i.term, (stable, x)) } else { line_at(&i.term, stable) };
                i.sel = Some(unit);
                (i.block, i.drag, i.unit) = (false, Some(true), Some((m.clicks > 2, unit)));
                if i.look.copy_on_select
                    && let Some(s) = selection_text(i)
                {
                    clipboard(|c| c.set_text(s));
                }
            }
            MouseKind::Down if m.button == 0 => {
                i.sel = Some(((stable, x), (stable, x)));
                (i.block, i.drag, i.unit) = (m.alt, Some(false), None);
            }
            MouseKind::Down if m.button == 2 && i.look.right_click_paste => {
                if let Some(s) = i.look.pasted() {
                    let _ = i.term.send_paste(&s);
                }
            }
            MouseKind::Move => {
                if i.drag.is_some()
                    && let Some((line, from)) = i.unit
                {
                    let at = if line { line_at(&i.term, stable) } else { word_at(&i.term, (stable, x)) };
                    i.sel = Some(stretch(from, at));
                } else if i.drag.is_some()
                    && let Some(s) = &mut i.sel
                    && s.1 != (stable, x)
                {
                    s.1 = (stable, x);
                    i.drag = Some(true);
                }
            }
            MouseKind::Up if m.button == 0 => {
                i.unit = None;
                match i.drag.take() {
                    Some(true) if i.look.copy_on_select => {
                        if let Some(s) = selection_text(i) {
                            clipboard(|c| c.set_text(s));
                        }
                    }
                    Some(false) => i.sel = None, // a click clears the selection
                    _ => {}
                }
            }
            _ if hover_changed => {}
            _ => return (None, false),
        }
        (None, true)
    }
}

/// A Co-op viewer's pointer position, measured in pixels of their cells (`theirs`), as pixels of
/// ours: the same cell at our size. Unchanged when they don't say how big their cells are.
fn in_our_cells((x, y): (f32, f32), ours: (f32, f32), theirs: Option<(f32, f32)>) -> (f32, f32) {
    match theirs {
        Some((tw, th)) if tw > 0.0 && th > 0.0 => (x * ours.0 / tw, y * ours.1 / th),
        _ => (x, y),
    }
}

impl Drop for Tab {
    fn drop(&mut self) {
        let _ = self.tx.send(Ctl::Close);
        let mut i = self.inner.lock().unwrap();
        i.closed = true;
        i.log_lines(true);
        i.log = None; // the writer finishes the file
        drop(i);
        self.notify.notify_one();
    }
}

fn terminal_size(i: &Inner) -> TerminalSize {
    let (cols, rows) = if i.size.0 == 0 { (80, 24) } else { i.size };
    TerminalSize {
        rows,
        cols,
        pixel_width: (cols as f32 * i.cell.0) as usize,
        pixel_height: (rows as f32 * i.cell.1) as usize,
        dpi: 96,
    }
}

/// Runs `f` (which works on a locked tab) so a panic in it stays in this tab: caught while the
/// lock is still held, it doesn't poison the tab's mutex. Poisoned, the next lock from a UI
/// command panics on the main thread, which aborts the whole app with every session in it.
fn contained<R>(what: &str, f: impl FnOnce() -> R) -> Option<R> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
        .inspect_err(|_| eprintln!("{what}: recovered from a panic (details above)"))
        .ok()
}

/// Sends a frame whenever the terminal changed, at most every few milliseconds.
async fn frames(inner: Arc<Mutex<Inner>>, notify: Arc<Notify>, out: Arc<Mutex<Channel>>) {
    loop {
        notify.notified().await;
        let msgs = {
            let mut i = inner.lock().unwrap();
            if i.closed {
                return;
            }
            i.next_frames()
        };
        // A window gone (the tab moving to another) drops frames until `attach`, which sends all.
        let out = out.lock().unwrap().clone();
        for m in msgs {
            if out.send(InvokeResponseBody::Raw(m)).is_err() {
                break;
            }
        }
        // Coalesce bursts of output into one frame.
        tokio::time::sleep(std::time::Duration::from_millis(8)).await;
    }
}

impl Inner {
    /// The messages for the window, and for the Co-op sink when the tab is shared. Taken under
    /// the tab's lock, so a sink set just now starts with the full frame `set_share` asks for.
    fn next_frames(&mut self) -> Vec<Vec<u8>> {
        // Co-op viewers see the screen without this side's selection (they draw it from coop.rs
        // `Msg::Selected`, in the owner's ring colour, as they draw everyone's). While
        // there is none and they have what the window has, they get the window's very bytes; a
        // selection appearing starts them off from what they have.
        if self.share.is_some() && self.theirs.is_none() && self.sel.is_some() {
            self.theirs =
                Some(Sent { rows: self.sent.clone(), meta: self.meta.clone(), images: self.images.clone() });
        }
        let msgs = contained("terminal frame", || self.frame(true)).unwrap_or_default();
        // No copy is made without a sink.
        let Some(sink) = self.share.clone() else { return msgs };
        let theirs = match self.theirs.take() {
            None => msgs.clone(),
            Some(mut t) => {
                self.swap_sent(&mut t);
                let theirs = contained("terminal frame", || self.frame(false)).unwrap_or_default();
                self.swap_sent(&mut t);
                let alike = self.sel.is_none() && t.rows == self.sent && t.meta == self.meta && t.images == self.images;
                if !alike {
                    self.theirs = Some(t);
                }
                theirs
            }
        };
        // The owner's selection, for the ring to draw in their colour: told when it changes.
        let mut theirs = theirs;
        let picked = sel_range(self.sel, self.block).map(|r| (r.a, r.b, r.block));
        if self.told != Some(picked) {
            self.told = Some(picked);
            let mut m = vec![MSG_SELECTION];
            if serde_json::to_writer(&mut m, &picked).is_ok() {
                theirs.push(m);
            }
        }
        // A sink nobody reads any more (the ring ended) is dropped.
        if theirs.into_iter().any(|m| sink.send(m).is_err()) {
            (self.share, self.theirs) = (None, None);
        }
        msgs
    }

    fn swap_sent(&mut self, t: &mut Sent) {
        std::mem::swap(&mut self.sent, &mut t.rows);
        std::mem::swap(&mut self.meta, &mut t.meta);
        std::mem::swap(&mut self.images, &mut t.images);
    }

    /// Everything is sent again, to the window and to Co-op viewers.
    fn resend(&mut self) {
        self.meta = None;
        if let Some(t) = &mut self.theirs {
            t.meta = None;
        }
    }

    /// A new sink, or a viewer joining: the viewers get everything again (the window doesn't).
    fn set_share(&mut self, sink: Option<UnboundedSender<Vec<u8>>>) {
        self.theirs = sink.is_some().then(Sent::default);
        self.told = None;
        self.share = sink;
    }

    /// Program output. Scrolled back, the screen keeps showing the same lines (`scroll` counts
    /// from the bottom, so it moves up by what arrived), or jumps to the bottom if set to.
    fn output(&mut self, b: &[u8]) {
        let rows = self.size.1;
        let top = |t: &Terminal, scroll: usize| {
            let s = t.screen();
            s.phys_to_stable_row_index(s.scrollback_rows().saturating_sub(rows + scroll))
        };
        let kept = (self.scroll > 0 && !self.look.scroll_on_output).then(|| top(&self.term, self.scroll));
        contained("terminal input", || self.term.advance_bytes(b));
        if self.look.scroll_on_output {
            self.scroll = 0;
        } else if let Some(row) = kept {
            let s = self.term.screen();
            // A row that rolled out of the scrollback: the oldest one left is on top.
            let phys = s.stable_row_to_phys(row).unwrap_or(0);
            self.scroll = s.scrollback_rows().saturating_sub(rows).saturating_sub(phys);
        }
    }

    /// Messages bringing the UI up to date: new images first, then the frame. Empty if nothing changed.
    /// `own`: with this side's selection (the window), or without it (Co-op viewers).
    fn frame(&mut self, own: bool) -> Vec<Vec<u8>> {
        let (cols, rows) = self.size;
        if cols == 0 {
            // Not sized yet: the UI needs the font to work out the grid.
            let frame = Frame { rows: 0, scroll: 0, lines: vec![], meta: None, style: Some(self.style()) };
            return encode(&frame).into_iter().collect();
        }
        let total = self.term.screen().scrollback_rows();
        self.scroll = self.scroll.min(total.saturating_sub(rows));
        let start = total.saturating_sub(rows + self.scroll);
        let pal = self.term.palette();
        let default_bg = rgb32(pal.background);
        let sel = if own { sel_range(self.sel, self.block) } else { None };
        let sel_fg = rgb32(if pal.selection_fg.3 == 0.0 { pal.foreground } else { pal.selection_fg });
        let sel_bg = rgb32(pal.selection_bg);
        let screen = self.term.screen();
        // Find-bar matches on these rows, and whether each is the current one.
        let (first, last) = (screen.phys_to_stable_row_index(start), screen.phys_to_stable_row_index(total.max(1) - 1));
        let hits: Vec<((Cell, Cell), bool)> = self.find.as_ref().map_or(vec![], |f| {
            f.hits
                .iter()
                .enumerate()
                .filter(|(_, (a, b))| b.0 >= first && a.0 <= last)
                .map(|(n, h)| (*h, f.current == Some(n)))
                .collect()
        });
        let mut lines: Vec<Vec<Run>> = Vec::with_capacity(rows);
        // One placement per image (not per cell): a placement's visible cells form a rectangle,
        // and so do their texture coordinates.
        // (hash, image id, placement id), destination, texture coords, data.
        type Placed = (([u8; 32], Option<u32>, Option<u32>), [f32; 4], [f32; 4], Arc<ImageData>);
        let mut placed: Vec<Placed> = vec![];

        // Not `with_phys_lines`: it mis-indexes once the scrollback ring wraps (more lines than
        // the scrollback holds) and panics. This copies the visible rows, which is cheap.
        let phys = screen.lines_in_phys_range(start..total);
        let default_fg = rgb32(pal.foreground);
        let rules = self.look.highlights.clone();
        let mut cached = std::mem::take(&mut self.highlighted);
        for (r, line) in phys.iter().enumerate() {
            let stable = screen.phys_to_stable_row_index(start + r);
            let mut runs: Vec<Run> = vec![];
            let spans = if rules.is_empty() {
                &vec![]
            } else {
                let seqno = line.current_seqno();
                let hit = cached.remove(&stable).filter(|(s, _)| *s == seqno);
                let spans = hit.map_or_else(|| highlight_spans(line, &rules), |(_, spans)| spans);
                &self.highlighted.entry(stable).insert_entry((seqno, spans)).into_mut().1
            };
            for cell in line.visible_cells() {
                let x = cell.cell_index();
                let attrs = cell.attrs();
                let (mut fg, mut flags) = fg_color(attrs, &pal);
                // Highlight rules colour text the program left in the default colour.
                if fg == default_fg
                    && !attrs.reverse()
                    && let Some((.., c)) = spans.iter().find(|(a, b, _)| (*a..*b).contains(&x))
                {
                    fg = *c;
                }
                let mut bg = rgb32(pal.resolve_bg(attrs.background()));
                if attrs.reverse() {
                    std::mem::swap(&mut fg, &mut bg);
                }
                if let Some((_, now)) = hits.iter().find(|((a, b), _)| (stable, x) >= *a && (stable, x) <= *b) {
                    fg = HIT_FG;
                    bg = if *now { HIT_NOW_BG } else { HIT_BG };
                }
                if sel.is_some_and(|s| s.has(stable, x)) {
                    let sfg = pal.selection_fg;
                    fg = rgb32(if sfg.3 == 0.0 { pal.foreground } else { sfg });
                    bg = rgb32(pal.selection_bg);
                }
                if attrs.underline() != Underline::None
                    || self.hover.is_some_and(|(a, b)| (stable, x) >= a && (stable, x) <= b)
                {
                    flags |= UNDERLINE;
                }
                if attrs.strikethrough() {
                    flags |= STRIKE;
                }
                if attrs.italic() {
                    flags |= ITALIC;
                }
                let w = cell.width();
                if let Some(imgs) = attrs.images() {
                    for img in imgs {
                        let (tl, br) = (img.top_left(), img.bottom_right());
                        let uv = [*tl.x, *tl.y, *br.x, *br.y];
                        let dst = [x as f32, r as f32, (x + w) as f32, r as f32 + 1.0];
                        let key = (img.image_data().hash(), img.image_id(), img.placement_id());
                        match placed.iter_mut().find(|(k, ..)| *k == key) {
                            Some((_, d, u, _)) => {
                                *d = union(*d, dst);
                                *u = union(*u, uv);
                            }
                            None => placed.push((key, dst, uv, img.image_data().clone())),
                        }
                    }
                }
                let bg = (bg != default_bg).then_some(bg);
                let s = cell.str();
                let blank = attrs.invisible() || s.trim().is_empty();
                if blank && bg.is_none() && flags & (UNDERLINE | STRIKE) == 0 {
                    continue;
                }
                let t = if blank { " " } else { s };
                if t.is_ascii()
                    && w == 1
                    && let Some(last) = runs.last_mut()
                    && last.2.is_ascii()
                    && (last.0 + last.1) as usize == x
                    && (last.3, last.4, last.5) == (fg, bg, flags)
                {
                    last.1 += 1;
                    last.2.push_str(t);
                    continue;
                }
                runs.push(Run(x as u16, w as u16, t.to_string(), fg, bg, flags));
            }
            // A selected row is lit to its right edge, past the last cell the program wrote (the
            // blanks and the line break are part of what is selected), not only under its text.
            if let Some(s) = sel {
                let from = runs.last().map_or(0, |r| (r.0 + r.1) as usize).max(s.row_start(stable));
                let to = s.row_end(stable, cols);
                if to > from {
                    runs.push(Run(from as u16, (to - from) as u16, " ".repeat(to - from), sel_fg, Some(sel_bg), 0));
                }
            }
            lines.push(runs);
        }
        lines.resize(rows, vec![]);

        let cur = self.term.cursor_pos();
        let title = self.term.get_title();
        let meta = Meta {
            cursor: (self.scroll == 0 && cur.visibility == CursorVisibility::Visible && cur.y >= 0)
                .then_some((cur.x, cur.y as usize)),
            title: if title == "wezterm" { String::new() } else { title.to_string() },
            alive: self.alive,
            ended: self.ended,
            lost: self.lost,
            cwd: self.term.get_current_dir().map(|u| percent_decode(u.path())).unwrap_or_default(),
            link: self.hover.is_some(),
            scrolled: self.scroll > 0,
            scrollback: (self.scroll, total.saturating_sub(rows)),
            images: placed.iter().map(|(k, d, u, _)| Placement { hash: hex(&k.0), dst: *d, uv: *u }).collect(),
            shared: self.share.as_ref().map(|_| (first, self.term.is_mouse_grabbed())),
        };

        let full = self.meta.is_none() || self.sent.len() != rows;
        let mut msgs = vec![];
        for (_, _, _, data) in &placed {
            if self.images.insert(data.hash())
                && let Some(m) = image_msg(data)
            {
                msgs.push(m);
            }
        }
        // Forget images that left the screen: the UI drops them too, and gets them again if they return.
        self.images.retain(|h| placed.iter().any(|(k, ..)| k.0 == *h));

        // The UI moves what it has by `scroll` rows, then takes only the rows that still differ.
        let scroll = if full { 0 } else { scrolled(&self.sent, &lines) };
        let changed: Vec<(usize, &Vec<Run>)> = lines
            .iter()
            .enumerate()
            .filter(|(y, l)| full || self.sent.get((*y as i32 + scroll) as usize) != Some(l))
            .collect();
        let new_meta = self.meta.as_ref() != Some(&meta);
        if changed.is_empty() && !new_meta {
            return msgs;
        }
        let frame =
            Frame { rows, scroll, lines: changed, meta: new_meta.then_some(&meta), style: full.then(|| self.style()) };
        msgs.extend(encode(&frame));
        self.sent = lines;
        self.meta = Some(meta);
        msgs
    }
}

impl Inner {
    fn style(&self) -> StyleMsg<'_> {
        // The terminal's palette: programs may have changed colours with OSC sequences.
        let (look, pal) = (&self.look, self.term.palette());
        StyleMsg {
            font: &look.font,
            size: look.font_size,
            cursor: look.cursor,
            blink: look.cursor_blink,
            line: look.line_height,
            fg: rgb32(pal.foreground),
            bg: rgb32(pal.background),
            cc: rgb32(pal.cursor_bg),
        }
    }
}

fn encode(frame: &Frame) -> Option<Vec<u8>> {
    let mut m = vec![MSG_FRAME];
    serde_json::to_writer(&mut m, frame).ok()?;
    Some(m)
}

fn is_zero(n: &i32) -> bool {
    *n == 0
}

/// How far the screen scrolled since `old` (up is positive): the shift that keeps the most
/// non-blank rows. Scroll regions (a status bar below) still count; the bar's row is resent.
fn scrolled(old: &[Vec<Run>], new: &[Vec<Run>]) -> i32 {
    let n = new.len() as i32;
    let same = |k: i32| {
        (0..n)
            .filter(|&y| {
                let row = &new[y as usize];
                !row.is_empty() && old.get((y + k) as usize) == Some(row)
            })
            .count() as i32
    };
    let mut best = (same(0), 0);
    for k in 1..n {
        // A shift of k keeps at most n - k rows.
        if n - k <= best.0 {
            break;
        }
        for k in [k, -k] {
            let s = same(k);
            if s > best.0 {
                best = (s, k);
            }
        }
    }
    best.1
}

fn union(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])]
}

fn hex(h: &[u8; 32]) -> String {
    h.iter().map(|b| format!("{b:02x}")).collect()
}

/// `[MSG_IMAGE][hash: 32][format][width: u32 LE][height: u32 LE][data]`, by format:
/// - 0: RGBA pixels.
/// - 1: an encoded file (PNG, JPEG, GIF, ...) as it is; the UI decodes it, animation included.
/// - 2: an animation: `[frames: u32][duration ms: u32 per frame][RGBA per frame]`.
///
/// ponytail: animations go as raw frames (width x height x 4 each); fine for terminal-sized
/// images, a long full-screen animation is many MB. Encode to a format the UI decodes if that bites.
fn image_msg(data: &ImageData) -> Option<Vec<u8>> {
    let (format, w, h, bytes): (u8, u32, u32, Vec<u8>) = match &*data.data() {
        ImageDataType::Rgba8 { data, width, height, .. } => (0, *width, *height, data.clone()),
        ImageDataType::AnimRgba8 { frames, width, height, .. } if frames.len() == 1 => {
            (0, *width, *height, frames[0].clone())
        }
        ImageDataType::AnimRgba8 { frames, durations, width, height, .. } => {
            let mut b = Vec::with_capacity(4 + 4 * frames.len() + frames.iter().map(Vec::len).sum::<usize>());
            b.extend_from_slice(&(frames.len() as u32).to_le_bytes());
            for d in durations.iter().chain(std::iter::repeat(&std::time::Duration::ZERO)).take(frames.len()) {
                b.extend_from_slice(&(d.as_millis() as u32).to_le_bytes());
            }
            frames.iter().for_each(|f| b.extend_from_slice(f));
            (2, *width, *height, b)
        }
        ImageDataType::EncodedFile(bytes) => (1, 0, 0, bytes.clone()),
        ImageDataType::EncodedLease(lease) => (1, 0, 0, lease.get_data().ok()?),
    };
    let mut m = Vec::with_capacity(42 + bytes.len());
    m.push(MSG_IMAGE);
    m.extend_from_slice(&data.hash());
    m.push(format);
    m.extend_from_slice(&w.to_le_bytes());
    m.extend_from_slice(&h.to_le_bytes());
    m.extend_from_slice(&bytes);
    Some(m)
}

/// The cells of one row as text, one string per column (empty for the right half of a wide character).
fn row_cells(term: &Terminal, row: StableRowIndex) -> Vec<String> {
    let screen = term.screen();
    let Some(phys) = screen.stable_row_to_phys(row) else { return vec![] };
    let line = &screen.lines_in_phys_range(phys..phys + 1)[0];
    let mut cells = vec![" ".to_string(); screen.physical_cols];
    for c in line.visible_cells() {
        let at = c.cell_index();
        for (k, cell) in cells.iter_mut().enumerate().skip(at).take(c.width().max(1)) {
            *cell = if k == at { c.str().to_string() } else { String::new() };
        }
    }
    cells
}

/// Double click: the run of word characters around `at`. Paths, hosts and addresses stay whole
/// (`.-_/:@~` count as word characters); a click on a blank or a symbol selects just it. A word
/// the terminal wrapped onto the next row is taken whole.
fn word_at(term: &Terminal, (row, col): Cell) -> (Cell, Cell) {
    let screen = term.screen();
    let wrapped = |r: StableRowIndex| {
        screen.stable_row_to_phys(r).is_some_and(|p| screen.lines_in_phys_range(p..p + 1)[0].last_cell_was_wrapped())
    };
    // "" is a wide character's second cell.
    let word = |s: &String| s.is_empty() || s.chars().all(|c| c.is_alphanumeric() || ".-_/:@~+%=#".contains(c));
    let mut cells = row_cells(term, row);
    if !cells.get(col).is_some_and(word) {
        return ((row, col), (row, col));
    }
    let (mut a, mut b) = ((row, col), (row, col));
    let here = cells.clone();
    loop {
        if a.1 > 0 && word(&cells[a.1 - 1]) {
            a.1 -= 1;
        } else if a.1 == 0 && wrapped(a.0 - 1) && row_cells(term, a.0 - 1).last().is_some_and(word) {
            cells = row_cells(term, a.0 - 1);
            a = (a.0 - 1, cells.len() - 1);
        } else {
            break;
        }
    }
    cells = here;
    loop {
        if b.1 + 1 < cells.len() && word(&cells[b.1 + 1]) {
            b.1 += 1;
        } else if b.1 + 1 == cells.len() && wrapped(b.0) && row_cells(term, b.0 + 1).first().is_some_and(word) {
            cells = row_cells(term, b.0 + 1);
            b = (b.0 + 1, 0);
        } else {
            break;
        }
    }
    (a, b)
}

/// A double or triple click's word or line `from`, stretched to take in the one `at` the pointer
/// is dragged to, before or after it.
fn stretch(from: (Cell, Cell), at: (Cell, Cell)) -> (Cell, Cell) {
    (from.0.min(at.0), from.1.max(at.1))
}

/// Triple click: the whole line, including the rows it wrapped onto.
fn line_at(term: &Terminal, row: StableRowIndex) -> (Cell, Cell) {
    let screen = term.screen();
    let wrapped = |r: StableRowIndex| {
        screen.stable_row_to_phys(r).is_some_and(|p| screen.lines_in_phys_range(p..p + 1)[0].last_cell_was_wrapped())
    };
    let (mut a, mut b) = (row, row);
    while wrapped(a - 1) {
        a -= 1;
    }
    while wrapped(b) {
        b += 1;
    }
    ((a, 0), (b, screen.physical_cols.saturating_sub(1)))
}

/// A selection, ready to test cells against: the text between two cells in reading order, or
/// (`block`) the rectangle of rows `a.0..=b.0` and columns `a.1..=b.1`.
#[derive(Clone, Copy)]
struct SelRange {
    a: Cell,
    b: Cell,
    block: bool,
}

impl SelRange {
    #[inline]
    fn has(&self, row: StableRowIndex, col: usize) -> bool {
        if self.block {
            row >= self.a.0 && row <= self.b.0 && col >= self.a.1 && col <= self.b.1
        } else {
            (row, col) >= self.a && (row, col) <= self.b
        }
    }

    /// The first selected column of `row` (0 when the row is wholly inside, or not selected).
    fn row_start(&self, row: StableRowIndex) -> usize {
        if self.block || row == self.a.0 { self.a.1 } else { 0 }
    }

    /// One past the last selected column of `row` on a `cols`-wide screen: the right edge for a
    /// row inside the selection, the end cell on its last row, 0 for a row outside it.
    fn row_end(&self, row: StableRowIndex, cols: usize) -> usize {
        if row < self.a.0 || row > self.b.0 {
            0
        } else if self.block || row == self.b.0 {
            (self.b.1 + 1).min(cols)
        } else {
            cols
        }
    }
}

fn sel_range(sel: Option<(Cell, Cell)>, block: bool) -> Option<SelRange> {
    let (a, b) = sel?;
    Some(if block {
        SelRange { a: (a.0.min(b.0), a.1.min(b.1)), b: (a.0.max(b.0), a.1.max(b.1)), block }
    } else if a <= b {
        SelRange { a, b, block }
    } else {
        SelRange { a: b, b: a, block }
    })
}

fn selection_text(i: &Inner) -> Option<String> {
    range_text(&i.term, sel_range(i.sel, i.block)?)
}

/// The text of a range of cells: wrapped rows joined, trailing blanks dropped.
fn range_text(term: &Terminal, r: SelRange) -> Option<String> {
    let SelRange { a: (r0, c0), b: (r1, c1), block } = r;
    let screen = term.screen();
    let mut s = String::new();
    if block {
        // A rectangle: the same columns of every row, one line each.
        let rows: Vec<String> = (r0..=r1)
            .filter_map(|row| screen.stable_row_to_phys(row))
            .map(|phys| screen.lines_in_phys_range(phys..phys + 1)[0].columns_as_str(c0..c1 + 1).trim_end().to_string())
            .collect();
        s = rows.join("\n");
        return (!s.trim().is_empty()).then_some(s);
    }
    for row in r0..=r1 {
        let Some(phys) = screen.stable_row_to_phys(row) else { continue };
        let line = &screen.lines_in_phys_range(phys..phys + 1)[0];
        let from = if row == r0 { c0 } else { 0 };
        let to = if row == r1 { c1 + 1 } else { line.len() };
        s.push_str(line.columns_as_str(from..to.max(from)).trim_end());
        if row != r1 && !line.last_cell_was_wrapped() {
            s.push('\n');
        }
    }
    (!s.is_empty()).then_some(s)
}

fn kmods(ctrl: bool, alt: bool, shift: bool) -> KeyModifiers {
    let mut r = KeyModifiers::NONE;
    if shift {
        r |= KeyModifiers::SHIFT;
    }
    if ctrl {
        r |= KeyModifiers::CTRL;
    }
    if alt {
        r |= KeyModifiers::ALT;
    }
    r
}

/// Keys the UI sends as key presses rather than text: specials, and Ctrl/Alt chords.
fn keycode(k: &Key) -> Option<KeyCode> {
    Some(match k.key.as_str() {
        "Enter" => KeyCode::Enter,
        "Tab" => KeyCode::Tab,
        "Backspace" => KeyCode::Backspace,
        "Escape" => KeyCode::Escape,
        "ArrowUp" => KeyCode::UpArrow,
        "ArrowDown" => KeyCode::DownArrow,
        "ArrowLeft" => KeyCode::LeftArrow,
        "ArrowRight" => KeyCode::RightArrow,
        "Home" => KeyCode::Home,
        "End" => KeyCode::End,
        "PageUp" => KeyCode::PageUp,
        "PageDown" => KeyCode::PageDown,
        "Insert" => KeyCode::Insert,
        "Delete" => KeyCode::Delete,
        f if f.len() > 1 && f.starts_with('F') && f[1..].parse::<u8>().is_ok_and(|n| (1..=24).contains(&n)) => {
            KeyCode::Function(f[1..].parse().ok()?)
        }
        " " if k.ctrl => KeyCode::Char(' '),
        // Ctrl+Alt is AltGr on Windows layouts: that arrives as text, so don't double-send.
        s if (k.ctrl || k.alt) && !(k.ctrl && k.alt) => {
            let mut chars = s.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => KeyCode::Char(c.to_ascii_lowercase()),
                _ => return None,
            }
        }
        _ => return None,
    })
}

fn fg_color(attrs: &CellAttributes, pal: &ColorPalette) -> (u32, u8) {
    let bold = attrs.intensity() == Intensity::Bold;
    let c = match attrs.foreground() {
        // Bold also brightens the 8 base colours, like most terminals.
        ColorAttribute::PaletteIndex(i) if i < 8 && bold => pal.colors.0[i as usize + 8],
        c => pal.resolve_fg(c),
    };
    let flags = match attrs.intensity() {
        Intensity::Bold => BOLD,
        Intensity::Half => DIM,
        Intensity::Normal => 0,
    };
    (rgb32(c), flags)
}

pub(crate) fn rgb32(c: SrgbaTuple) -> u32 {
    let (r, g, b, _) = c.to_srgb_u8();
    (r as u32) << 16 | (g as u32) << 8 | b as u32
}

fn srgb([r, g, b]: [u8; 3]) -> SrgbaTuple {
    SrgbaTuple(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0)
}

/// Palette from a built-in scheme.
pub fn scheme_palette(sc: &Scheme) -> ColorPalette {
    let mut p = ColorPalette::default();
    p.foreground = srgb(rgb(sc.fg));
    p.background = srgb(rgb(sc.bg));
    p.cursor_bg = srgb(rgb(sc.cursor));
    p.cursor_border = p.cursor_bg;
    p.cursor_fg = p.background;
    p.selection_bg = srgb(rgb(sc.selection));
    for (i, c) in sc.ansi.iter().enumerate() {
        p.colors.0[i] = srgb(rgb(*c));
    }
    p
}

/// Palette from a session's own style, on top of a base (the current scheme).
pub fn palette(s: &Style, base: ColorPalette) -> ColorPalette {
    let mut p = base;
    let c = srgb;
    if let Some(v) = s.fg {
        p.foreground = c(v);
    }
    if let Some(v) = s.bg {
        p.background = c(v);
    }
    if let Some(v) = s.cursor {
        p.cursor_bg = c(v);
        p.cursor_border = c(v);
    }
    for (i, v) in s.palette.iter().flatten().take(16).enumerate() {
        p.colors.0[i] = c(*v);
    }
    p
}

/// The `code` command's arguments that open a terminal's folder in VS Code. An SSH session's
/// `cwd` (absolute, on the host) opens there through the Remote-SSH extension, as the session's
/// user, else `user` (Settings › Defaults). A local tab's opens as it is, or when the shell doesn't
/// say (or names a folder that isn't here), the tab's starting folder or your home.
pub fn code_args(kind: &Kind, cwd: &str, user: &str) -> Result<Vec<String>, String> {
    match kind {
        Kind::Ssh(s) => {
            let t = &s.target;
            let user = if t.user.is_empty() { user } else { &t.user };
            let path: String = cwd
                .bytes()
                .map(|b| if b.is_ascii_alphanumeric() || b"/-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
                .collect();
            Ok(vec!["--folder-uri".into(), format!("vscode-remote://ssh-remote+{}{path}", remote_ssh_host(&t.host, user, t.port))])
        }
        Kind::Shell { dir, .. } | Kind::Agent { dir, .. } => {
            // OSC 7 hands over a URL's path: "/C:/Users/…" on Windows.
            let cwd = cwd.strip_prefix('/').filter(|p| cfg!(windows) && p.get(1..2) == Some(":")).unwrap_or(cwd);
            let home = directories::UserDirs::new().map(|d| d.home_dir().display().to_string()).unwrap_or_default();
            let dir = crate::paths::expand(dir.here()).display().to_string();
            let folder = [cwd, &dir, &home].into_iter().find(|d| !d.is_empty() && std::path::Path::new(d).is_dir());
            Ok(vec![folder.ok_or("No folder to open.")?.to_string()])
        }
        _ => Err("Only SSH sessions and local shells open in VS Code.".into()),
    }
}

/// Remote-SSH's name for a host, made as the extension makes it (`HostInfo.toAuthorityString`,
/// 0.128): the host itself, or with a user, a port or capitals, `{"hostName","user","port"}` as
/// JSON in hex.
fn remote_ssh_host(host: &str, user: &str, port: u16) -> String {
    if user.is_empty() && port == 22 && host == host.to_lowercase() && !host.contains(['/', '\\', '+']) {
        return host.into();
    }
    #[derive(Serialize)]
    struct HostInfo<'a> {
        #[serde(rename = "hostName")]
        host: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        user: Option<&'a str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        port: Option<u16>,
    }
    let info = HostInfo { host, user: Some(user).filter(|u| !u.is_empty()), port: Some(port).filter(|p| *p != 22) };
    serde_json::to_vec(&info).unwrap_or_default().iter().map(|b| format!("{b:02x}")).collect()
}

/// The environment variable that hands a WinRM session's password to `winrm_argv`'s PowerShell,
/// which takes it out at once: a command line is there for other programs to read.
const WINRM_PASSWORD: &str = "XTERMIUM_WINRM_PASSWORD";
/// `winrm_argv`'s exit code when the host refused the login (access denied, wrong password).
const WINRM_REFUSED: u32 = 2;

/// The password a WinRM session with a user signs in with: its shared login's, else a saved one,
/// else asked for (and saved when you tick Remember). Also whether it's a saved one, so one the
/// host refuses can be forgotten.
fn winrm_password(id: &str, user: &str, host: &str, shared: Option<String>, prompt: &Prompter) -> anyhow::Result<(String, bool)> {
    if let Some(p) = shared {
        return Ok((p, false));
    }
    if let Some(p) = crate::config::login(id) {
        return Ok((p, true));
    }
    let asked = tauri::async_runtime::block_on(prompt.ask("Password", format!("Password for {user}@{host}:"), true));
    let (p, remember) = asked.ok_or_else(|| anyhow::anyhow!("cancelled"))?;
    if remember {
        let _ = crate::config::remember(id, &p);
    }
    Ok((p, remember))
}

/// PowerShell remoting: the local PowerShell (7 if installed) enters a session on the host, as
/// `user` with the password in `WINRM_PASSWORD`, or as you (the Windows login) without a user;
/// leaving it (`exit`) returns to a local prompt (no profile).
fn winrm_argv(host: &str, port: u16, user: &str, https: bool, insecure: bool) -> Vec<String> {
    // A PowerShell string literal: its quote characters, typographic ones too, are doubled.
    let quote = |s: &str| {
        let q: String = s.chars().flat_map(|c| if "'\u{2018}\u{2019}\u{201A}\u{201B}".contains(c) { vec![c, c] } else { vec![c] }).collect();
        format!("'{q}'")
    };
    let mut login = String::new();
    let mut cmd = format!("Enter-PSSession -ComputerName {} -Port {port}", quote(host));
    if !user.is_empty() {
        login = format!(
            "$c = [pscredential]::new({}, [Net.NetworkCredential]::new('', $env:{WINRM_PASSWORD}).SecurePassword); Remove-Item Env:{WINRM_PASSWORD}; ",
            quote(user)
        );
        cmd += " -Credential $c";
    }
    if https {
        cmd += " -UseSSL";
        if insecure {
            cmd += " -SessionOption (New-PSSessionOption -SkipCACheck -SkipCNCheck -SkipRevocationCheck)";
        }
    }
    let pwsh = !cfg!(windows)
        || std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join("pwsh.exe").is_file()));
    // Not connected: the tab says the session stopped, rather than leaving a local prompt.
    // ErrorCode 5 is access denied, 1326 a wrong user name or password.
    let cmd = format!(
        "try {{ {login}{cmd} -ErrorAction Stop }} catch {{ $_; if ($_.Exception.ErrorCode -in 5, 1326) {{ exit {WINRM_REFUSED} }}; exit 1 }}"
    );
    [if pwsh { "pwsh" } else { "powershell" }, "-NoLogo", "-NoProfile", "-NoExit", "-Command", &cmd].map(String::from).to_vec()
}

/// Runs `program` (its words split on spaces) in folder `dir`, "" being the home folder.
fn run_pty(
    program: &str,
    dir: &str,
    term: &str,
    rx: UnboundedReceiver<Ctl>,
    feed: impl FnMut(&[u8]),
) -> anyhow::Result<()> {
    run_argv(program.split_whitespace().map(String::from).collect(), vec![], dir, term, rx, feed)
}

/// Runs a program (`argv` empty: the default shell) with extra environment variables.
fn run_argv(
    argv: Vec<String>,
    env: Vec<(String, String)>,
    dir: &str,
    term: &str,
    rx: UnboundedReceiver<Ctl>,
    feed: impl FnMut(&[u8]),
) -> anyhow::Result<()> {
    // `~` and `~/…` are the home folder, in the working folder and in the program's own path.
    let folder = crate::paths::expand(dir);
    if !dir.is_empty() && !folder.is_dir() {
        anyhow::bail!("the working folder {dir} doesn't exist");
    }
    let mut argv = argv;
    if let Some(p) = argv.first_mut().filter(|p| p.starts_with('~')) {
        *p = crate::paths::expand(p).display().to_string();
    }
    // The PATH the program will run with: the session's own if it sets one, else the app's.
    let path = env.iter().find(|(k, _)| k.eq_ignore_ascii_case("PATH")).map(|(_, v)| v.into()).or_else(|| std::env::var_os("PATH")).unwrap_or_default();
    let (argv, beside) = through_cmd(argv, &path);
    let mut parts = argv.iter();
    let name = parts.next();
    let mut cmd = match name {
        Some(p) => portable_pty::CommandBuilder::new(p),
        None => portable_pty::CommandBuilder::new_default_prog(),
    };
    cmd.args(parts);
    // The shell's environment first (the PATH the program is looked up on included), then the
    // session's own, so it wins.
    for (k, v) in crate::support::shell_env() {
        cmd.env(k, v);
    }
    for (k, v) in env {
        cmd.env(k, v);
    }
    // cmd looks the `.cmd` up again by its name: its folder goes first on the PATH, plainly, so
    // cmd finds the very file found here, also when the PATH names that folder in quotes (cmd
    // doesn't read such an entry).
    if let Some(dir) = beside
        && let Ok(joined) = std::env::join_paths(std::iter::once(dir).chain(std::env::split_paths(&path)))
    {
        cmd.env("PATH", joined);
    }
    if !dir.is_empty() {
        cmd.cwd(folder);
    } else if let Some(d) = directories::UserDirs::new() {
        cmd.cwd(d.home_dir());
    }
    cmd.env("TERM", term);
    // A file of the program's exact name with no `.exe` or `.cmd` beside it (a script for sh, as
    // npm installs next to its `.cmd`): Windows can't start it (os error 193).
    let script = |e: &anyhow::Error| cfg!(windows) && e.to_string().contains("(os error 193)");
    // Windows says "CreateProcessW … failed: The system cannot find the file specified. (os error 2)".
    let missing = |e: &anyhow::Error| {
        e.to_string().contains("(os error 2)")
            || e.downcast_ref::<std::io::Error>().is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound)
    };
    run_cmd(cmd, rx, feed).map_err(|e| match name {
        Some(p) if missing(&e) => anyhow::anyhow!("{p} wasn't found: install it, or check that it's on your PATH"),
        Some(p) if script(&e) => anyhow::anyhow!("{p} on your PATH is a script Windows can't start: there is no {p}.exe or {p}.cmd beside it. Install it again, or give its full path as a custom command"),
        _ => e,
    })
}

/// On Windows, finds a program named without a path on `path` the way the system does, so the
/// pty library doesn't: that one takes a file of the exact name first, and npm puts a script
/// for sh named `claude` beside `claude.cmd` ("not a valid Win32 application"). The first folder
/// with an `.exe` or `.com` of the name gives that file's full path. The first with a `.cmd` or
/// `.bat` (what npm installs, which only cmd.exe runs) gives the command through `cmd /c` and
/// that folder, to go first on the program's PATH (`run_argv`). Folders are read as
/// `split_paths` reads them, one in quotes too. Anything else, and every other system, runs as
/// it is.
fn through_cmd(mut argv: Vec<String>, path: &std::ffi::OsStr) -> (Vec<String>, Option<std::path::PathBuf>) {
    if !cfg!(windows) {
        return (argv, None);
    }
    let Some(name) = argv.first().filter(|n| !n.contains(['\\', '/', '.'])).cloned() else { return (argv, None) };
    for dir in std::env::split_paths(path).filter(|d| !d.as_os_str().is_empty()) {
        let found = |ext: &str| Some(dir.join(format!("{name}{ext}"))).filter(|f| f.is_file());
        if let Some(exe) = found(".exe").or_else(|| found(".com")) {
            argv[0] = exe.display().to_string();
            return (argv, None);
        }
        if found(".cmd").or_else(|| found(".bat")).is_some() {
            return (["cmd".to_string(), "/c".into()].into_iter().chain(argv).collect(), Some(dir));
        }
    }
    (argv, None)
}

/// Console programs started from the GUI must not flash a console window.
#[cfg(windows)]
pub fn no_window(cmd: &mut std::process::Command) -> &mut std::process::Command {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(0x0800_0000) // CREATE_NO_WINDOW
}

fn run_cmd(
    cmd: portable_pty::CommandBuilder,
    mut rx: UnboundedReceiver<Ctl>,
    mut feed: impl FnMut(&[u8]),
) -> anyhow::Result<()> {
    use portable_pty::{PtySize, native_pty_system};
    let pair = native_pty_system().openpty(PtySize { rows: 24, cols: 80, pixel_width: 0, pixel_height: 0 })?;
    let mut child = pair.slave.spawn_command(cmd)?;
    drop(pair.slave);
    let mut killer = child.clone_killer();
    let mut reader = pair.master.try_clone_reader()?;
    let mut writer = pair.master.take_writer()?;
    // ConPTY keeps the output open until the pseudo console closes, even after the program
    // exits: drop it once the program is gone, so the reader below sees the end.
    let master = Arc::new(Mutex::new(Some(pair.master)));
    let (exited_tx, exited) = std::sync::mpsc::channel::<()>();
    let waiter = {
        let master = master.clone();
        std::thread::spawn(move || {
            let status = child.wait();
            let _ = exited_tx.send(());
            let m = master.lock().unwrap().take();
            drop(m); // outside the lock: closing a pseudo console can take a while
            status
        })
    };
    // Input gets its own thread: a program that stops reading blocks the write, and a blocked
    // write must not keep the tab from closing.
    let (input, keys) = std::sync::mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        for b in keys {
            if writer.write_all(&b).is_err() {
                break;
            }
        }
    });
    std::thread::spawn(move || {
        while let Some(c) = rx.blocking_recv() {
            match c {
                Ctl::Input(b) => {
                    let _ = input.send(b);
                }
                Ctl::Resize(cols, rows) => {
                    if let Some(m) = &*master.lock().unwrap() {
                        let _ = m.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
                    }
                }
                Ctl::Sftp(_) | Ctl::Cwd(_) => {}
                Ctl::Close => break,
            }
        }
        // The tab closed. Gracefully first: closing the console hangs the program up
        // (CTRL_CLOSE_EVENT on Windows, SIGHUP elsewhere). Forcefully if it's still there.
        let m = master.lock().unwrap().take();
        drop(m);
        if exited.recv_timeout(std::time::Duration::from_secs(3)).is_err() {
            let _ = killer.kill();
        }
    });
    let mut buf = vec![0u8; 64 * 1024];
    while let Ok(n @ 1..) = reader.read(&mut buf) {
        feed(&buf[..n]);
    }
    let status = waiter.join().map_err(|_| anyhow::anyhow!("waiting for the program failed"))??;
    // A failing program keeps its tab open, so what it printed can be read.
    if !status.success() {
        return Err(Exited(status.exit_code()).into());
    }
    Ok(())
}

/// A tab's program ended with an error code.
#[derive(Debug)]
struct Exited(u32);

impl std::fmt::Display for Exited {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "the program exited with code {}", self.0)
    }
}

impl std::error::Error for Exited {}

fn run_serial(
    port: &str,
    baud: u32,
    mut rx: UnboundedReceiver<Ctl>,
    mut feed: impl FnMut(&[u8]),
) -> anyhow::Result<()> {
    let mut port = serialport::new(port, baud).timeout(std::time::Duration::from_millis(100)).open().map_err(|e| {
        use serialport::ErrorKind::{Io, NoDevice};
        match e.kind() {
            NoDevice | Io(std::io::ErrorKind::NotFound) => {
                anyhow::anyhow!("{port} isn't there: check the port's name, and that the device is plugged in")
            }
            _ => anyhow::anyhow!("couldn't open {port}: {e}"),
        }
    })?;
    let mut writer = port.try_clone()?;
    let closed = Arc::new(AtomicBool::new(false));
    let closed2 = closed.clone();
    std::thread::spawn(move || {
        while let Some(c) = rx.blocking_recv() {
            match c {
                Ctl::Input(b) => {
                    if writer.write_all(&b).is_err() {
                        break;
                    }
                }
                Ctl::Resize(..) | Ctl::Sftp(_) | Ctl::Cwd(_) => {}
                Ctl::Close => break,
            }
        }
        closed2.store(true, Ordering::Relaxed);
    });
    let mut buf = [0u8; 4096];
    while !closed.load(Ordering::Relaxed) {
        match port.read(&mut buf) {
            Ok(n) => feed(&buf[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

/// The text from physical row `start` on, at most its last `max` lines with text. The screen's
/// blank rows below the prompt don't count: on a tall screen right after `clear`, the last `max`
/// rows can all be blank (`run … until` once returned nothing that way).
fn rows_text(screen: &wezterm_term::Screen, start: usize, max: usize) -> String {
    let total = screen.scrollback_rows();
    let start = start.max(total.saturating_sub(max + screen.physical_rows));
    let mut text: Vec<String> =
        screen.lines_in_phys_range(start..total).iter().map(|l| l.as_str().trim_end().to_string()).collect();
    while text.last().is_some_and(String::is_empty) {
        text.pop();
    }
    text.split_off(text.len().saturating_sub(max)).join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A command's output is found below its line even on a screen taller than the line limit,
    /// with only blank rows under it (as right after `clear`).
    #[test]
    fn text_from_skips_blank_rows_below() {
        let mut i = inner(40, 80, None);
        i.term.advance_bytes("\x1b[H\x1b[2J$ ");
        let s = i.term.screen();
        let from = s.visible_row_to_stable_row(i.term.cursor_pos().y);
        i.term.advance_bytes("echo marker\r\nmarker\r\n$ ");
        let s = i.term.screen();
        let at = |r| s.stable_row_to_phys(r).unwrap();
        assert_eq!(rows_text(s, at(from), 50), "$ echo marker\nmarker\n$");
        assert_eq!(rows_text(s, at(from + 1), 50), "marker\n$");
        assert_eq!(rows_text(s, at(from), 2), "marker\n$");
        assert_eq!(rows_text(s, 0, 1), "$");
    }

    /// Host and user stay PowerShell string literals, whatever quotes they hold.
    #[test]
    fn winrm_command() {
        let argv = winrm_argv("srv'; calc; '", 5986, "CORP\\o\u{2019}brien", true, true);
        assert_eq!(&argv[1..5], ["-NoLogo", "-NoProfile", "-NoExit", "-Command"]);
        assert_eq!(
            argv[5],
            "try { $c = [pscredential]::new('CORP\\o\u{2019}\u{2019}brien', [Net.NetworkCredential]::new('', $env:XTERMIUM_WINRM_PASSWORD).SecurePassword); \
             Remove-Item Env:XTERMIUM_WINRM_PASSWORD; \
             Enter-PSSession -ComputerName 'srv''; calc; ''' -Port 5986 -Credential $c -UseSSL \
             -SessionOption (New-PSSessionOption -SkipCACheck -SkipCNCheck -SkipRevocationCheck) -ErrorAction Stop } \
             catch { $_; if ($_.Exception.ErrorCode -in 5, 1326) { exit 2 }; exit 1 }"
        );
        assert_eq!(
            winrm_argv("h", 5985, "", false, true)[5],
            "try { Enter-PSSession -ComputerName 'h' -Port 5985 -ErrorAction Stop } catch { $_; if ($_.Exception.ErrorCode -in 5, 1326) { exit 2 }; exit 1 }"
        );
    }

    /// VS Code opens an SSH folder through Remote-SSH, naming the host as the extension does.
    #[test]
    fn vscode_folders() {
        let ssh = |host: &str, user: &str, port: u16| {
            let mut s = crate::config::Ssh::default();
            (s.target.host, s.target.user, s.target.port) = (host.into(), user.into(), port);
            Kind::Ssh(s)
        };
        assert_eq!(code_args(&ssh("pve", "", 22), "/etc/pve", "").unwrap(), ["--folder-uri", "vscode-remote://ssh-remote+pve/etc/pve"]);
        // {"hostName":"Web01","user":"root","port":2222}, the default user standing in for none
        let hex = "7b22686f73744e616d65223a225765623031222c2275736572223a22726f6f74222c22706f7274223a323232327d";
        assert_eq!(
            code_args(&ssh("Web01", "", 2222), "/srv/my app#1", "root").unwrap()[1],
            format!("vscode-remote://ssh-remote+{hex}/srv/my%20app%231")
        );
        assert!(code_args(&Kind::Telnet { host: "h".into(), port: 23 }, "/", "").is_err());
    }

    /// Closing the tab ends a local program that's busy and never reads its input.
    #[test]
    fn closing_ends_a_busy_program() {
        let (tx, rx) = unbounded_channel();
        let prog = if cfg!(windows) { "ping -n 60 127.0.0.1" } else { "sleep 60" };
        let run = std::thread::spawn(move || run_pty(prog, "", "xterm-256color", rx, |_| {}));
        std::thread::sleep(std::time::Duration::from_millis(500));
        let start = std::time::Instant::now();
        // Typing it can't read, then the close.
        for _ in 0..200 {
            let _ = tx.send(Ctl::Input(vec![b'x'; 4096]));
        }
        tx.send(Ctl::Close).unwrap();
        while !run.is_finished() {
            assert!(start.elapsed().as_secs() < 8, "the program outlived its tab");
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    /// A program that is only a `.cmd` on the PATH goes through cmd on Windows; an `.exe`, a path,
    /// or another system runs as given.
    #[test]
    fn npm_shims_go_through_cmd() {
        let dir = crate::config::dir().join("shim-test");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("claude.cmd"), "@echo off
").unwrap();
        std::fs::write(dir.join("codex.cmd"), "@echo off
").unwrap();
        std::fs::write(dir.join("codex.exe"), "").unwrap();
        // npm's script for sh sits beside the `.cmd`: the pty library would start that one.
        std::fs::write(dir.join("claude"), "#!/bin/sh").unwrap();
        let path = dir.to_string_lossy().into_owned();
        let argv = |p: &str| vec![p.to_string(), "--model".into(), "x".into()];
        let on = |p: &str, path: &str| through_cmd(argv(p), std::ffi::OsStr::new(path));
        if cfg!(windows) {
            let through = (vec!["cmd".to_string(), "/c".into(), "claude".into(), "--model".into(), "x".into()], Some(dir.clone()));
            assert_eq!(on("claude", &path), through, "through cmd, with the folder for its PATH");
            assert_eq!(on("claude", &format!("C:/nowhere;\"{path}\"")), through, "a folder the PATH names in quotes too");
            let exe = dir.join("codex.exe").display().to_string();
            assert_eq!(on("codex", &path), (vec![exe, "--model".into(), "x".into()], None), "an exe of that name, by its full path");
        } else {
            assert_eq!(on("claude", &path), (argv("claude"), None));
        }
        assert_eq!(on("aider", &path), (argv("aider"), None), "not there at all: left for the error message");
        assert_eq!(on("C:/tools/claude.cmd", &path), (argv("C:/tools/claude.cmd"), None), "a path is run as it is");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A program's error code reaches the caller: WinRM forgets a refused password by it.
    #[test]
    fn exit_code_is_kept() {
        let (tx, rx) = unbounded_channel();
        // ConPTY asks where the cursor is before the program starts: the terminal's answer.
        let feed = move |b: &[u8]| {
            if b.windows(4).any(|w| w == b"\x1b[6n") {
                let _ = tx.send(Ctl::Input(b"\x1b[1;1R".to_vec()));
            }
        };
        let argv: &[&str] = if cfg!(windows) { &["cmd", "/c", "exit", "2"] } else { &["sh", "-c", "exit 2"] };
        let e = run_argv(argv.iter().map(|s| s.to_string()).collect(), vec![], "", "xterm-256color", rx, feed).unwrap_err();
        assert_eq!(e.downcast_ref::<Exited>().map(|e| e.0), Some(WINRM_REFUSED), "{e:#}");
    }

    /// A program that isn't there, or a folder that isn't, says so plainly.
    #[test]
    fn missing_program_or_folder_says_so() {
        let run = |prog: &str, dir: &str| {
            let (_tx, rx) = unbounded_channel();
            run_pty(prog, dir, "xterm-256color", rx, |_| {}).unwrap_err().to_string()
        };
        assert_eq!(run("no-such-agent --x", ""), "no-such-agent wasn't found: install it, or check that it's on your PATH");
        assert!(run("no-such-agent", "/no/such/folder").contains("doesn't exist"));
        // `~` is the home folder: it is there, what isn't under it still isn't.
        assert!(run("no-such-agent", "~/").contains("wasn't found"));
        assert!(run("~/no-such-agent", "~").contains("wasn't found"));
        assert!(run("no-such-agent", "~/no-such-xtermium-folder").contains("doesn't exist"));
    }

    /// Agents installed with npm are `.cmd` shims on Windows; they run, in the session's folder.
    #[cfg(windows)]
    #[test]
    fn runs_a_cmd_shim_in_its_folder() {
        let dir = std::env::temp_dir().join("xtermium-agent-test");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("agent.cmd"), "@echo in %CD%\r\n").unwrap();
        let (tx, rx) = unbounded_channel();
        let mut out = Vec::new();
        let prog = dir.join("agent.cmd").to_string_lossy().into_owned();
        let _ = run_pty(&prog, &dir.to_string_lossy(), "xterm-256color", rx, |b| {
            // ConPTY asks where the cursor is and waits for the terminal's answer.
            if b.windows(4).any(|w| w == b"\x1b[6n") {
                let _ = tx.send(Ctl::Input(b"\x1b[1;1R".to_vec()));
            }
            out.extend_from_slice(b);
        });
        let out = String::from_utf8_lossy(&out).replace(['\r', '\n'], "");
        assert!(out.contains("xtermium-agent-test"), "{out}");
    }

    /// Replaying the same keystroke encodes it per terminal mode: why broadcast sends keys, not bytes.
    #[test]
    fn replay_encodes_per_terminal_mode() {
        let run = |setup: &[u8]| {
            let (tx, mut rx) = unbounded_channel();
            let mut term = Terminal::new(
                TerminalSize::default(),
                Arc::new(TermConfig(ColorPalette::default(), 1000)),
                "xtermium",
                "test",
                Box::new(ChanWriter(Arc::new(Mutex::new(tx)))),
            );
            term.advance_bytes(setup);
            for e in
                [Typed::Key(KeyCode::Char('c'), KeyModifiers::CTRL), Typed::Key(KeyCode::UpArrow, KeyModifiers::NONE)]
            {
                apply(&mut term, &e);
            }
            // wezterm-term writes from its own thread; collect until both keys arrived.
            let mut sent = vec![];
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            while sent.len() < 4 && std::time::Instant::now() < deadline {
                match rx.try_recv() {
                    Ok(Ctl::Input(b)) => sent.extend(b),
                    _ => std::thread::sleep(std::time::Duration::from_millis(5)),
                }
            }
            sent
        };
        assert_eq!(run(b""), b"\x03\x1b[A");
        // DECCKM (application cursor keys), as set by vim/less.
        assert_eq!(run(b"\x1b[?1h"), b"\x03\x1bOA");
    }

    /// A log flood (`docker logs -f`): long JSON lines, colours, wide and combining characters,
    /// progress lines, stray bytes; read in odd-sized chunks that split characters, with frames
    /// made in between, a resize, scrolling back and a selection while the scrollback rolls over.
    #[test]
    fn survives_a_log_flood() {
        let mut seed = 0x9e3779b97f4a7c15u64;
        let mut rnd = move |n: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % n as u64) as usize
        };
        const BITS: [&str; 12] = [
            "{\"severity\":\"INFO\",\"path\":\"/api/v4/jobs/request\",\"meta.caller_id\":\"POST /api/:version/jobs/request\"}",
            "\x1b[32mINFO\x1b[0m ",
            "\x1b[1;31mERROR\x1b[0m \x1b[38;2;255;128;0mtruecolor\x1b[0m ",
            "日本語のログ ",
            "🚀👩‍💻🏳️‍🌈 ",
            "e\u{301}a\u{308}\u{20dd} ",
            "\t|\t",
            "\r[=====>     ] 50%",
            "\x08\x08\x7f",
            "\u{200b}\u{feff}\u{fe0f}",
            "\x1b[2K\x1b[1G\x1b[?25l\x1b[?25h",
            "\x1b]0;gitlab\x07",
        ];
        let mut data = Vec::new();
        for _ in 0..5_000 {
            for _ in 0..rnd(60) {
                data.extend_from_slice(BITS[rnd(BITS.len())].as_bytes());
            }
            if rnd(50) == 0 {
                data.extend((0..rnd(20)).map(|_| rnd(256) as u8)); // not UTF-8
            }
            data.extend_from_slice(if rnd(3) == 0 { b"\n" } else { b"\r\n" });
        }

        // A short scrollback, so it wraps around (where `docker logs -f` once crashed it).
        let mut i = inner(120, 40, None);
        i.look.scrollback = 1_000;
        i.term = new_term(&i.look, i.writer.clone());
        let size = terminal_size(&i);
        i.term.resize(size);
        let mut at = 0;
        let mut chunks = 0;
        while at < data.len() {
            let end = (at + 1 + rnd(8192)).min(data.len());
            i.output(&data[at..end]);
            at = end;
            chunks += 1;
            if chunks % 3 == 0 {
                i.frame(true);
            }
            match chunks {
                200 => {
                    i.size = (57, 13);
                    let size = terminal_size(&i);
                    i.term.resize(size);
                }
                400 => i.scroll = 500,
                600 => {
                    let top = i.term.screen().visible_row_to_stable_row(0);
                    i.sel = Some(((top, 3), (top + 5, 40)));
                    let _ = selection_text(&i);
                }
                _ => {}
            }
        }
        i.frame(true);
        let _ = selection_text(&i);
    }

    /// Scrolled back, output leaves the lines on screen where they are, unless the setting
    /// brings it to the bottom; at the bottom the screen follows the output.
    #[test]
    fn output_keeps_scrolled_back_screen() {
        let top = |i: &Inner| {
            let s = i.term.screen();
            let p = s.scrollback_rows() - i.size.1 - i.scroll;
            s.lines_in_phys_range(p..p + 1)[0].as_str().trim_end().to_owned()
        };
        let mut i = inner(20, 4, None);
        (0..30).for_each(|n| i.output(format!("line {n}\r\n").as_bytes()));
        assert_eq!(top(&i), "line 27");
        i.scroll = 10;
        assert_eq!(top(&i), "line 17");
        (30..40).for_each(|n| i.output(format!("line {n}\r\n").as_bytes()));
        assert_eq!((top(&i), i.scroll), ("line 17".into(), 20));
        i.look.scroll_on_output = true;
        i.output(b"line 40\r\n");
        assert_eq!((top(&i), i.scroll), ("line 38".into(), 0));
    }

    #[test]
    fn detects_scrolls() {
        let screen = |rows: &[&str]| -> Vec<Vec<Run>> {
            rows.iter()
                .map(|s| if s.is_empty() { vec![] } else { vec![Run(0, 1, s.to_string(), 0, None, 0)] })
                .collect()
        };
        let old = screen(&["a", "b", "c", "d"]);
        assert_eq!(scrolled(&old, &old), 0);
        assert_eq!(scrolled(&old, &screen(&["b", "c", "d", ""])), 1);
        assert_eq!(scrolled(&old, &screen(&["", "", "a", "b"])), -2);
        // A scroll region above a status bar.
        assert_eq!(scrolled(&screen(&["a", "b", "c", "bar"]), &screen(&["b", "c", "x", "bar"])), 1);
        // Blank rows don't count as kept.
        assert_eq!(scrolled(&screen(&["a", "", "", ""]), &screen(&["", "", "", "z"])), 0);
    }

    /// A sized terminal with nothing attached; `log` collects what it would write to a file.
    fn inner(cols: usize, rows: usize, log: Option<std::sync::mpsc::Sender<String>>) -> Inner {
        let (tx, _rx) = unbounded_channel();
        let look = Look {
            font: String::new(),
            font_size: 14.0,
            palette: ColorPalette::default(),
            scrollback: 10_000,
            scroll_on_output: false,
            cursor: Cursor::Block,
            cursor_blink: true,
            line_height: 1.0,
            copy_on_select: true,
            highlights: Default::default(),
            right_click_paste: true,
            strip_paste: true,
            log: false,
        };
        let mut i = Inner::new(look, Arc::new(Mutex::new(tx)), log.map(Log::new));
        (i.size, i.cell) = ((cols, rows), (8.0, 16.0));
        let size = terminal_size(&i);
        i.term.resize(size);
        i
    }

    /// A Co-op viewer's paste (`Tab::paste`) is the owner's kind of paste: trimmed when the owner
    /// strips pastes, and one bracketed paste when the program asked for it, so a newline in it
    /// doesn't run anything by itself.
    #[test]
    fn a_viewers_paste_is_a_paste() {
        let mut i = inner(20, 3, None);
        let (tx, mut rx) = unbounded_channel();
        i.term = new_term(&i.look, Arc::new(Mutex::new(tx)));
        let size = terminal_size(&i);
        i.term.resize(size);
        i.term.advance_bytes(b"\x1b[?2004h");
        apply(&mut i.term, &Typed::Paste(i.look.strip(" rm -rf build\necho done\n".into())));
        // wezterm writes from a thread of its own: wait for the whole paste.
        let (mut sent, until) = (vec![], std::time::Instant::now() + std::time::Duration::from_secs(5));
        while !sent.ends_with(b"\x1b[201~") && std::time::Instant::now() < until {
            match rx.try_recv() {
                Ok(Ctl::Input(b)) => sent.extend(b),
                _ => std::thread::sleep(std::time::Duration::from_millis(5)),
            }
        }
        assert_eq!(String::from_utf8(sent).unwrap(), "\x1b[200~rm -rf build\necho done\x1b[201~");
    }

    /// A viewer with bigger cells clicks the same cell on the owner's screen.
    #[test]
    fn viewer_pointers_land_on_the_same_cell() {
        // Their cells are 10 × 20 pixels, ours 8 × 16: the 3rd column and 2nd row either way.
        assert_eq!(in_our_cells((25.0, 30.0), (8.0, 16.0), Some((10.0, 20.0))), (20.0, 24.0));
        assert_eq!(in_our_cells((25.0, 30.0), (8.0, 16.0), Some((0.0, 0.0))), (25.0, 30.0));
        assert_eq!(in_our_cells((25.0, 30.0), (8.0, 16.0), None), (25.0, 30.0));
    }

    /// A shared tab's sink starts from a full frame (the one that carries the style), gets one
    /// again when a viewer joins, and otherwise the window's very bytes, except that the owner's
    /// selection is the owner's alone: viewers' frames leave it out.
    #[test]
    fn the_co_op_sink_gets_the_screen_without_our_selection() {
        let full = |m: &[u8]| m[0] == MSG_FRAME && std::str::from_utf8(&m[1..]).unwrap().contains("\"style\"");
        // The sink's frames; the owner's selection as told (`MSG_SELECTION`) goes to `told`.
        let told = std::cell::RefCell::new(vec![]);
        let drain = |rx: &mut UnboundedReceiver<Vec<u8>>| {
            let mut got = vec![];
            while let Ok(m) = rx.try_recv() {
                if m[0] == MSG_SELECTION {
                    told.borrow_mut().push(String::from_utf8(m[1..].to_vec()).unwrap());
                } else {
                    got.push(m);
                }
            }
            got
        };
        let mut i = inner(20, 3, None);
        i.term.advance_bytes(b"hello");
        assert!(!i.next_frames().is_empty());

        // Not shared: nothing is copied anywhere.
        assert!(i.share.is_none());

        let (tx, mut rx) = unbounded_channel();
        i.set_share(Some(tx.clone()));
        let first = i.next_frames();
        assert!(full(&drain(&mut rx)[0]), "sharing starts viewers from the whole screen");
        assert!(first.iter().all(|m| !full(m)), "the window isn't sent everything again");
        assert!(i.theirs.is_none(), "with no selection, viewers follow the window's bytes");
        assert_eq!(told.take(), ["null"], "the ring hears there is no selection");

        // A change after that is a small frame, the same to both.
        i.term.advance_bytes(b"!");
        let diff = i.next_frames();
        assert!(!diff.is_empty() && !full(&diff[0]));
        assert_eq!(drain(&mut rx), diff);

        // The owner selects: their window shows it, the viewers' frames don't.
        let sel_bg = rgb32(i.term.palette().selection_bg);
        let lit = |ms: &[Vec<u8>]| ms.iter().any(|m| String::from_utf8_lossy(&m[1..]).contains(&format!(",{sel_bg},")));
        let r0 = i.term.screen().phys_to_stable_row_index(0);
        i.sel = Some(((r0, 0), (r0, 4)));
        let ours = i.next_frames();
        assert!(lit(&ours), "the window shows the selection");
        let theirs = drain(&mut rx);
        assert!(!lit(&theirs), "the viewers don't");
        assert_eq!(told.take(), [format!("[[{r0},0],[{r0},4],false]")], "the ring hears of it, to draw it");
        // Output while it lasts reaches the viewers, still without it.
        i.term.advance_bytes(b"\r\nmore");
        i.next_frames();
        let theirs = drain(&mut rx);
        assert!(!theirs.is_empty() && !lit(&theirs));
        assert!(told.borrow().is_empty(), "told once, not with every frame");

        // Gone again: the viewers have what the window has, and follow its bytes once more.
        i.sel = None;
        i.next_frames();
        assert!(drain(&mut rx).is_empty(), "nothing changed for the viewers' frames");
        assert_eq!(told.take(), ["null"]);
        assert!(i.theirs.is_none());
        i.term.advance_bytes(b"?");
        let diff = i.next_frames();
        assert_eq!(drain(&mut rx), diff);

        // A viewer joining: everything again, to the viewers.
        i.set_share(Some(tx));
        i.next_frames();
        assert!(full(&drain(&mut rx)[0]));

        // A sink nobody listens to is let go.
        drop(rx);
        i.term.advance_bytes(b"?");
        i.next_frames();
        assert!(i.share.is_none());
    }

    /// A Co-op viewer's selection is their own: their pointer never selects here, and their copy
    /// reads any range (widened to words or lines like a double or triple click) without touching
    /// the owner's selection; whatever range they send, the walk stays inside the rows there are.
    #[test]
    fn a_viewers_selection_is_their_own() {
        let mut i = inner(20, 3, None);
        i.term.advance_bytes(b"ls /var/log\r\nsecond line");
        let r0 = i.term.screen().phys_to_stable_row_index(0);
        let owners = Some(((r0, 0), (r0, 1)));
        i.sel = owners;

        // The viewer drags over the first row: nothing selected here.
        let mouse = |kind, x: f32| Mouse { kind, button: 0, x, y: 8.0, lines: 0, ctrl: false, alt: false, shift: false, clicks: 1 };
        for (kind, x) in [(MouseKind::Down, 4.0), (MouseKind::Move, 60.0), (MouseKind::Up, 60.0)] {
            i.pointer(&mouse(kind, x), Some((8.0, 16.0)));
        }
        let double = Mouse { clicks: 2, ..mouse(MouseKind::Down, 40.0) };
        i.pointer(&double, Some((8.0, 16.0)));
        assert!(i.sel == owners && i.drag.is_none(), "the owner's selection stays as it was");

        assert_eq!(i.pick((r0, 4), (r0, 4), false, 1), (((r0, 3), (r0, 10)), Some("/var/log".into())));
        assert_eq!(i.pick((r0, 5), (r0, 5), false, 2), (((r0, 0), (r0, 19)), Some("ls /var/log".into())));
        // Backwards is the same selection.
        assert_eq!(i.pick((r0 + 1, 0), (r0, 3), false, 0), (((r0, 3), (r0 + 1, 0)), Some("/var/log\ns".into())));
        assert_eq!(i.pick((r0, 3), (r0 + 1, 5), true, 0).1.as_deref(), Some("/va\nond"));
        // Out of range: the rows there are.
        let ((a, b), text) = i.pick((isize::MIN, 0), (isize::MAX, usize::MAX), false, 0);
        assert!(a.0 >= r0 - 10_000 && b.0 <= r0 + 3 && b.1 == 19);
        assert_eq!(text.as_deref().map(str::trim_end), Some("ls /var/log\nsecond line"));
        assert_eq!(i.sel, owners);
    }

    /// The log holds the text as the screen showed it: no colours, redrawn progress bars once in
    /// their final state, backspaces applied, wrapped lines whole, full-screen programs left out.
    #[test]
    fn logs_plain_text() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut i = inner(40, 5, Some(tx));
        for chunk in [
            &b"\x1b[1;31mred\x1b[0m plain\r\n"[..],
            b"progress 10%\rprogress 55%",
            b"\rprogress 100%\r\n",
            b"abc\x08\x08XY\r\n",
            b"a line that runs past forty columns and wraps\r\n",
            b"\x1b[?1049hFULL SCREEN APP\r\nredrawn\x1b[?1049l",
            b"one\r\ntwo\r\nthree\r\nfour\r\nfive\r\n",
            b"\x1b[2J\x1b[Hafter clear\r\n",
            b"prompt> ",
        ] {
            i.term.advance_bytes(chunk);
            i.log_lines(false);
        }
        i.log_lines(true);
        let text: String = rx.try_iter().collect();
        assert_eq!(
            text,
            "red plain\nprogress 100%\naXY\na line that runs past forty columns and wraps\none\ntwo\nthree\nfour\nfive\nafter clear\nprompt>\n"
        );
    }

    /// The find bar: matches across wrapped rows, in the scrollback, with or without case, and
    /// next to wide characters; the current match is highlighted strongest.
    #[test]
    fn finds_text() {
        let mut i = inner(20, 3, None);
        i.term.advance_bytes("Error one\r\n世界 error two\r\n0123456789abcdefgERROR wraps\r\nlast\r\n");
        let rows = |hits: &[(Cell, Cell)]| -> Vec<(usize, usize, usize, usize)> {
            let s = i.term.screen();
            let phys = |r| s.stable_row_to_phys(r).unwrap();
            hits.iter().map(|((r0, c0), (r1, c1))| (phys(*r0), *c0, phys(*r1), *c1)).collect()
        };
        // "世界 " is 5 cells wide: the match starts in column 5. The last one wraps at column 20.
        let hits = search(&i.term, "error", false);
        assert_eq!(rows(&hits), [(0, 0, 0, 4), (1, 5, 1, 9), (2, 17, 3, 1)]);
        assert_eq!(rows(&search(&i.term, "ERROR", true)), [(2, 17, 3, 1)]);
        assert!(search(&i.term, "absent", false).is_empty());

        // Starts at the newest match at or above the view's bottom; steps wrap around, and an
        // off-screen match is scrolled into view.
        assert_eq!(i.find("error", false, 0), (3, 3));
        assert_eq!(i.find("error", false, 1), (3, 1));
        assert!(i.scroll > 0, "scrolled back to the first match");
        assert_eq!(i.find("error", false, -1), (3, 3));
        assert_eq!(i.find("ERROR", true, 0), (1, 1));
        assert_eq!(i.find("absent", false, 0), (0, 0));
        assert_eq!(i.find("error", false, 1), (3, 1));
        i.scroll = 3; // the first rows on screen
        let f = String::from_utf8(i.frame(true).pop().unwrap()[1..].to_vec()).unwrap();
        assert!(f.contains(&format!(",{HIT_FG},{HIT_NOW_BG},")), "current match highlighted: {f}");
        assert!(f.contains(&format!(",{HIT_FG},{HIT_BG},")), "other matches highlighted: {f}");
    }

    /// Highlight rules colour matches in plain text, and leave text a program coloured alone.
    #[test]
    fn highlights_plain_text() {
        let mut i = inner(60, 3, None);
        i.look.highlights = Arc::new(crate::settings::Highlight::compile(true, &crate::settings::Highlight::defaults(), 0x0b0b0d));
        i.term.advance_bytes("ERROR at 192.0.2.12:22 then \x1b[35mERROR\x1b[0m [2001:db8::1:2] at 10:22\r\n");
        let f = String::from_utf8(i.frame(true).pop().unwrap()[1..].to_vec()).unwrap();
        assert_eq!(f.matches(&format!("\"ERROR\",{}", 0xef5f6b)).count(), 1, "{f}");
        assert!(f.contains(&format!("\"192.0.2.12:22\",{}", 0x5fa8f5)), "{f}");
        assert!(f.contains(&format!("\"2001:db8::1:2\",{}", 0x5fa8f5)), "shortened IPv6: {f}");
        assert!(!f.contains(&format!("10:22\",{}", 0x5fa8f5)), "a time isn't an address: {f}");
        assert!(crate::settings::Highlight::compile(false, &crate::settings::Highlight::defaults(), 0x0b0b0d).is_empty());
    }

    #[test]
    fn finds_web_addresses() {
        let found = |t: &str| urls(t).into_iter().map(|r| t[r].to_string()).collect::<Vec<_>>();
        assert_eq!(
            found("see https://a.b/c?d=1, and (http://x.y/z_(1)) or <https://q.r/>. xhttp://no"),
            ["https://a.b/c?d=1", "http://x.y/z_(1)", "https://q.r/"]
        );
        assert_eq!(found("[docs](https://x.y/a) end."), ["https://x.y/a"]);
        assert!(found("http:// https:/x plain").is_empty());
    }

    /// Ctrl+hover finds the address under the pointer: in plain text (wrapped rows too) and in
    /// OSC 8 hyperlinks, where the link text needn't look like an address.
    #[test]
    fn finds_links_under_the_pointer() {
        let mut i = inner(20, 4, None);
        i.term.advance_bytes("go https://example.com/long/path ok\r\n\x1b]8;;https://osc.example/\x1b\\click me\x1b]8;;\x1b\\ x\r\n");
        let s = |p| i.term.screen().phys_to_stable_row_index(p);
        let (r0, r1, r2) = (s(0), s(1), s(2));
        // Row 0 holds "go https://example.c", row 1 the rest: the address spans both.
        let (url, span) = link_at(&i.term, (r1, 3)).unwrap();
        assert_eq!(url, "https://example.com/long/path");
        assert_eq!(span, ((r0, 3), (r1, 11)));
        assert_eq!(link_at(&i.term, (r0, 4)).unwrap().0, url);
        assert!(link_at(&i.term, (r0, 0)).is_none(), "\"go\" isn't part of it");
        assert!(link_at(&i.term, (r1, 13)).is_none(), "\"ok\" isn't either");
        let (url, span) = link_at(&i.term, (r2, 2)).unwrap();
        assert_eq!((url.as_str(), span), ("https://osc.example/", ((r2, 0), (r2, 7))));
        assert!(link_at(&i.term, (r2, 9)).is_none());
    }

    /// A selected row is lit to the right edge, and the last row up to the end cell.
    #[test]
    fn selection_fills_rows_to_the_edge() {
        let mut i = inner(20, 4, None);
        i.term.advance_bytes("ab\r\ncdef\r\n");
        let (r0, r1) = (i.term.screen().phys_to_stable_row_index(0), i.term.screen().phys_to_stable_row_index(1));
        i.sel = Some(((r0, 1), (r1, 2)));
        let f = String::from_utf8(i.frame(true).pop().unwrap()[1..].to_vec()).unwrap();
        let (fg, bg) = (rgb32(i.term.palette().foreground), rgb32(i.term.palette().selection_bg));
        assert!(f.contains(&format!("[2,18,\"{}\",{fg},{bg},0]", " ".repeat(18))), "the first row's blanks past the text: {f}");
        assert!(!f.contains("[3,17,"), "the last row stops at the end cell: {f}");
        assert!(f.contains(&format!("[0,3,\"cde\",{fg},{bg},0]")) && f.contains("[3,1,"), "the last row's selected text, then the rest: {f}");
        i.sel = Some(((r0, 5), (r1, 0)));
        let f = String::from_utf8(i.frame(true).pop().unwrap()[1..].to_vec()).unwrap();
        assert!(f.contains(&format!("[5,15,\"{}\",{fg},{bg},0]", " ".repeat(15))), "a selection that starts past the text: {f}");
    }

    /// Double click takes a word (paths and hosts whole, wide characters included), triple click
    /// the line with the rows it wrapped onto.
    #[test]
    fn selects_words_and_lines() {
        let mut i = inner(20, 4, None);
        i.term.advance_bytes("ls /var/log  世界x\r\n0123456789abcdefghijKLM\r\n");
        let s = |p| i.term.screen().phys_to_stable_row_index(p);
        let (r0, r1, r2) = (s(0), s(1), s(2));
        assert_eq!(word_at(&i.term, (r0, 5)), ((r0, 3), (r0, 10)), "/var/log");
        assert_eq!(word_at(&i.term, (r0, 0)), ((r0, 0), (r0, 1)), "ls");
        assert_eq!(word_at(&i.term, (r0, 11)), ((r0, 11), (r0, 11)), "a blank selects itself");
        assert_eq!(word_at(&i.term, (r0, 14)), ((r0, 13), (r0, 17)), "世界x, from a wide character's second half");
        assert_eq!(line_at(&i.term, r2), ((r1, 0), (r2, 19)), "a wrapped line, from its second row");
        assert_eq!(word_at(&i.term, (r2, 1)), ((r1, 0), (r2, 2)), "a wrapped word, from its second row");
        assert_eq!(word_at(&i.term, (r1, 3)), ((r1, 0), (r2, 2)), "a wrapped word, from its first row");
        // Double click on "/var/log", drag back to "ls": both words whole; and forward again.
        let from = word_at(&i.term, (r0, 5));
        i.sel = Some(stretch(from, word_at(&i.term, (r0, 1))));
        assert_eq!(selection_text(&i).as_deref(), Some("ls /var/log"));
        i.sel = Some(stretch(from, word_at(&i.term, (r0, 15))));
        assert_eq!(selection_text(&i).as_deref(), Some("/var/log  世界x"));
        i.sel = Some(word_at(&i.term, (r0, 5)));
        assert_eq!(selection_text(&i).as_deref(), Some("/var/log"));
        i.sel = Some(line_at(&i.term, r1));
        assert_eq!(selection_text(&i).as_deref(), Some("0123456789abcdefghijKLM"));
        // Alt+drag: the rectangle between the corners, dragged up and to the left.
        i.block = true;
        i.sel = Some(((r1, 5), (r0, 3)));
        assert_eq!(selection_text(&i).as_deref(), Some("/va\n345"));
        let s = sel_range(i.sel, true).unwrap();
        assert!(s.has(r0, 4) && s.has(r1, 3) && !s.has(r1, 6) && !s.has(r2, 4));
    }

    /// A reconnect keeps what the screen showed, undoes the old session's modes, and points the
    /// terminal's answers at the new connection.
    #[test]
    fn reconnect_keeps_the_output() {
        // A plain shell: the marker follows the old lines, which stay as they were.
        let mut i = inner(40, 8, None);
        i.term.advance_bytes("line one\r\nline two\r\n$ ");
        i.reconnecting(unbounded_channel().0);
        i.term.advance_bytes("new prompt");
        let s = i.term.screen();
        let text: Vec<String> =
            s.lines_in_phys_range(0..s.scrollback_rows()).iter().map(|l| l.as_str().trim_end().to_string()).collect();
        assert_eq!(text[..3], ["line one", "line two", "$"], "{text:?}");
        assert!(text[3].starts_with("── reconnecting at ") && text[4] == "new prompt", "{text:?}");

        // A full-screen program was running.
        let mut i = inner(40, 5, None);
        i.term.advance_bytes("before the drop\r\n\x1b[?1049h\x1b[?1000h\x1b[?25lfull screen app");
        assert!(i.term.is_alt_screen_active() && i.term.is_mouse_grabbed());
        let (tx, mut rx) = unbounded_channel();
        i.reconnecting(tx);
        assert!(!i.term.is_alt_screen_active() && !i.term.is_mouse_grabbed());
        let s = i.term.screen();
        let text: Vec<String> =
            s.lines_in_phys_range(0..s.scrollback_rows()).iter().map(|l| l.as_str().trim_end().to_string()).collect();
        assert!(text.iter().any(|l| l == "before the drop"), "{text:?}");
        assert!(text.iter().any(|l| l.starts_with("── reconnecting at ")), "{text:?}");
        // A cursor position request: the answer (written by wezterm's own thread) goes to the new
        // connection.
        i.term.advance_bytes("\x1b[6n");
        let mut got = vec![];
        for _ in 0..200 {
            if let Ok(Ctl::Input(b)) = rx.try_recv() {
                got.extend(b);
            }
            if got.ends_with(b"R") {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(got.starts_with(b"\x1b[") && got.ends_with(b"R"), "{got:?}");
    }

    /// Where a coloured log flood spends its time on a big screen. Measure before and after
    /// touching `frame()` or logging: `cargo test --release bench_flood -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn bench_flood() {
        use std::time::{Duration, Instant};
        let mut data = Vec::new();
        for n in 0..200_000 {
            data.extend_from_slice(
                format!("\x1b[2m2026-10-13T12:00:{:02}Z\x1b[0m \x1b[32mINFO\x1b[0m worker[{n}] GET /api/v4/jobs/{n} 200 \x1b[36m{}ms\x1b[0m user=root ok\r\n", n % 60, n % 977)
                    .as_bytes(),
            );
        }
        // Logging: the same output, with every line also going to a log.
        let (tx, rx) = std::sync::mpsc::channel();
        let drain = std::thread::spawn(move || rx.iter().map(|s: String| s.len()).sum::<usize>());
        let mut logged = inner(200, 60, Some(tx));
        let t = Instant::now();
        for chunk in data.chunks(64 * 1024) {
            logged.term.advance_bytes(chunk);
            logged.log_lines(false);
        }
        let with_log = t.elapsed();
        drop(logged);
        println!("with logging: parse and log {with_log:?}, {} MB of text logged", drain.join().unwrap() >> 20);

        let mut i = inner(200, 60, None);
        let (mut parse, mut build, mut frames, mut bytes) = (Duration::ZERO, Duration::ZERO, 0, 0);
        for chunk in data.chunks(64 * 1024) {
            let t = Instant::now();
            i.term.advance_bytes(chunk);
            parse += t.elapsed();
            let t = Instant::now();
            let m = i.frame(true);
            build += t.elapsed();
            frames += 1;
            bytes += m.iter().map(Vec::len).sum::<usize>();
        }
        println!(
            "{} MB: parse {parse:?}, {frames} frames {build:?} ({:?}/frame, {} KB/frame)",
            data.len() >> 20,
            build / frames,
            (bytes / frames as usize) >> 10
        );
    }
}
