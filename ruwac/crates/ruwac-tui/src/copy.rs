//! Client-owned canonical selection. It never writes to a PTY or desktop clipboard.
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Clone, Debug, PartialEq)]
pub enum Key {
    Char(char),
    Sequence(String),
}
impl Key {
    pub fn escape(&self) -> bool {
        matches!(self,Self::Sequence(s) if s=="\x1b")
    }
    pub fn bytes(&self) -> String {
        match self {
            Self::Char(c) => c.to_string(),
            Self::Sequence(s) => s.clone(),
        }
    }
}
pub fn keys(text: &str) -> Vec<Key> {
    let mut out = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let c = rest.chars().next().unwrap();
        if c != '\x1b' {
            out.push(Key::Char(c));
            rest = &rest[c.len_utf8()..];
            continue;
        }
        let n = if rest.len() == 1 {
            1
        } else if rest.starts_with("\x1b[") {
            rest.as_bytes()[2..]
                .iter()
                .position(|b| (0x40..=0x7e).contains(b))
                .map(|n| n + 3)
                .unwrap_or(rest.len())
        } else if rest.starts_with("\x1bO") {
            rest.len().min(3)
        } else if rest.starts_with("\x1b]") {
            rest.find('\x07')
                .map(|n| n + 1)
                .or_else(|| rest.find("\x1b\\").map(|n| n + 2))
                .unwrap_or(rest.len())
        } else {
            1 + rest[1..].chars().next().unwrap().len_utf8()
        };
        out.push(Key::Sequence(rest[..n].into()));
        rest = &rest[n..];
    }
    out
}
pub fn width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}
pub fn clip(text: &str, columns: usize) -> String {
    let mut out = String::new();
    let mut used = 0;
    for g in text.graphemes(true) {
        if g.chars().any(char::is_control) {
            continue;
        }
        let w = width(g).max(1);
        if used + w > columns {
            break;
        }
        out.push_str(g);
        used += w;
    }
    out
}
pub fn backspace(text: &mut String) {
    if let Some((byte, _)) = text.grapheme_indices(true).last() {
        text.truncate(byte);
    }
}
#[derive(Clone)]
pub struct Line {
    pub text: String,
    pub wrapped: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Position {
    pub row: usize,
    pub col: usize,
}
#[derive(PartialEq)]
pub enum Action {
    Keep,
    Cancel,
    Save,
}
pub struct Copy {
    pub lines: Vec<Line>,
    pub cols: usize,
    pub cursor: Position,
    pub top: usize,
    pub left: usize,
    pub anchor: Option<Position>,
    pub rectangle: bool,
    pub dropped: usize,
    pub pane: String,
    pub message: String,
    live_styles: std::collections::BTreeMap<usize, Vec<serde_json::Value>>,
    styles_captured: bool,
    desired: usize,
    repeat: usize,
    margins: (Option<usize>, Option<usize>),
    edit: Option<(String, bool)>,
    search: Option<(String, bool)>,
}
impl Copy {
    pub fn new(
        lines: Vec<Line>,
        cols: usize,
        cursor: Position,
        dropped: usize,
        pane: String,
    ) -> Self {
        let row = cursor.row.min(lines.len().saturating_sub(1));
        let col = cursor.col.min(cols.saturating_sub(1));
        Self {
            lines,
            cols: cols.max(1),
            cursor: Position { row, col },
            top: row,
            left: 0,
            anchor: None,
            rectangle: false,
            dropped,
            pane,
            message: String::new(),
            live_styles: Default::default(),
            styles_captured: false,
            desired: col,
            repeat: 0,
            margins: (None, None),
            edit: None,
            search: None,
        }
    }
    /// History text is immutable; attach styles only to matching live rows.
    /// Older scrollback has no style metadata and remains plain text.
    pub fn preserve_live_styles(&mut self, snapshot: &serde_json::Value) {
        if self.styles_captured {
            return;
        }
        self.styles_captured = true;
        let Some(pane) = snapshot["panes"].as_array().and_then(|p| {
            p.iter()
                .find(|p| p["id"].as_str() == Some(self.pane.as_str()))
        }) else {
            return;
        };
        let grid = &pane["grid"];
        let Some(rows) = grid["cells"].as_array() else {
            return;
        };
        if grid["cols"].as_u64() != Some(self.cols as u64) || rows.len() > self.lines.len() {
            return;
        }
        let start = self.lines.len() - rows.len();
        for (n, row) in rows.iter().enumerate() {
            let Some(cells) = row.as_array() else {
                continue;
            };
            let text: String = cells
                .iter()
                .filter(|c| c[1] != true)
                .filter_map(|c| c[0].as_str())
                .collect();
            if text.trim_end_matches(' ') == self.lines[start + n].text.trim_end_matches(' ') {
                self.live_styles.insert(start + n, cells.clone());
            }
        }
    }
    fn style(&self, row: usize, col: usize, selected: bool) -> String {
        self.live_styles
            .get(&row)
            .and_then(|cells| cells.get(col))
            .map(|cell| format!("\x1b[{}m", super::cell_style(cell, selected)))
            .unwrap_or_else(|| {
                if selected {
                    "\x1b[0;7m".into()
                } else {
                    "\x1b[0m".into()
                }
            })
    }
    fn snap(&self, row: usize, col: usize, forward: bool) -> usize {
        let mut start = 0;
        for g in self.lines[row].text.graphemes(true) {
            let end = start + width(g).max(1);
            if col > start && col < end {
                return if forward && end < self.cols {
                    end
                } else {
                    start
                };
            }
            start = end;
        }
        col.min(self.cols - 1)
    }
    fn goto(&mut self, row: usize, col: usize) {
        self.cursor.row = row.min(self.lines.len().saturating_sub(1));
        self.cursor.col = self.snap(self.cursor.row, col.min(self.cols - 1), false);
        self.desired = self.cursor.col;
    }
    fn vertical(&mut self, row: usize) {
        self.cursor.row = row.min(self.lines.len() - 1);
        self.cursor.col = self.snap(self.cursor.row, self.desired.min(self.cols - 1), false);
    }
    fn joined(&self) -> (String, Vec<usize>) {
        let mut text = String::new();
        let mut starts = Vec::new();
        for line in &self.lines {
            starts.push(text.len());
            text.push_str(&line.text);
            if !line.wrapped {
                text.push('\n');
            }
        }
        (text, starts)
    }
    fn byte(&self, starts: &[usize]) -> usize {
        let mut col = 0;
        for (i, g) in self.lines[self.cursor.row].text.grapheme_indices(true) {
            if col + width(g).max(1) > self.cursor.col {
                return starts[self.cursor.row] + i;
            }
            col += width(g).max(1);
        }
        starts[self.cursor.row] + self.lines[self.cursor.row].text.len()
    }
    fn byte_position(&self, starts: &[usize], byte: usize) -> Position {
        let row = starts
            .partition_point(|s| *s <= byte)
            .saturating_sub(1)
            .min(self.lines.len() - 1);
        let offset = byte.saturating_sub(starts[row]);
        let mut col = 0;
        for (i, g) in self.lines[row].text.grapheme_indices(true) {
            if i + g.len() > offset {
                break;
            }
            col += width(g).max(1);
        }
        Position {
            row,
            col: col.min(self.cols - 1),
        }
    }
    fn find(&mut self, reverse: bool) {
        let Some((needle, _)) = &self.search else {
            return;
        };
        if needle.is_empty() {
            return;
        }
        let (text, starts) = self.joined();
        let at = self.byte(&starts);
        let end = at + text[at..].graphemes(true).next().map(str::len).unwrap_or(0);
        let found = if reverse {
            text[..at]
                .rfind(needle)
                .or_else(|| text[at..].rfind(needle).map(|n| at + n))
        } else {
            text[end..]
                .find(needle)
                .map(|n| end + n)
                .or_else(|| text[..end].find(needle))
        };
        if let Some(byte) = found {
            let p = self.byte_position(&starts, byte);
            self.goto(p.row, p.col);
            self.message = "Match found (search wraps)".into();
        } else {
            self.message = "No match".into();
        }
    }
    fn word(&mut self, key: char) {
        let (text, starts) = self.joined();
        let at = self.byte(&starts);
        let byte = match key {
            'w' => text
                .unicode_word_indices()
                .find(|(i, _)| *i > at)
                .map(|(i, _)| i),
            'b' => text
                .unicode_word_indices()
                .take_while(|(i, _)| *i < at)
                .last()
                .map(|(i, _)| i),
            _ => text
                .unicode_word_indices()
                .map(|(i, w)| {
                    (
                        i,
                        w.grapheme_indices(true)
                            .last()
                            .map(|(b, _)| i + b)
                            .unwrap_or(i),
                    )
                })
                .find(|(_, last)| *last > at)
                .map(|(_, last)| last),
        };
        if let Some(byte) = byte {
            let p = self.byte_position(&starts, byte);
            self.goto(p.row, p.col);
        }
    }
    /// Show the newest `height` lines with the cursor on the last one.
    pub fn align_bottom(&mut self, height: usize) {
        let last = self.lines.len().saturating_sub(1);
        self.top = self.lines.len().saturating_sub(height.max(1));
        self.cursor.row = last;
        self.desired = self.cursor.col;
    }
    /// Mouse wheel: move the view by `delta` lines (negative = older),
    /// keeping the cursor inside it.
    pub fn scroll(&mut self, delta: isize, height: usize) {
        let height = height.max(1);
        let max_top = self.lines.len().saturating_sub(height);
        let top = (self.top as isize + delta).clamp(0, max_top as isize) as usize;
        self.top = top;
        let bottom = (top + height - 1).min(self.lines.len().saturating_sub(1));
        self.cursor.row = self.cursor.row.clamp(top, bottom.max(top));
    }
    /// A held mouse drag at a copy-view edge moves only this immutable view.
    pub fn drag_scroll(&mut self, row: usize, col: usize, height: usize) -> bool {
        let height = height.max(1);
        let delta = if row == 0 {
            -2
        } else if row >= height - 1 {
            2
        } else {
            return false;
        };
        let previous = self.top;
        self.scroll(delta, height);
        if self.top == previous {
            return false;
        }
        self.point(row.min(height - 1), col);
        true
    }
    /// True when the newest line is visible.
    pub fn at_bottom(&self, height: usize) -> bool {
        self.top + height.max(1) >= self.lines.len()
    }
    /// Move the cursor to a 0-based screen position of the copy view.
    pub fn point(&mut self, row: usize, col: usize) {
        self.cursor.row = (self.top + row).min(self.lines.len().saturating_sub(1));
        self.cursor.col = (self.left + col).min(self.cols.saturating_sub(1));
        self.desired = self.cursor.col;
    }
    /// Begin a mouse selection at the cursor.
    pub fn mark(&mut self) {
        self.anchor = Some(self.cursor);
        self.rectangle = false;
    }
    pub fn paste_query(&mut self, text: &str) {
        if let Some((query, _)) = &mut self.edit {
            for c in text.chars().filter(|c| !c.is_control()) {
                if query.len() + c.len_utf8() > 1024 {
                    break;
                }
                query.push(c);
            }
        } else {
            self.message = "Paste ignored in selection mode".into();
        }
    }
    pub fn key(&mut self, key: &Key, height: usize) -> Action {
        if let Some((mut query, reverse)) = self.edit.take() {
            match key {
                Key::Char('\r' | '\n') => {
                    self.search = Some((query, reverse));
                    self.find(reverse);
                    return Action::Keep;
                }
                k if k.escape() => return Action::Keep,
                Key::Char('\x7f' | '\x08') => backspace(&mut query),
                Key::Char(c) if !c.is_control() => {
                    if query.len() + c.len_utf8() <= 1024 {
                        query.push(*c);
                    }
                }
                _ => (),
            }
            self.edit = Some((query, reverse));
            return Action::Keep;
        }
        if key.escape() || matches!(key, Key::Char('\x03' | 'q')) {
            return Action::Cancel;
        }
        if let Key::Char(c @ '0'..='9') = key {
            if *c != '0' || self.repeat != 0 {
                self.repeat =
                    (self.repeat.saturating_mul(10) + (*c as usize - '0' as usize)).min(100000);
                return Action::Keep;
            }
        }
        let n = std::mem::take(&mut self.repeat).max(1);
        let height = height.max(1);
        let action = match key {
            Key::Sequence(s) => match s.as_str() {
                "\x1b[A" | "\x1bOA" => "up",
                "\x1b[B" | "\x1bOB" => "down",
                "\x1b[C" | "\x1bOC" => "right",
                "\x1b[D" | "\x1bOD" => "left",
                "\x1b[H" | "\x1bOH" | "\x1b[1~" | "\x1b[7~" => "begin",
                "\x1b[F" | "\x1bOF" | "\x1b[4~" | "\x1b[8~" => "end",
                "\x1b[5~" => "pageup",
                "\x1b[6~" => "pagedown",
                _ => "",
            },
            Key::Char(c) => match c {
                'h' | '\x08' => "left",
                'j' | '\x0e' => "down",
                'k' | '\x10' => "up",
                'l' => "right",
                'g' => "begin",
                'G' => "end",
                '0' | '\x01' => "home",
                '$' => "last",
                '^' => "first",
                'H' => "top",
                'M' => "middle",
                'L' => "bottom",
                '\x02' => "pageup",
                '\x06' => "pagedown",
                '\x15' => "halfup",
                '\x04' => "halfdown",
                _ => "",
            },
        };
        match action {
            "up" => self.vertical(self.cursor.row.saturating_sub(n)),
            "down" => self.vertical(self.cursor.row.saturating_add(n)),
            "left" => {
                let col = self.snap(self.cursor.row, self.cursor.col.saturating_sub(n), false);
                self.goto(self.cursor.row, col);
            }
            "right" => {
                let col = self.snap(
                    self.cursor.row,
                    self.cursor.col.saturating_add(n).min(self.cols - 1),
                    true,
                );
                self.goto(self.cursor.row, col);
            }
            "begin" => self.goto(0, 0),
            "end" => self.goto(self.lines.len() - 1, 0),
            "home" => self.goto(self.cursor.row, 0),
            "last" => {
                let col = width(self.lines[self.cursor.row].text.trim_end()).saturating_sub(1);
                self.goto(self.cursor.row, col);
            }
            "first" => {
                let s = &self.lines[self.cursor.row].text;
                self.goto(self.cursor.row, width(&s[..s.len() - s.trim_start().len()]));
            }
            "top" => self.vertical(self.top),
            "middle" => self.vertical(self.top + height / 2),
            "bottom" => self.vertical(self.top + height - 1),
            "pageup" | "halfup" => self.vertical(self.cursor.row.saturating_sub(n.saturating_mul(
                if action == "halfup" {
                    height / 2
                } else {
                    height
                },
            ))),
            "pagedown" | "halfdown" => self.vertical(self.cursor.row.saturating_add(
                n.saturating_mul(if action == "halfdown" {
                    height / 2
                } else {
                    height
                }),
            )),
            _ => match key {
                Key::Char(' ' | '\r' | '\n') => {
                    if self.anchor.is_some() {
                        return Action::Save;
                    }
                    self.anchor = Some(self.cursor);
                    self.message = "First mark set; move and mark again to copy".into();
                }
                Key::Char('/' | '?') => {
                    self.edit = Some((String::new(), matches!(key, Key::Char('?'))))
                }
                Key::Char('n' | 'N') => {
                    if let Some((_, reverse)) = &self.search {
                        self.find(if matches!(key, Key::Char('N')) {
                            !*reverse
                        } else {
                            *reverse
                        });
                    }
                }
                Key::Char('v') => {
                    self.rectangle = !self.rectangle;
                    self.margins = (None, None);
                }
                Key::Char('c') => {
                    self.rectangle = true;
                    self.margins.0 = Some(self.cursor.col);
                }
                Key::Char('C') => {
                    self.rectangle = true;
                    self.margins.1 = Some(self.cursor.col);
                }
                Key::Char(c @ ('w' | 'b' | 'e')) => self.word(*c),
                _ => (),
            },
        }
        Action::Keep
    }
    fn bounds(&self, row: usize) -> Option<(usize, usize)> {
        let a = self.anchor?;
        let b = self.cursor;
        if row < a.row.min(b.row) || row > a.row.max(b.row) {
            return None;
        }
        if self.rectangle {
            let left = self.margins.0.unwrap_or(a.col.min(b.col));
            let right = self.margins.1.unwrap_or(a.col.max(b.col));
            return Some((left.min(right), left.max(right)));
        }
        let (a, b) = if a <= b { (a, b) } else { (b, a) };
        Some((
            if row == a.row { a.col } else { 0 },
            if row == b.row { b.col } else { usize::MAX },
        ))
    }
    pub fn selected(&self, limit: usize) -> Result<String, &'static str> {
        let Some(anchor) = self.anchor else {
            return Err("Set a first mark before copying");
        };
        let mut out = String::new();
        for row in anchor.row.min(self.cursor.row)..=anchor.row.max(self.cursor.row) {
            let (left, right) = self.bounds(row).unwrap();
            let mut col = 0;
            let mut used = 0;
            for g in self.lines[row].text.graphemes(true) {
                let w = width(g).max(1);
                if col <= right && col + w > left {
                    if out.len() + g.len() > limit {
                        return Err("Selection exceeds 256 KiB; internal buffer unchanged");
                    }
                    out.push_str(g);
                    used += w;
                }
                col += w;
            }
            if self.rectangle {
                let pad = (right - left + 1).saturating_sub(used).min(self.cols);
                if out.len() + pad > limit {
                    return Err("Selection exceeds 256 KiB; internal buffer unchanged");
                }
                out.extend(std::iter::repeat_n(' ', pad));
            }
            if row < anchor.row.max(self.cursor.row) && (self.rectangle || !self.lines[row].wrapped)
            {
                if out.len() == limit {
                    return Err("Selection exceeds 256 KiB; internal buffer unchanged");
                }
                out.push('\n');
            }
        }
        Ok(out)
    }
    pub fn render(&mut self, height: usize, columns: usize) -> String {
        let height = height.max(1);
        let columns = columns.max(1);
        if self.cursor.row < self.top {
            self.top = self.cursor.row;
        } else if self.cursor.row >= self.top + height {
            self.top = self.cursor.row - height + 1;
        }
        if self.cursor.col < self.left {
            self.left = self.cursor.col;
        } else if self.cursor.col >= self.left + columns {
            self.left = self.cursor.col - columns + 1;
        }
        let mut out =
            String::from("\x1b[?25l\x1b[?1003l\x1b[?1000h\x1b[?1002h\x1b[?1006h\x1b[0m\x1b[H");
        let mut style = String::new();
        for row in self.top..(self.top + height).min(self.lines.len()) {
            out.push_str(&format!("\x1b[{};1H", row - self.top + 1));
            let mut col = 0;
            for g in self.lines[row].text.graphemes(true) {
                let w = width(g).max(1);
                let selected = self
                    .bounds(row)
                    .is_some_and(|(a, b)| col <= b && col + w > a);
                if col >= self.left && col + w <= self.left + columns {
                    let next = self.style(row, col, selected);
                    if next != style {
                        out.push_str(&next);
                        style = next;
                    }
                    out.push_str(&clip(g, w));
                } else if col < self.left && col + w > self.left {
                    let next = self.style(row, col, selected);
                    if next != style {
                        out.push_str(&next);
                        style = next;
                    }
                    out.push(' ');
                }
                col += w;
                if col >= self.left + columns {
                    break;
                }
            }
            for blank in col.max(self.left)..self.left + columns {
                let selected = self
                    .bounds(row)
                    .is_some_and(|(a, b)| blank >= a && blank <= b);
                let next = self.style(row, blank, selected);
                if next != style {
                    out.push_str(&next);
                    style = next;
                }
                out.push(' ');
            }
        }
        // Clear only rows outside the immutable snapshot, never the whole screen.
        for y in self.lines.len().saturating_sub(self.top).min(height)..height {
            out.push_str(&format!("\x1b[{};1H\x1b[0m\x1b[K", y + 1));
        }
        let footer = if let Some((query, reverse)) = &self.edit {
            format!("{}{}", if *reverse { '?' } else { '/' }, query)
        } else if !self.message.is_empty() {
            format!("COPY · {}", self.message)
        } else {
            format!(
                "COPY {}/{} col {} · Space/Enter mark · / search · v rect · Esc · dropped {} · {}",
                self.cursor.row + 1,
                self.lines.len(),
                self.cursor.col + 1,
                self.dropped,
                self.pane
            )
        };
        out.push_str(&format!(
            "\x1b[{};1H{}{}{}\x1b[0m",
            height + 1,
            super::STATUS_STYLE,
            clip(&footer, columns),
            " ".repeat(columns.saturating_sub(width(&clip(&footer, columns))))
        ));
        if self.edit.is_none() {
            out.push_str(&format!(
                "\x1b[{};{}H\x1b[?25h",
                self.cursor.row - self.top + 1,
                self.cursor.col - self.left + 1
            ));
        }
        out
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn held_edge_drag_scrolls_history_without_moving_the_anchor() {
        let mut c = Copy::new(
            (0..100)
                .map(|n| Line {
                    text: format!("line {n:03}"),
                    wrapped: false,
                })
                .collect(),
            20,
            Position { row: 99, col: 0 },
            0,
            "pane".into(),
        );
        c.align_bottom(10);
        c.point(5, 3);
        c.mark();
        let anchor = c.anchor;
        assert!(c.drag_scroll(0, 2, 10));
        assert_eq!(c.top, 88);
        assert_eq!(c.anchor, anchor);
        assert!(!c.drag_scroll(4, 2, 10));
        while c.drag_scroll(0, 2, 10) {}
        assert_eq!(c.top, 0);
        assert_eq!(c.anchor, anchor);
        assert!(!c.drag_scroll(0, 2, 10));
        assert!(c.selected(10000).unwrap().lines().count() > 10);
    }
    #[test]
    fn uniform_copy_frame_has_bounded_transport_overhead() {
        let mut c = Copy::new(
            (0..64)
                .map(|_| Line {
                    text: "a".repeat(200),
                    wrapped: false,
                })
                .collect(),
            200,
            Position { row: 63, col: 0 },
            0,
            "pane".into(),
        );
        c.align_bottom(60);
        let frame = c.render(60, 200);
        assert!(
            frame.len() < 16000,
            "Uniform12000cell frame expanded to{}bytes",
            frame.len()
        );
        assert!(!frame.contains("\x1b[2J"));
    }
    #[test]
    fn mouse_selection_keeps_live_colours_and_never_erases_the_screen() {
        use serde_json::json;
        let mut c = Copy::new(
            vec![Line {
                text: "AB".into(),
                wrapped: false,
            }],
            2,
            Position { row: 0, col: 0 },
            0,
            "p".into(),
        );
        let cell = |glyph| json!([glyph,false,[23,145,201],[5,9,17],true,false,{"italic":true,"underline":"curly","underline_color":[7,8,9]}]);
        c.preserve_live_styles(
            &json!({"panes":[{"id":"p","grid":{"cols":2,"cells":[[cell("A"),cell("B")]]}}]}),
        );
        c.mark();
        for col in 0..2 {
            c.point(0, col);
            let out = c.render(3, 8);
            assert!(out.contains("38;2;23;145;201;48;2;5;9;17"));
            assert!(out.contains(";1;7;3;4:3;58;2;7;8;9m"));
            assert!(!out.contains("\x1b[2J"));
            assert!(out.contains(super::super::STATUS_STYLE));
            assert!(
                out.contains("\x1b[2;1H\x1b[0m\x1b[K"),
                "unused rows cleared"
            );
        }
        assert_eq!(c.selected(100).unwrap(), "AB");
    }
    #[test]
    fn stale_live_styles_are_not_attached_to_different_snapshot_text() {
        use serde_json::json;
        let mut c = model();
        c.preserve_live_styles(&json!({"panes":[{"id":c.pane,"grid":{"cols":c.cols,"cells":[[["different",false,1,4]]]}}]}));
        assert!(c.live_styles.is_empty());
    }
    fn model() -> Copy {
        Copy::new(
            vec![
                Line {
                    text: "A界e\u{301}🧪Z".into(),
                    wrapped: true,
                },
                Line {
                    text: "NEXT tail".into(),
                    wrapped: false,
                },
                Line {
                    text: "短".into(),
                    wrapped: false,
                },
            ],
            12,
            Position { row: 0, col: 0 },
            0,
            "pane".into(),
        )
    }
    #[test]
    fn unicode_marks_softwrap_and_rectangles() {
        let mut c = model();
        c.goto(0, 1);
        c.anchor = Some(c.cursor);
        c.goto(0, 5);
        assert_eq!(c.selected(100).unwrap(), "界e\u{301}🧪");
        c.goto(1, 3);
        assert_eq!(c.selected(100).unwrap(), "界e\u{301}🧪ZNEXT");
        c.rectangle = true;
        c.anchor = Some(Position { row: 1, col: 2 });
        c.goto(2, 4);
        assert_eq!(c.selected(100).unwrap(), "XT \n   ");
        assert!(c.selected(2).is_err());
    }
    #[test]
    fn search_across_wrap_and_combining() {
        let mut c = model();
        c.search = Some(("ZNEXT".into(), false));
        c.find(false);
        assert_eq!(c.cursor, Position { row: 0, col: 6 });
        c.search = Some(("\u{301}".into(), false));
        c.find(false);
        assert_eq!(c.cursor, Position { row: 0, col: 3 });
        c.search = Some(("not present".into(), false));
        let before = c.cursor;
        c.find(false);
        assert_eq!(c.cursor, before);
    }
    #[test]
    fn arrows_counts_and_atomic_graphemes() {
        let mut c = model();
        c.goto(0, 1);
        c.key(&Key::Sequence("\x1b[C".into()), 4);
        assert_eq!(c.cursor.col, 3);
        c.key(&Key::Sequence("\x1b[D".into()), 4);
        assert_eq!(c.cursor.col, 1);
        c.key(&Key::Char('2'), 4);
        c.key(&Key::Char('j'), 4);
        assert_eq!(c.cursor.row, 2);
        assert_eq!(
            keys("é\x1bOA\x01["),
            vec![
                Key::Char('é'),
                Key::Sequence("\x1bOA".into()),
                Key::Char('\x01'),
                Key::Char('[')
            ]
        );
        let mut s = "e\u{301}🧪".into();
        backspace(&mut s);
        assert_eq!(s, "e\u{301}");
        backspace(&mut s);
        assert_eq!(s, "");
    }
}
