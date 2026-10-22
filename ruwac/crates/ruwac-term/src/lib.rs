use alacritty_terminal::{
    Term,
    event::{Event, EventListener},
    grid::{Dimensions, Grid},
    index::{Column, Line, Point},
    term::{
        ClipboardType, Config, Osc52, TermMode,
        cell::{Cell, Flags},
    },
    vte::ansi::{Color, Processor},
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
type Formatter = Arc<dyn Fn(&str) -> String + Sync + Send>;
/// Largest OSC 52 mouse selection a program may offer to the desktop.
pub const MAX_SELECTION: usize = 128 * 1024;
#[derive(Default)]
struct Pending {
    replies: Vec<String>,
    store: Option<String>,
    load: Option<Formatter>,
}
#[derive(Clone, Default)]
struct Events(Arc<Mutex<Pending>>);
impl EventListener for Events {
    fn send_event(&self, e: Event) {
        let mut pending = self.0.lock().unwrap();
        match e {
            Event::PtyWrite(s) => pending.replies.push(s),
            // OSC 52 PRIMARY ("p"/"s") only, e.g. a nested RUWAC client's mouse
            // selection. The desktop decides whether a user gesture backs it;
            // CLIPBOARD ("c") writes and reads stay ignored.
            Event::ClipboardStore(ClipboardType::Selection, text) => pending.store = Some(text),
            Event::ClipboardLoad(ClipboardType::Selection, format) => pending.load = Some(format),
            _ => (),
        }
    }
}
#[derive(Clone, Copy)]
struct Size(usize, usize);
impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.0
    }
    fn screen_lines(&self) -> usize {
        self.0
    }
    fn columns(&self) -> usize {
        self.1
    }
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Archive {
    pub rows: u16,
    pub cols: u16,
    pub epoch: u64,
    pub history: Vec<(String, bool)>,
    pub grid: Value,
    pub dropped_lines: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary: Option<PrimaryArchive>,
}

impl Archive {
    /// The writer and reader apply the same bounds. Invalid live output must
    /// never replace the last readable durable checkpoint.
    pub fn validate(&self) -> std::result::Result<(), String> {
        validate_terminal_data(self.rows, self.cols, &self.history, &self.grid)?;
        if let Some(primary) = &self.primary {
            if self.grid["alternate_screen"] != true
                || primary.grid["alternate_screen"] != false
                || self.history.iter().map(|(s, _)| s.len()).sum::<usize>()
                    + primary.history.iter().map(|(s, _)| s.len()).sum::<usize>()
                    > 512 * 1024
            {
                return Err("invalid combined primary/alternate archive".into());
            }
            validate_terminal_data(primary.rows, primary.cols, &primary.history, &primary.grid)?;
        }
        Ok(())
    }
}
fn validate_terminal_data(
    rows: u16,
    cols: u16,
    history: &[(String, bool)],
    grid: &Value,
) -> std::result::Result<(), String> {
    if !ruwac_protocol::TerminalLimits::CURRENT.valid(rows, cols)
        || history.len() > 50000
        || history.iter().map(|(s, _)| s.len()).sum::<usize>() > 512 * 1024
        || history
            .iter()
            .any(|(s, _)| s.chars().any(|c| c.is_control() && c != '\t'))
    {
        return Err("invalid terminal archive dimensions/history".into());
    }
    validate_grid(grid, rows, cols)
}

/// Flat hidden-primary archive: no recursively nested buffers are accepted.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrimaryArchive {
    pub rows: u16,
    pub cols: u16,
    pub epoch: u64,
    pub history: Vec<(String, bool)>,
    pub grid: Value,
    pub dropped_lines: usize,
}
impl PrimaryArchive {
    fn from_archive(a: Archive) -> Self {
        Self {
            rows: a.rows,
            cols: a.cols,
            epoch: a.epoch,
            history: a.history,
            grid: a.grid,
            dropped_lines: a.dropped_lines,
        }
    }
    fn as_archive(&self) -> Archive {
        Archive {
            rows: self.rows,
            cols: self.cols,
            epoch: self.epoch,
            history: self.history.clone(),
            grid: self.grid.clone(),
            dropped_lines: self.dropped_lines,
            primary: None,
        }
    }
}

/// Immutable, bounded canonical copy view, independent of later output/resize.
pub struct HistorySnapshot {
    pub rows: Vec<(String, bool)>,
    pub bytes: usize,
    pub cols: u16,
    pub epoch: u64,
    pub dropped: usize,
    pub cursor: (usize, usize),
    pub alternate: bool,
    pub buffer: &'static str,
}
impl HistorySnapshot {
    pub fn metadata(&self) -> Value {
        json!({"epoch":self.epoch,"total_lines":self.rows.len(),"cols":self.cols,
            "text_bytes":self.bytes,"dropped_lines":self.dropped,"cursor_row":self.cursor.0,"cursor_col":self.cursor.1,
            "alternate_screen":self.alternate,"buffer":self.buffer})
    }
    pub fn page(&self, offset: usize, lines: usize) -> Value {
        let mut text = String::new();
        let mut rows = Vec::new();
        let mut wrapped = Vec::new();
        for (row, wrap) in self
            .rows
            .iter()
            .skip(offset.min(self.rows.len()))
            .take(lines.min(500))
        {
            if text.len() + row.len() + 1 > 128 * 1024 {
                break;
            }
            text.push_str(row);
            if !wrap {
                text.push('\n');
            }
            wrapped.push(*wrap);
            rows.push(json!({"text":row,"wrapped":wrap}));
        }
        let mut result = self.metadata();
        result["offset"] = json!(offset);
        result["next_offset"] = json!(offset.saturating_add(rows.len()));
        result["text"] = json!(text);
        result["rows"] = json!(rows);
        result["wrapped"] = json!(wrapped);
        result
    }
}

pub struct Terminal {
    archived: Option<Archive>,
    term: Term<Events>,
    parser: Processor,
    events: Events,
    selection_serial: u64,
    selection: Option<(u64, String)>,
    selection_request: Option<(u64, Formatter)>,
    pub rows: u16,
    pub cols: u16,
    pub history_epoch: u64,
}
impl Terminal {
    pub fn new(rows: u16, cols: u16) -> Self {
        let events = Events::default();
        let config = Config {
            scrolling_history: history_limit(cols),
            osc52: Osc52::CopyPaste,
            ..Config::default()
        };
        let term = Term::new(config, &Size(rows as usize, cols as usize), events.clone());
        Self {
            archived: None,
            term,
            parser: Processor::new(),
            events,
            selection_serial: 0,
            selection: None,
            selection_request: None,
            rows,
            cols,
            history_epoch: 0,
        }
    }
    pub fn feed(&mut self, b: &[u8]) -> Vec<String> {
        if self.archived.is_some() {
            return Vec::new();
        }
        self.history_epoch = self.history_epoch.wrapping_add(1);
        self.parser.advance(&mut self.term, b);
        let (replies, store, load) = {
            let mut pending = self.events.0.lock().unwrap();
            (
                std::mem::take(&mut pending.replies),
                pending.store.take(),
                pending.load.take(),
            )
        };
        if let Some(text) =
            store.filter(|t| !t.is_empty() && t.len() <= MAX_SELECTION && !t.contains('\0'))
        {
            self.selection_serial += 1;
            self.selection = Some((self.selection_serial, text));
        }
        if let Some(format) = load {
            self.selection_serial += 1;
            self.selection_request = Some((self.selection_serial, format));
        }
        replies
    }
    /// Serials of the latest OSC 52 PRIMARY offer and read request.
    pub fn selection_serials(&self) -> (Option<u64>, Option<u64>) {
        (
            self.selection.as_ref().map(|(n, _)| *n),
            self.selection_request.as_ref().map(|(n, _)| *n),
        )
    }
    pub fn selection_offer(&self, serial: u64) -> Option<&str> {
        self.selection
            .as_ref()
            .filter(|(n, _)| *n == serial)
            .map(|(_, t)| t.as_str())
    }
    /// The program's OSC 52 answer for one outstanding read request.
    pub fn selection_reply(&mut self, serial: u64, text: &str) -> Option<String> {
        if self
            .selection_request
            .as_ref()
            .is_none_or(|(n, _)| *n != serial)
        {
            return None;
        }
        let (_, format) = self.selection_request.take()?;
        Some(format(text))
    }
    pub fn resize(&mut self, rows: u16, cols: u16) {
        self.history_epoch = self.history_epoch.wrapping_add(1);
        self.rows = rows;
        self.cols = cols;
        if self.archived.is_some() {
            return;
        }
        self.term.set_options(Config {
            scrolling_history: history_limit(cols),
            osc52: Osc52::CopyPaste,
            ..Config::default()
        });
        self.term.resize(Size(rows as usize, cols as usize))
    }
    pub fn reset_input_modes(&mut self) {
        self.feed(
            b"\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l\x1b[?2004l\x1b[?1l\x1b[?1049l\x1b[0m",
        );
    }
    pub fn sgr_mouse(&self) -> bool {
        self.archived.is_none() && self.term.mode().contains(TermMode::SGR_MOUSE)
    }
    pub fn mouse(&self) -> bool {
        self.archived.is_none()
            && self.term.mode().intersects(
                TermMode::MOUSE_REPORT_CLICK | TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION,
            )
    }
    pub fn paste(&self) -> bool {
        self.archived.is_none() && self.term.mode().contains(TermMode::BRACKETED_PASTE)
    }
    /// Persist canonical cells/text, never raw terminal escape sequences or input.
    /// Each pane gets a bounded tail; truncation is explicit on restored history.
    pub fn archive(&self) -> Archive {
        self.archive_encoded(false)
    }
    /// Compact durable grids; canonical state and the public render format stay unchanged.
    pub fn checkpoint_archive(&self) -> Archive {
        self.archive_encoded(true)
    }
    fn archive_encoded(&self, compact: bool) -> Archive {
        if let Some(a) = &self.archived {
            let mut archive = a.clone();
            archive.epoch = self.history_epoch;
            if !compact {
                expand_grid(&mut archive.grid);
                if let Some(primary) = archive.primary.as_mut() {
                    expand_grid(&mut primary.grid);
                }
            }
            return archive;
        }
        let alternate = self.term.mode().contains(TermMode::ALT_SCREEN);
        let mut active =
            self.archive_grid_encoded(self.term.grid(), alternate, 512 * 1024, compact);
        if alternate {
            // Read the actual hidden canonical grid; no buffer swap, parser replay
            // or second live model. The text budget is shared by both buffers.
            let used = active
                .history
                .iter()
                .map(|(row, _)| row.len())
                .sum::<usize>();
            let primary = self.archive_grid_encoded(
                self.term.primary_grid(),
                false,
                (512_usize * 1024).saturating_sub(used),
                compact,
            );
            active.primary = Some(PrimaryArchive::from_archive(primary));
        }
        active
    }
    pub fn primary_available(&self) -> bool {
        self.archived
            .as_ref()
            .is_none_or(|a| a.grid["alternate_screen"] != true || a.primary.is_some())
    }
    pub fn primary_archive(&self) -> Option<Archive> {
        if let Some(a) = &self.archived {
            let mut value = if a.grid["alternate_screen"] == true {
                a.primary.as_ref()?.as_archive()
            } else {
                a.clone()
            };
            expand_grid(&mut value.grid);
            return Some(value);
        }
        Some(self.archive_grid(self.term.primary_grid(), false, 512 * 1024))
    }
    fn archive_grid(&self, g: &Grid<Cell>, alternate: bool, budget: usize) -> Archive {
        self.archive_grid_encoded(g, alternate, budget, false)
    }
    fn archive_grid_encoded(
        &self,
        g: &Grid<Cell>,
        alternate: bool,
        budget: usize,
        compact: bool,
    ) -> Archive {
        let top = g.topmost_line().0;
        let bottom = g.bottommost_line().0;
        let mut history = Vec::new();
        let mut bytes = 0usize;
        for r in (top..=bottom).rev().take(50000) {
            let mut row = String::new();
            for c in 0..self.cols as usize {
                let cell = &g[Point::new(Line(r), Column(c))];
                if !cell
                    .flags
                    .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
                {
                    row.push(if cell.c == '\t' { ' ' } else { cell.c });
                    if let Some(z) = cell.zerowidth() {
                        row.extend(z.iter());
                    }
                }
            }
            let wrap = g[Point::new(Line(r), Column(self.cols as usize - 1))]
                .flags
                .contains(Flags::WRAPLINE);
            let row = if wrap { row } else { row.trim_end().to_owned() };
            if bytes + row.len() > budget {
                break;
            }
            bytes += row.len();
            history.push((row, wrap));
        }
        history.reverse();
        Archive {
            rows: self.rows,
            cols: self.cols,
            epoch: self.history_epoch,
            dropped_lines: (bottom - top + 1) as usize - history.len(),
            history,
            grid: self.grid_encoded(g, alternate, compact),
            primary: None,
        }
    }
    pub fn freeze_history(&self) -> HistorySnapshot {
        self.freeze_buffer(false).unwrap()
    }
    pub fn freeze_primary_history(&self) -> Option<HistorySnapshot> {
        self.freeze_buffer(true)
    }
    fn freeze_buffer(&self, primary: bool) -> Option<HistorySnapshot> {
        let selected;
        let archived = if primary {
            if let Some(a) = &self.archived {
                selected = if a.grid["alternate_screen"] == true {
                    a.primary.as_ref()?.as_archive()
                } else {
                    a.clone()
                };
                Some(&selected)
            } else {
                None
            }
        } else {
            self.archived.as_ref()
        };
        if let Some(a) = archived {
            let cursor = a.history.len().saturating_sub(a.rows as usize)
                + a.grid["cursor"]["row"].as_u64().unwrap_or(0) as usize;
            return Some(HistorySnapshot {
                rows: a.history.clone(),
                bytes: a.history.iter().map(|(s, _)| s.len() + 1).sum(),
                cols: a.cols,
                epoch: self.history_epoch,
                dropped: a.dropped_lines,
                cursor: (
                    cursor,
                    a.grid["cursor"]["col"].as_u64().unwrap_or(0) as usize,
                ),
                alternate: a.grid["alternate_screen"] == true,
                buffer: if primary { "primary" } else { "active" },
            });
        }
        let g = if primary {
            self.term.primary_grid()
        } else {
            self.term.grid()
        };
        let top = g.topmost_line().0;
        let bottom = g.bottommost_line().0;
        let total = (bottom - top + 1) as usize;
        let mut rows = Vec::new();
        let mut bytes = 0;
        'history: for r in (top..=bottom).rev().take(50000) {
            let mut row = String::new();
            for c in 0..self.cols as usize {
                let cell = &g[Point::new(Line(r), Column(c))];
                if !cell
                    .flags
                    .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
                {
                    for ch in std::iter::once(if cell.c == '\t' { ' ' } else { cell.c })
                        .chain(cell.zerowidth().into_iter().flat_map(|z| z.iter().copied()))
                    {
                        if row.len() + ch.len_utf8()
                            > (8 * 1024 * 1024 - bytes).min(128 * 1024) + self.cols as usize * 4
                        {
                            break 'history;
                        }
                        row.push(ch);
                    }
                }
            }
            let wrap = g[Point::new(Line(r), Column(self.cols as usize - 1))]
                .flags
                .contains(Flags::WRAPLINE);
            if !wrap {
                row.truncate(row.trim_end().len());
            }
            if row.len() + 1 > 128 * 1024 || bytes + row.len() + 1 > 8 * 1024 * 1024 {
                break;
            }
            row.shrink_to_fit();
            bytes += row.len() + 1;
            rows.push((row, wrap));
        }
        rows.reverse();
        let dropped = total - rows.len();
        Some(HistorySnapshot {
            rows,
            bytes,
            cols: self.cols,
            epoch: self.history_epoch,
            dropped,
            cursor: (
                ((g.cursor.point.line.0 - top).max(0) as usize).saturating_sub(dropped),
                g.cursor.point.column.0,
            ),
            alternate: !primary && self.term.mode().contains(TermMode::ALT_SCREEN),
            buffer: if primary { "primary" } else { "active" },
        })
    }
    pub fn restore(mut a: Archive) -> std::result::Result<Self, String> {
        a.validate()?;
        if let Some(primary) = a.primary.as_mut() {
            for (row, _) in &mut primary.history {
                *row = row.replace('\t', " ");
            }
        }
        // Legacy checkpoints contain the tab placeholder plus already expanded cells.
        // One blank cell preserves displayed columns without expanding it twice.
        for (row, _) in &mut a.history {
            *row = row.replace('\t', " ");
        }
        let mut term = Self::new(a.rows, a.cols);
        term.history_epoch = a.epoch.wrapping_add(1);
        term.archived = Some(a);
        Ok(term)
    }
    pub fn capture(&self, lines: usize) -> String {
        self.capture_buffer(lines, false).unwrap()
    }
    pub fn capture_primary(&self, lines: usize) -> Option<String> {
        self.capture_buffer(lines, true)
    }
    fn capture_buffer(&self, lines: usize, primary: bool) -> Option<String> {
        let selected;
        let archived = if primary {
            if let Some(a) = &self.archived {
                selected = if a.grid["alternate_screen"] == true {
                    a.primary.as_ref()?.as_archive()
                } else {
                    a.clone()
                };
                Some(&selected)
            } else {
                None
            }
        } else {
            self.archived.as_ref()
        };
        if let Some(a) = archived {
            return Some(
                a.history
                    .iter()
                    .skip(a.history.len().saturating_sub(lines))
                    .map(|(s, _)| format!("{}\n", s.trim_end()))
                    .collect(),
            );
        }
        let g = if primary {
            self.term.primary_grid()
        } else {
            self.term.grid()
        };
        let bottom = g.bottommost_line().0;
        let start = (bottom + 1 - lines as i32).max(g.topmost_line().0);
        let mut out = String::new();
        for r in start..=bottom {
            let mut row = String::new();
            for c in 0..self.cols as usize {
                let cell = &g[Point::new(Line(r), Column(c))];
                if !cell
                    .flags
                    .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
                {
                    row.push(if cell.c == '\t' { ' ' } else { cell.c });
                    if let Some(z) = cell.zerowidth() {
                        row.extend(z.iter())
                    }
                }
            }
            out.push_str(row.trim_end());
            out.push('\n');
        }
        Some(out)
    }
    pub fn history_page(&self, offset: usize, lines: usize) -> Value {
        self.history_buffer_page(offset, lines, false).unwrap()
    }
    pub fn primary_history_page(&self, offset: usize, lines: usize) -> Option<Value> {
        self.history_buffer_page(offset, lines, true)
    }
    fn history_buffer_page(&self, offset: usize, lines: usize, primary: bool) -> Option<Value> {
        let selected;
        let archived = if primary {
            if let Some(a) = &self.archived {
                selected = if a.grid["alternate_screen"] == true {
                    a.primary.as_ref()?.as_archive()
                } else {
                    a.clone()
                };
                Some(&selected)
            } else {
                None
            }
        } else {
            self.archived.as_ref()
        };
        if let Some(a) = archived {
            let mut text = String::new();
            let mut wrapped = Vec::new();
            let mut rows = Vec::new();
            for (row, wrap) in a
                .history
                .iter()
                .skip(offset.min(a.history.len()))
                .take(lines.min(500))
            {
                if text.len() + row.len() + 1 > 128 * 1024 {
                    break;
                }
                text.push_str(row);
                if !wrap {
                    text.push('\n');
                }
                wrapped.push(*wrap);
                rows.push(json!({"text":row,"wrapped":wrap}));
            }
            return Some(
                json!({"text":text,"offset":offset,"next_offset":offset.saturating_add(wrapped.len()),"epoch":self.history_epoch,"total_lines":a.history.len(),"wrapped":wrapped,"rows":rows,"alternate_screen":a.grid["alternate_screen"] == true,"restored":true,"dropped_lines":a.dropped_lines,"buffer":if primary {"primary"}else{"active"}}),
            );
        }
        let g = if primary {
            self.term.primary_grid()
        } else {
            self.term.grid()
        };
        let start = g.topmost_line().0;
        let bottom = g.bottommost_line().0;
        let total = (bottom - start + 1) as usize;
        let mut text = String::new();
        let mut count = 0;
        let mut wrapped = Vec::new();
        let mut rows = Vec::new();
        for r in (start + offset.min(total) as i32)..=bottom {
            if count >= lines.min(500) {
                break;
            }
            let mut row = String::new();
            for c in 0..self.cols as usize {
                let cell = &g[Point::new(Line(r), Column(c))];
                if !cell
                    .flags
                    .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
                {
                    row.push(if cell.c == '\t' { ' ' } else { cell.c });
                    if let Some(z) = cell.zerowidth() {
                        row.extend(z.iter());
                    }
                }
            }
            let wrap = g[Point::new(Line(r), Column(self.cols as usize - 1))]
                .flags
                .contains(Flags::WRAPLINE);
            let row = if wrap { row.as_str() } else { row.trim_end() };
            if text.len() + row.len() + 1 > 128 * 1024 {
                break;
            }
            text.push_str(row);
            if !wrap {
                text.push('\n');
            }
            wrapped.push(wrap);
            rows.push(json!({"text":row,"wrapped":wrap}));
            count += 1;
        }
        Some(
            json!({"text":text,"offset":offset,"next_offset":offset.saturating_add(count),"epoch":self.history_epoch,"total_lines":total,"wrapped":wrapped,"rows":rows,"alternate_screen":!primary && self.term.mode().contains(TermMode::ALT_SCREEN),"buffer":if primary {"primary"}else{"active"}}),
        )
    }
    pub fn snapshot(&self) -> Value {
        if let Some(a) = &self.archived {
            let mut grid = a.grid.clone();
            expand_grid(&mut grid);
            // Restored cells are cropped/padded rather than reflowed. Wraps
            // only describe their original column width.
            if let Some(wrapped) = grid.get_mut("wrapped").and_then(Value::as_array_mut) {
                if self.cols as u64 != a.grid["cols"].as_u64().unwrap() {
                    wrapped.fill(json!(false));
                }
                wrapped.truncate(self.rows as usize);
                wrapped.resize(self.rows as usize, json!(false));
            }
            let blank = json!([" ", false, 256, 257, false, false]);
            let cells = grid["cells"].as_array_mut().unwrap();
            cells.truncate(self.rows as usize);
            cells.resize_with(self.rows as usize, || json!([]));
            for row in cells {
                let row = row.as_array_mut().unwrap();
                row.truncate(self.cols as usize);
                row.resize(self.cols as usize, blank.clone());
            }
            grid["rows"] = json!(self.rows);
            grid["cols"] = json!(self.cols);
            grid["cursor"]["visible"] = json!(false);
            for mode in [
                "mouse",
                "mouse_drag",
                "mouse_motion",
                "mouse_sgr",
                "application_cursor",
                "bracketed_paste",
            ] {
                grid[mode] = json!(false);
            }
            grid["restored"] = json!(true);
            return grid;
        }
        self.snapshot_grid(
            self.term.grid(),
            self.term.mode().contains(TermMode::ALT_SCREEN),
        )
    }
    fn snapshot_grid(&self, g: &Grid<Cell>, alternate: bool) -> Value {
        self.grid_encoded(g, alternate, false)
    }
    fn grid_encoded(&self, g: &Grid<Cell>, alternate: bool, compact: bool) -> Value {
        let mut palette = Vec::new();
        let mut keys = std::collections::HashMap::new();
        let mut rows = Vec::new();
        for r in 0..self.rows as usize {
            let mut cells = Vec::new();
            for c in 0..self.cols as usize {
                let cell = &g[Point::new(Line(r as i32), Column(c))];
                let mut s = cell.c.to_string();
                if let Some(z) = cell.zerowidth() {
                    s.extend(z.iter())
                }
                let key = (
                    s.clone(),
                    packed_color(cell.fg),
                    packed_color(cell.bg),
                    cell.flags.bits(),
                    cell.underline_color().map(packed_color),
                );
                if compact {
                    if let Some(index) = keys.get(&key) {
                        cells.push(json!(*index));
                        continue;
                    }
                }
                let mut value = json!([
                    s,
                    cell.flags
                        .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER),
                    color(cell.fg),
                    color(cell.bg),
                    cell.flags.contains(Flags::BOLD),
                    cell.flags.contains(Flags::INVERSE)
                ]);
                let extended = cell.flags.intersects(
                    Flags::ITALIC
                        | Flags::DIM
                        | Flags::HIDDEN
                        | Flags::STRIKEOUT
                        | Flags::ALL_UNDERLINES
                        | Flags::WIDE_CHAR,
                ) || cell.underline_color().is_some();
                if extended {
                    let underline = if cell.flags.contains(Flags::UNDERCURL) {
                        "curly"
                    } else if cell.flags.contains(Flags::DOUBLE_UNDERLINE) {
                        "double"
                    } else if cell.flags.contains(Flags::DOTTED_UNDERLINE) {
                        "dotted"
                    } else if cell.flags.contains(Flags::DASHED_UNDERLINE) {
                        "dashed"
                    } else if cell.flags.contains(Flags::UNDERLINE) {
                        "single"
                    } else {
                        "none"
                    };
                    value.as_array_mut().unwrap().push(json!({
                        "italic":cell.flags.contains(Flags::ITALIC),"dim":cell.flags.contains(Flags::DIM),
                        "hidden":cell.flags.contains(Flags::HIDDEN),"strike":cell.flags.contains(Flags::STRIKEOUT),
                        "width":if cell.flags.contains(Flags::WIDE_CHAR) {2} else {1},
                        "underline":underline,"underline_color":cell.underline_color().map(color)
                    }));
                }
                if compact {
                    let index = palette.len();
                    keys.insert(key, index);
                    palette.push(value);
                    cells.push(json!(index));
                } else {
                    cells.push(value);
                }
            }
            rows.push(cells)
        }
        let mut value = json!({"rows":self.rows,"cols":self.cols,"cells":rows,"cursor":{"row":g.cursor.point.line.0,"col":g.cursor.point.column.0,"visible":self.term.mode().contains(TermMode::SHOW_CURSOR)},"mouse":self.mouse(),"mouse_drag":self.term.mode().contains(TermMode::MOUSE_DRAG),"mouse_motion":self.term.mode().contains(TermMode::MOUSE_MOTION),"mouse_sgr":self.term.mode().contains(TermMode::SGR_MOUSE),"alternate_screen":alternate,"application_cursor":self.term.mode().contains(TermMode::APP_CURSOR),"bracketed_paste":self.paste()});
        value["wrapped"] = json!(
            (0..self.rows as i32)
                .map(
                    |row| g[Point::new(Line(row), Column(self.cols as usize - 1))]
                        .flags
                        .contains(Flags::WRAPLINE)
                )
                .collect::<Vec<_>>()
        );
        if compact {
            let indices = value.as_object_mut().unwrap().remove("cells").unwrap();
            value["cell_encoding"] = json!("palette-v1");
            value["cell_palette"] = json!(palette);
            value["cell_indices"] = indices;
            // Dictionaries are counterproductive for mostly unique cells.
            // Preserve the prior plain encoding in that case.
            if palette.len() > self.rows as usize * self.cols as usize / 2 {
                expand_grid(&mut value);
            }
        }
        value
    }
}
fn packed_color(c: Color) -> u32 {
    match c {
        Color::Spec(rgb) => {
            0x1000000 | ((rgb.r as u32) << 16) | ((rgb.g as u32) << 8) | rgb.b as u32
        }
        Color::Indexed(i) => i as u32,
        Color::Named(n) => n as u32,
    }
}
fn valid_cell(cell: &Value) -> bool {
    let Some(c) = cell.as_array() else {
        return false;
    };
    (c.len() == 6 || c.len() == 7 && valid_extended_style(&c[6]))
        && c[0]
            .as_str()
            .is_some_and(|s| s.len() <= 4096 && !s.chars().any(|c| c.is_control() && c != '\t'))
        && [1, 4, 5].iter().all(|i| c[*i].is_boolean())
        && [2, 3].iter().all(|i| valid_color(&c[*i]))
}
fn valid_extended_style(style: &Value) -> bool {
    let Some(s) = style.as_object() else {
        return false;
    };
    s.len() == 7
        && s.contains_key("underline_color")
        && ["italic", "dim", "hidden", "strike"]
            .iter()
            .all(|key| style[*key].is_boolean())
        && matches!(style["width"].as_u64(), Some(1 | 2))
        && matches!(
            style["underline"].as_str(),
            Some("none" | "single" | "double" | "curly" | "dotted" | "dashed")
        )
        && (style["underline_color"].is_null() || valid_color(&style["underline_color"]))
}
fn validate_grid(grid: &Value, rows: u16, cols: u16) -> std::result::Result<(), String> {
    if grid["rows"] != rows || grid["cols"] != cols {
        return Err("invalid terminal archive grid".into());
    }
    if let Some(wrapped) = grid.get("wrapped") {
        if !wrapped
            .as_array()
            .is_some_and(|v| v.len() == rows as usize && v.iter().all(Value::is_boolean))
        {
            return Err("invalid terminal wrap metadata".into());
        }
    }
    if grid.get("cell_encoding").is_some() {
        if grid["cell_encoding"] != "palette-v1" || grid.get("cells").is_some() {
            return Err("invalid cell encoding".into());
        }
        let palette = grid["cell_palette"].as_array().ok_or("invalid palette")?;
        if palette.is_empty()
            || palette.len() > rows as usize * cols as usize
            || !palette.iter().all(valid_cell)
        {
            return Err("invalid palette cells".into());
        }
        let indices = grid["cell_indices"]
            .as_array()
            .ok_or("invalid palette rows")?;
        if indices.len() != rows as usize
            || !indices.iter().all(|row| {
                row.as_array().is_some_and(|r| {
                    r.len() == cols as usize
                        && r.iter()
                            .all(|i| i.as_u64().is_some_and(|i| i < palette.len() as u64))
                })
            })
        {
            return Err("invalid palette indices".into());
        }
    } else {
        if grid.get("cell_palette").is_some() || grid.get("cell_indices").is_some() {
            return Err("ambiguous cell encoding".into());
        }
        let matrix = grid["cells"].as_array().ok_or("invalid cells")?;
        if matrix.len() != rows as usize
            || !matrix.iter().all(|row| {
                row.as_array()
                    .is_some_and(|r| r.len() == cols as usize && r.iter().all(valid_cell))
            })
        {
            return Err("invalid terminal archive cells".into());
        }
    }
    Ok(())
}
/// Used only after the bounded palette was validated, when a renderer requests
/// its existing canonical six-field cell tuples. History reads avoid expansion.
fn expand_grid(grid: &mut Value) {
    if grid["cell_encoding"] != "palette-v1" {
        return;
    }
    let palette = grid["cell_palette"].as_array().unwrap();
    let rows: Vec<Vec<Value>> = grid["cell_indices"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            row.as_array()
                .unwrap()
                .iter()
                .map(|i| palette[i.as_u64().unwrap() as usize].clone())
                .collect()
        })
        .collect();
    let object = grid.as_object_mut().unwrap();
    object.remove("cell_encoding");
    object.remove("cell_palette");
    object.remove("cell_indices");
    object.insert("cells".into(), json!(rows));
}
fn valid_color(c: &Value) -> bool {
    c.as_u64().is_some_and(|i| i <= 268)
        || c.as_array()
            .is_some_and(|a| a.len() == 3 && a.iter().all(|n| n.as_u64().is_some_and(|n| n <= 255)))
}
fn history_limit(cols: u16) -> usize {
    let lines = std::env::var("RUWAC_SCROLLBACK_LINES")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(50000)
        .clamp(2000, 1000000);
    let budget = std::env::var("RUWAC_SCROLLBACK_MIB")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(256)
        .clamp(8, 512)
        * 1024
        * 1024;
    lines
        .min(
            budget
                / (cols.max(1) as usize
                    * std::mem::size_of::<alacritty_terminal::term::cell::Cell>()),
        )
        .max(1)
}
fn color(c: Color) -> Value {
    match c {
        Color::Spec(rgb) => json!([rgb.r, rgb.g, rgb.b]),
        Color::Indexed(i) => json!(i),
        Color::Named(n) => json!(n as u16),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grid_modes_unicode() {
        let mut t = Terminal::new(4, 20);
        t.feed("ž界e\u{301}🧪".as_bytes());
        assert!(t.capture(4).contains("ž界e\u{301}🧪"));
        t.feed(b"\x1b[38;2;1;2;3mX\x1b[?1000h\x1b[?2004h");
        let s = t.snapshot();
        assert_eq!(s["cells"][0][6][2], json!([1, 2, 3]));
        assert!(t.mouse() && t.paste());
        t.feed(b"\x1b[?1000l\x1b[?2004l");
        assert!(!t.mouse() && !t.paste());
        t.feed(b"\x1b[?1049hALT");
        assert!(t.capture(4).contains("ALT"));
        t.feed(b"\x1b[?1049l");
        assert!(t.capture(4).contains("ž界"));
    }
    #[test]
    fn tabs_copy_at_canonical_columns() {
        let mut t = Terminal::new(4, 16);
        t.feed(b"A\tB\r\n");
        assert_eq!(t.capture(4).lines().next(), Some("A       B"));
        let page = t.history_page(0, 500);
        assert_eq!(page["rows"][0]["text"], "A       B");
        assert_eq!(page["rows"][0]["wrapped"], false);
        assert_eq!(t.snapshot()["cells"][0][1][0], "\t");
        let restored = Terminal::restore(t.archive()).unwrap();
        assert_eq!(restored.history_page(0, 500)["rows"], page["rows"]);
        assert_eq!(restored.capture(4), t.capture(4));
    }
    #[test]
    fn paged_scrollback_and_wrapping() {
        let mut t = Terminal::new(4, 80);
        for n in 0..51000 {
            t.feed(format!("line-{n:05} ž界\r\n").as_bytes());
        }
        let p = t.history_page(0, 500);
        assert!(p["total_lines"].as_u64().unwrap() >= 50000);
        assert_eq!(p["next_offset"], 500);
        let next = t.history_page(500, 500);
        assert_ne!(p["text"], next["text"]);
        assert!(next["text"].as_str().unwrap().contains("ž界"));
        t.feed(b"\x1b[?1049hALT");
        assert_eq!(t.history_page(0, 500)["total_lines"], 4);
        t.feed(b"\x1b[?1049l");
        assert!(t.history_page(0, 500)["total_lines"].as_u64().unwrap() >= 50000);
        let mut wrapped = Terminal::new(2, 4);
        wrapped.feed(b"abcdefghX");
        let p = wrapped.history_page(0, 500);
        assert!(p["wrapped"].as_array().unwrap().iter().any(|v| v == true));
        assert!(p["text"].as_str().unwrap().contains("abcdefghX"));
    }
    #[test]
    fn archived_canonical_unicode_cells_and_read_only_modes() {
        let mut t = Terminal::new(4, 10);
        t.feed(
            "ž界e\u{301}🧪abcdefghijk\r\n\x1b[38;2;1;2;3mCOLOUR\x1b[?1000h\x1b[?2004h".as_bytes(),
        );
        t.feed(b"\r\nTAB\tEND");
        let before = t.history_page(0, 500);
        assert!(before["text"].as_str().unwrap().contains("TAB"));
        assert!(!before["text"].as_str().unwrap().contains('\t'));
        let a: Archive =
            serde_json::from_slice(&serde_json::to_vec(&t.archive()).unwrap()).unwrap();
        let mut restored = Terminal::restore(a.clone()).unwrap();
        assert_eq!(restored.capture(4), t.capture(4));
        assert_eq!(restored.snapshot()["cells"], t.snapshot()["cells"]);
        assert_eq!(restored.history_page(0, 500)["text"], before["text"]);
        assert_eq!(restored.history_page(0, 500)["wrapped"], before["wrapped"]);
        assert!(!restored.mouse() && !restored.paste());
        restored.feed(b"FORGED-INPUT");
        assert_eq!(restored.capture(4), t.capture(4));
        restored.resize(6, 20);
        assert_eq!(restored.snapshot()["cells"].as_array().unwrap().len(), 6);
        assert_eq!(
            restored.snapshot()["cells"][0].as_array().unwrap().len(),
            20
        );
        let mut bad = a;
        bad.grid["cells"][0][0][0] = json!("\x1b");
        assert!(Terminal::restore(bad).is_err());
    }
    #[test]
    fn archive_has_explicit_bounded_tail() {
        let mut t = Terminal::new(4, 80);
        for i in 0..20000 {
            t.feed(format!("line-{i:05} {}\r\n", "X".repeat(60)).as_bytes());
        }
        let a = t.archive();
        assert!(a.dropped_lines > 0);
        assert!(a.history.iter().map(|(s, _)| s.len()).sum::<usize>() <= 512 * 1024);
        let restored = Terminal::restore(a).unwrap();
        assert!(restored.capture(4).contains("line-19999"));
        assert!(
            restored.history_page(0, 500)["dropped_lines"]
                .as_u64()
                .unwrap()
                > 0
        );
    }
    #[test]
    fn frozen_history_survives_output_resize_and_enforces_tail_bound() {
        let mut t = Terminal::new(4, 10);
        t.feed("ž界e\u{301}abcdefghijk\r\nMARK\r\n".as_bytes());
        let frozen = t.freeze_history();
        let before = frozen.page(0, 500);
        t.feed(b"LIVE-OUTPUT\r\n");
        t.resize(8, 25);
        assert_eq!(frozen.page(0, 500), before);
        assert_eq!(frozen.metadata()["cols"], 10);
        assert_ne!(t.history_page(0, 500)["epoch"], before["epoch"]);
        let mut large = Terminal::new(4, 80);
        for n in 0..51000 {
            large.feed(format!("line-{n:05}\r\n").as_bytes());
        }
        let tail = large.freeze_history();
        assert_eq!(tail.metadata()["total_lines"], 50000);
        assert!(tail.metadata()["dropped_lines"].as_u64().unwrap() > 0);
        assert!(tail.metadata()["text_bytes"].as_u64().unwrap() <= 8 * 1024 * 1024);
        assert!(
            tail.page(49500, 500)["text"]
                .as_str()
                .unwrap()
                .contains("line-50999")
        );
    }
    #[test]
    fn canonical_primary_survives_alt_and_checkpoint_without_mutation() {
        let mut t = Terminal::new(4, 24);
        t.feed("PRIMARY Živjo 界e\u{301}\r\nline two\r\n".as_bytes());
        t.feed(b"\x1b[?1049hALT application\x1b[38;2;1;2;3m");
        let before = t.snapshot();
        let a = t.archive();
        assert_eq!(t.snapshot(), before);
        assert!(
            a.primary
                .as_ref()
                .unwrap()
                .history
                .iter()
                .any(|(s, _)| s.contains("PRIMARY Živjo"))
        );
        assert!(!a.history.iter().any(|(s, _)| s.contains("PRIMARY")));
        let restored = Terminal::restore(a).unwrap();
        assert!(restored.capture(20).contains("ALT application"));
        assert!(
            restored
                .capture_primary(20)
                .unwrap()
                .contains("PRIMARY Živjo 界e\u{301}")
        );
        assert_eq!(
            restored.freeze_primary_history().unwrap().metadata()["alternate_screen"],
            false
        );
        assert_eq!(
            restored.freeze_primary_history().unwrap().metadata()["buffer"],
            "primary"
        );
    }
    #[test]
    fn primary_uses_canonical_resize_and_alt_exit_state() {
        let mut t = Terminal::new(4, 40);
        t.feed(b"PRIMARY-resize-keeps-this-text\r\n");
        for byte in b"\x1b[?1049hALT-only" {
            t.feed(&[*byte]);
        }
        t.resize(6, 18);
        let primary = t.primary_archive().unwrap();
        assert_eq!(primary.cols, 18);
        assert_eq!(primary.rows, 6);
        let text = primary
            .history
            .iter()
            .map(|(s, wrap)| format!("{s}{}", if *wrap { "" } else { "\n" }))
            .collect::<String>();
        assert!(text.contains("PRIMARY-resize-keeps-this-text"), "{text:?}");
        let alt = t.capture(10);
        t.archive();
        assert_eq!(t.capture(10), alt);
        t.feed(b"\x1b[?1049l");
        assert!(
            t.capture(20)
                .replace('\n', "")
                .contains("PRIMARY-resize-keeps-this-text")
        );
        assert!(t.archive().primary.is_none());
    }
    #[test]
    fn dual_history_budget_and_legacy_missing_primary_are_explicit() {
        let mut t = Terminal::new(4, 80);
        for n in 0..10000 {
            t.feed(format!("{n:05}-{}\r\n", "x".repeat(65)).as_bytes());
        }
        t.feed(b"\x1b[?1049hALT");
        let mut a = t.archive();
        let p = a.primary.as_ref().unwrap();
        assert!(p.dropped_lines > 0);
        let bytes = a
            .history
            .iter()
            .chain(&p.history)
            .map(|(s, _)| s.len())
            .sum::<usize>();
        assert!(bytes <= 512 * 1024);
        let json = serde_json::to_value(&a).unwrap();
        assert!(json["primary"].get("primary").is_none());
        a.primary = None;
        let legacy = Terminal::restore(a).unwrap();
        assert!(!legacy.primary_available());
        assert!(legacy.capture_primary(20).is_none());
        assert!(legacy.capture(20).contains("ALT"));
    }
    #[test]
    fn corrupt_hidden_primary_is_rejected_without_replaying_controls() {
        let mut t = Terminal::new(4, 20);
        t.feed(b"primary\x1b[?1049hALT");
        let mut a = t.archive();
        a.primary
            .as_mut()
            .unwrap()
            .history
            .push(("\x1b[2J".into(), false));
        assert!(Terminal::restore(a).is_err());
        let mut a = t.archive();
        a.primary.as_mut().unwrap().grid["alternate_screen"] = json!(true);
        assert!(Terminal::restore(a).is_err());
        let mut json = serde_json::to_value(t.archive()).unwrap();
        json["primary"]["primary"] = json!({});
        assert!(serde_json::from_value::<Archive>(json).is_err());
    }
    #[test]
    fn replies_and_clipboard() {
        let mut t = Terminal::new(4, 20);
        assert!(!t.feed(b"\x1b[6n").is_empty());
        assert!(t.feed(b"\x1b]52;c;QUJD\x07").is_empty());
        assert_eq!(t.selection_serials(), (None, None));
        // PRIMARY offers and read requests wait for the desktop; nothing is
        // answered by the engine itself.
        assert!(t.feed(b"\x1b]52;p;QUJD\x07").is_empty());
        let (Some(offer), None) = t.selection_serials() else {
            panic!("offer expected")
        };
        assert_eq!(t.selection_offer(offer), Some("ABC"));
        assert_eq!(t.selection_offer(offer + 1), None);
        assert!(t.feed(b"\x1b]52;p;?\x07").is_empty());
        let (_, Some(request)) = t.selection_serials() else {
            panic!("request expected")
        };
        assert_eq!(t.selection_reply(request + 1, "x"), None);
        assert_eq!(
            t.selection_reply(request, "hi").as_deref(),
            Some("\x1b]52;p;aGk=\x07")
        );
        assert_eq!(t.selection_reply(request, "hi"), None);
    }
    #[test]
    fn compact_dual_grid_preserves_canonical_cells_and_history() {
        let mut t = Terminal::new(100, 200);
        let line = "Že\u{301}界".repeat(40);
        for r in 1..=100 {
            t.feed(format!("\x1b[{r};1H\x1b[38;2;1;2;3m\x1b[48;2;4;5;6m{line}").as_bytes());
        }
        t.feed(b"\x1b[?1049h\x1b[1;7m");
        for r in 1..=100 {
            t.feed(format!("\x1b[{r};1H{line}").as_bytes());
        }
        let plain = t.archive();
        let compact = t.checkpoint_archive();
        assert_eq!(compact.grid["cell_encoding"], "palette-v1");
        let plain_bytes = serde_json::to_vec(&plain).unwrap().len();
        let compact_bytes = serde_json::to_vec(&compact).unwrap().len();
        assert!(
            compact_bytes * 8 < plain_bytes,
            "{compact_bytes} / {plain_bytes}"
        );
        let restored = Terminal::restore(compact).unwrap();
        assert_eq!(restored.snapshot()["cells"], t.snapshot()["cells"]);
        assert_eq!(restored.capture(2000), t.capture(2000));
        assert_eq!(restored.capture_primary(2000), t.capture_primary(2000));
        assert_eq!(
            restored.primary_archive().unwrap().grid["cells"],
            plain.primary.as_ref().unwrap().grid["cells"]
        );
        assert_eq!(restored.archive().grid["cells"], plain.grid["cells"]);
        assert_eq!(
            restored.archive().primary.unwrap().grid["cells"],
            plain.primary.unwrap().grid["cells"]
        );
    }
    #[test]
    fn compact_unique_cells_keep_plain_encoding() {
        let mut t = Terminal::new(4, 20);
        for r in 1..=4 {
            for c in 1..=20 {
                t.feed(format!("\x1b[{r};{c}H\x1b[38;2;{r};{c};42mX").as_bytes());
            }
        }
        let archive = t.checkpoint_archive();
        assert!(archive.grid.get("cell_encoding").is_none());
        let restored = Terminal::restore(archive).unwrap();
        assert_eq!(restored.snapshot()["cells"], t.snapshot()["cells"]);
    }
    #[test]
    fn compact_corrupt_or_ambiguous_grids_fail_closed() {
        let t = Terminal::new(4, 20);
        let a = t.checkpoint_archive();
        assert_eq!(a.grid["cell_encoding"], "palette-v1");
        for variant in 0..8 {
            let mut bad = a.clone();
            match variant {
                0 => bad.grid["cell_indices"][0][0] = json!(-1),
                1 => bad.grid["cell_indices"][0][0] = json!(u64::MAX),
                2 => bad.grid["cell_palette"][0][0] = json!("\x1b[2J"),
                3 => bad.grid["cell_encoding"] = json!("unknown"),
                4 => bad.grid["cells"] = json!([]),
                5 => {
                    bad.grid.as_object_mut().unwrap().remove("cell_palette");
                }
                6 => {
                    bad.grid["cell_indices"][0].as_array_mut().unwrap().pop();
                }
                _ => {
                    bad.grid["cell_palette"] =
                        json!(vec![json!(["x", false, 0, 0, false, false]); 81])
                }
            }
            assert!(Terminal::restore(bad).is_err(), "variant {variant}");
        }
    }
    #[test]
    fn unreadable_live_glyph_is_rejected_before_checkpoint_replacement() {
        let mut t = Terminal::new(4, 20);
        t.feed(("e".to_owned() + &"\u{301}".repeat(3000)).as_bytes());
        let value = t.checkpoint_archive();
        assert!(value.validate().is_err());
        assert!(Terminal::restore(value).is_err());
        t.feed(b"\x1b[2J\x1b[HRECOVERED");
        assert!(t.checkpoint_archive().validate().is_ok());
    }
    #[test]
    fn extended_sgr_styles_survive_both_archive_encodings() {
        for (code, name) in [
            ("4", "single"),
            ("4:2", "double"),
            ("4:3", "curly"),
            ("4:4", "dotted"),
            ("4:5", "dashed"),
        ] {
            let mut t = Terminal::new(4, 20);
            t.feed(format!("\x1b[2;3;8;9;{code};58:2::7:8:9me\u{301}").as_bytes());
            let snapshot = t.snapshot();
            let style = &snapshot["cells"][0][0][6];
            for field in ["italic", "dim", "hidden", "strike"] {
                assert_eq!(style[field], true);
            }
            assert_eq!(style["underline"], name);
            assert_eq!(style["underline_color"], json!([7, 8, 9]));
            for archive in [t.archive(), t.checkpoint_archive()] {
                assert_eq!(
                    Terminal::restore(archive).unwrap().snapshot()["cells"],
                    snapshot["cells"]
                );
            }
            t.feed(b"\x1b[0mA");
            assert_eq!(t.snapshot()["cells"][0][1].as_array().unwrap().len(), 6);
        }
    }
    #[test]
    fn underline_color_participates_in_palette_identity() {
        let mut t = Terminal::new(4, 20);
        t.feed(b"\x1b[4;58;2;1;2;3mX\x1b[58;2;4;5;6mX");
        let expected = t.snapshot();
        assert_ne!(expected["cells"][0][0][6], expected["cells"][0][1][6]);
        assert_eq!(
            Terminal::restore(t.checkpoint_archive())
                .unwrap()
                .snapshot()["cells"],
            expected["cells"]
        );
    }
    #[test]
    fn invalid_extended_styles_are_rejected() {
        let mut t = Terminal::new(4, 20);
        t.feed(b"\x1b[3mX");
        let good = t.archive();
        for field in [
            "italic",
            "dim",
            "hidden",
            "strike",
            "width",
            "underline",
            "underline_color",
            "unexpected",
        ] {
            let mut bad = good.clone();
            bad.grid["cells"][0][0][6][field] = json!([999]);
            assert!(Terminal::restore(bad).is_err(), "{field}");
        }
        let mut bad = good.clone();
        let style = bad.grid["cells"][0][0][6].as_object_mut().unwrap();
        style.remove("underline_color");
        style.insert("unexpected".into(), json!(null));
        assert!(Terminal::restore(bad).is_err());
    }
}

#[cfg(test)]
mod selection_wrap_tests {
    use super::*;
    #[test]
    fn restored_wraps_follow_visible_rows_and_original_column_width() {
        let mut t = Terminal::new(4, 8);
        t.feed(b"abcdefghijkl\r\nnext");
        let original = t.archive();
        let mut restored = Terminal::restore(original.clone()).unwrap();
        restored.resize(2, 8);
        assert_eq!(restored.snapshot()["wrapped"], json!([true, false]));
        restored.resize(6, 8);
        assert_eq!(
            restored.snapshot()["wrapped"],
            json!([true, false, false, false, false, false])
        );
        restored.resize(6, 12);
        assert_eq!(restored.snapshot()["wrapped"], json!(vec![false; 6]));
        assert_eq!(restored.archive().grid, original.grid);
        assert!(restored.checkpoint_archive().validate().is_ok());
    }
    #[test]
    fn actual_parser_distinguishes_soft_wrap_from_explicit_line_break() {
        let mut t = Terminal::new(4, 8);
        t.feed(b"abcdefghijkl\r\nnext");
        assert_eq!(t.snapshot()["wrapped"], json!([true, false, false, false]));
        let a = t.archive();
        assert_eq!(
            Terminal::restore(a).unwrap().snapshot()["wrapped"],
            json!([true, false, false, false])
        );
    }
    #[test]
    fn malformed_wrap_metadata_is_rejected_but_legacy_archives_remain_readable() {
        let t = Terminal::new(3, 8);
        let mut a = t.archive();
        a.grid["wrapped"] = json!([true]);
        assert!(Terminal::restore(a).is_err());
        let mut a = t.archive();
        a.grid["wrapped"] = json!([false, 0, false]);
        assert!(Terminal::restore(a).is_err());
        let mut a = t.archive();
        a.grid.as_object_mut().unwrap().remove("wrapped");
        assert!(Terminal::restore(a).is_ok());
    }
}
