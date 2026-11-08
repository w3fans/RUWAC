use serde_json::{Value, json};

/// Reflow once per snapshot or geometry change; only visible rows become JSON.
#[derive(Default)]
pub struct History {
    lines: Vec<String>,
    rows: Vec<(String, bool)>,
    cols: usize,
    row_bytes: Vec<usize>,
    styles: Vec<(usize, usize)>,
    palette: Vec<Value>,
    frozen: bool,
    live_cells: Vec<(usize, Value)>,
}
impl History {
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
    pub fn len(&self) -> usize {
        self.rows.len()
    }
    pub fn clear(&mut self) {
        self.lines.clear();
        self.rows.clear();
        self.row_bytes.clear();
        self.styles.clear();
        self.palette.clear();
        self.cols = 0;
        self.frozen = false;
        self.live_cells.clear();
    }
    pub fn set(&mut self, text: &str) {
        self.clear();
        self.lines = text
            .strip_suffix('\n')
            .unwrap_or(text)
            .split('\n')
            .map(str::to_owned)
            .collect();
        self.cols = 0;
    }
    /// Styles are optional; old engines and plain checkpoints remain readable.
    pub fn set_styles(&mut self, text: &str, encoded: &Value) {
        self.styles.clear();
        self.palette.clear();
        let (Some(runs), Some(palette)) =
            (encoded["runs"].as_array(), encoded["palette"].as_array())
        else {
            return;
        };
        if runs.len() > 115_536
            || palette.len() > 4096
            || !palette.iter().all(|cell| {
                cell.as_array()
                    .is_some_and(|c| c.len() == 7 && c[6].is_object())
            })
        {
            return;
        }
        for run in runs {
            let Some(byte) = run[0].as_u64().and_then(|n| usize::try_from(n).ok()) else {
                self.styles.clear();
                return;
            };
            let Some(style) = run[1]
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .filter(|n| *n < palette.len())
            else {
                self.styles.clear();
                return;
            };
            if byte >= text.len()
                || !text.is_char_boundary(byte)
                || self
                    .styles
                    .last()
                    .is_some_and(|(previous, _)| *previous > byte)
            {
                self.styles.clear();
                return;
            }
            self.styles.push((byte, style));
        }
        self.palette = palette.clone();
    }
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
    pub fn unfreeze(&mut self) {
        self.frozen = false;
    }
    pub fn pin(&mut self) {
        self.frozen = true;
    }
    /// Pin selection to canonical history while retaining the starting screen's
    /// actual cell colours/attributes when its text still matches the snapshot.
    pub fn freeze(&mut self, text: &str, live: &Value) {
        self.set(text);
        self.reflow(live["cols"].as_u64().unwrap_or(80) as usize);
        self.frozen = true;
        let start = self.start(live["rows"].as_u64().unwrap_or(24) as usize, 0);
        if let Some(rows) = live["cells"].as_array() {
            for (row, cells) in rows.iter().enumerate() {
                let text = cells.as_array().map(|cells| {
                    cells
                        .iter()
                        .filter(|c| c[1] != true)
                        .filter_map(|c| c[0].as_str())
                        .collect::<String>()
                });
                if self
                    .rows
                    .get(start + row)
                    .zip(text.as_ref())
                    .is_some_and(|((line, _), text)| {
                        line.trim_end_matches(' ') == text.trim_end_matches(' ')
                    })
                {
                    self.live_cells.push((start + row, cells.clone()));
                }
            }
        }
    }
    pub fn start(&self, height: usize, offset: usize) -> usize {
        self.rows
            .len()
            .saturating_sub(height)
            .saturating_sub(offset)
    }
    pub fn point(&self, height: usize, offset: usize, row: usize, col: usize) -> (usize, usize) {
        (
            self.start(height, offset)
                .saturating_add(row)
                .min(self.rows.len().saturating_sub(1)),
            col.min(self.cols.saturating_sub(1)),
        )
    }
    pub fn visible_selection(
        &self,
        a: (usize, usize),
        z: (usize, usize),
        height: usize,
        offset: usize,
    ) -> Option<((usize, usize), (usize, usize))> {
        let (a, z) = if a <= z { (a, z) } else { (z, a) };
        let start = self.start(height, offset);
        if height == 0 || z.0 < start || a.0 >= start.saturating_add(height) {
            return None;
        }
        let first = if a.0 < start {
            (0, 0)
        } else {
            (a.0 - start, a.1)
        };
        let last = if z.0 >= start.saturating_add(height) {
            (height - 1, self.cols.saturating_sub(1))
        } else {
            (z.0 - start, z.1)
        };
        Some((first, last))
    }
    pub fn selected(&self, a: (usize, usize), z: (usize, usize)) -> String {
        let (a, z) = if a <= z { (a, z) } else { (z, a) };
        let mut result = String::new();
        let mut skip_indent = 0;
        for row in a.0..=z.0.min(self.rows.len().saturating_sub(1)) {
            let Some((text, wrapped)) = self.rows.get(row) else {
                break;
            };
            let start = if row == a.0 { a.1 } else { skip_indent };
            skip_indent = 0;
            let end = if row == z.0 {
                z.1.saturating_add(1)
            } else {
                self.cols
            };
            let mut line = String::new();
            let mut col = 0;
            let mut included = false;
            for ch in text.chars() {
                let width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
                if width > 0 {
                    included = col >= start && col < end;
                    col += width;
                }
                if included {
                    line.push(ch);
                }
            }
            if row != z.0 && !wrapped {
                result.push_str(line.trim_end_matches(' '));
                // Only screen rows whose styles survived the frozen snapshot
                // qualify. Unstyled history keeps every genuine hard newline.
                let next_end = if row + 1 == z.0 {
                    z.1.saturating_add(1)
                } else {
                    self.cols
                };
                let continuation = self
                    .live_cells
                    .iter()
                    .find(|(n, _)| *n == row)
                    .zip(self.live_cells.iter().find(|(n, _)| *n == row + 1))
                    .and_then(|((_, previous), (_, next))| {
                        crate::view::styled_url_continuation(previous, next, &result, next_end)
                    });
                if let Some(indent) = continuation {
                    skip_indent = indent;
                } else {
                    result.push('\n');
                }
            } else {
                result.push_str(&line);
            }
        }
        result
    }
    pub fn reflow(&mut self, cols: usize) {
        let cols = cols.max(2);
        if self.cols == cols {
            return;
        }
        self.cols = cols;
        self.live_cells.clear();
        self.rows.clear();
        self.row_bytes.clear();
        let mut byte = 0;
        for line in &self.lines {
            self.row_bytes.push(byte);
            let mut row = String::new();
            let mut width = 0;
            for ch in line.chars() {
                let n = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
                if n > 0 && width + n > cols {
                    self.rows.push((std::mem::take(&mut row), true));
                    self.row_bytes.push(byte);
                    width = 0;
                }
                row.push(ch);
                byte += ch.len_utf8();
                width += n;
            }
            self.rows.push((row, false));
            byte += 1;
        }
    }
    pub fn grid(&mut self, live: &Value, offset: usize) -> (Value, usize) {
        self.reflow(live["cols"].as_u64().unwrap_or(80) as usize);
        let height = live["rows"].as_u64().unwrap_or(24) as usize;
        let offset = offset.min(self.rows.len().saturating_sub(height));
        if (offset == 0 && !self.frozen) || self.is_empty() {
            return (live.clone(), 0);
        }
        let end = self.rows.len().saturating_sub(offset);
        let begin = end.saturating_sub(height);
        let mut grid = Value::Object(
            live.as_object()
                .into_iter()
                .flatten()
                .filter(|(key, _)| key.as_str() != "cells" && key.as_str() != "wrapped")
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        );
        let mut cells = Vec::new();
        let mut wrapped = Vec::new();
        for (index, (line, soft)) in self.rows[begin..end].iter().enumerate() {
            if let Some((_, cells_row)) = self
                .live_cells
                .iter()
                .find(|(row, _)| *row == begin + index)
            {
                cells.push(cells_row.clone());
                wrapped.push(*soft);
                continue;
            }
            let mut row: Vec<Value> = Vec::new();
            for (local_byte, ch) in line.char_indices() {
                let width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
                if width == 0 {
                    if let Some(last) = row.iter_mut().rev().find(|c| c[1] != true) {
                        last[0] =
                            json!(last[0].as_str().unwrap_or("").to_owned() + &ch.to_string());
                    }
                    continue;
                }
                let byte = self.row_bytes[begin + index] + local_byte;
                let n = self.styles.partition_point(|(at, _)| *at <= byte);
                let mut cell = n
                    .checked_sub(1)
                    .map(|n| self.palette[self.styles[n].1].clone())
                    .unwrap_or_else(|| json!(["", false, 256, 256, false, false, {}]));
                cell[0] = json!(ch.to_string());
                cell[1] = json!(false);
                cell[6]["width"] = json!(width);
                row.push(cell.clone());
                for _ in 1..width {
                    cell[0] = json!("");
                    cell[1] = json!(true);
                    row.push(cell.clone());
                }
            }
            cells.push(json!(row));
            wrapped.push(*soft);
        }
        grid["cells"] = json!(cells);
        grid["wrapped"] = json!(wrapped);
        grid["cursor"]["visible"] = json!(false);
        (grid, offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn history_styles_survive_scroll_reflow_and_plain_copy() {
        let text = "ž界e\u{301}fg\nplain\nlast\n";
        let mut h = History::default();
        h.set(text);
        let red = json!(["", false, 1, 256, true, false, {}]);
        let normal = json!(["", false, 256, 256, false, false, {}]);
        h.set_styles(
            text,
            &json!({"palette":[red,normal],"runs":[[0,0],["ž界e\u{301}fg\n".len(),1]]}),
        );
        let mut live = json!({"rows":2,"cols":4,"cursor":{"visible":true}});
        let grid = h.grid(&live, 3).0;
        assert_eq!(grid["cells"][0][0][2], 1);
        assert_eq!(grid["cells"][0][1][2], 1);
        assert_eq!(grid["cells"][0][2][1], true);
        assert_eq!(grid["cells"][0][3][0], "e\u{301}");
        assert_eq!(h.selected((0, 0), (1, 1)), "ž界e\u{301}fg");
        live["cols"] = json!(8);
        let grid = h.grid(&live, 1).0;
        assert_eq!(grid["cells"][0][0][2], 1);
        assert_eq!(grid["cells"][1][0][2], 256);
        h.set_styles(text, &json!({"palette":[red],"runs":[[1,0]]})); // inside UTF-8 character
        assert!(h.styles.is_empty());
        assert_eq!(h.grid(&live, 1).0["cells"][0][0][2], 256);
    }
    #[test]
    fn selection_keeps_absolute_rows_across_viewport_changes() {
        let mut h = History::default();
        h.set("one\ntwo\nthree\nfour\n");
        h.reflow(8);
        assert_eq!(h.len(), 4);
        let a = h.point(2, 0, 1, 3);
        let z = h.point(2, 2, 0, 0);
        assert_eq!(h.selected(a, z), "one\ntwo\nthree\nfour");
        assert_eq!(h.visible_selection(a, z, 2, 0), Some(((0, 0), (1, 3))));
        assert_eq!(h.visible_selection(a, z, 2, 2), Some(((0, 0), (1, 7))));
        assert_eq!(h.selected(z, a), h.selected(a, z));
    }
    #[test]
    fn selection_preserves_soft_wraps_wide_and_combining_characters() {
        let mut h = History::default();
        h.set("ab界e\u{301}fg\nlast\n");
        h.reflow(4);
        assert_eq!(h.selected((0, 2), (1, 0)), "界e\u{301}");
        assert_eq!(h.selected((1, 0), (2, 3)), "e\u{301}fg\nlast");
        assert_eq!(h.visible_selection((2, 0), (2, 3), 1, 2), None);
    }
    #[test]
    fn frozen_view_retains_starting_screen_colours_while_live_output_changes() {
        let cells = |s: &str, color: u32| {
            json!(
                s.chars()
                    .map(|ch| json!([ch.to_string(), false, color, 256, false, false, {}]))
                    .collect::<Vec<_>>()
            )
        };
        let live = json!({"rows":2,"cols":8,"cells":[cells("three",1),cells("four",2)],"cursor":{"visible":true}});
        let mut h = History::default();
        h.freeze("one\ntwo\nthree\nfour\n", &live);
        assert_eq!(h.grid(&live, 0).0["cells"], live["cells"]);
        let mut newer = live.clone();
        newer["cells"][0] = cells("changed", 3);
        assert_eq!(h.grid(&newer, 0).0["cells"], live["cells"]);
        h.unfreeze();
        assert_eq!(h.grid(&newer, 0).0, newer);
    }
    #[test]
    fn long_commands_reflow_without_losing_text_or_hard_line_boundaries() {
        let mut h = History::default();
        h.set("0123456789abcdef\nnext\nlast");
        h.reflow(6);
        assert_eq!(
            h.rows,
            vec![
                ("012345".into(), true),
                ("6789ab".into(), true),
                ("cdef".into(), false),
                ("next".into(), false),
                ("last".into(), false)
            ]
        );
        assert_eq!(
            h.rows.iter().map(|(s, _)| s.as_str()).collect::<String>(),
            "0123456789abcdefnextlast"
        );
        h.reflow(10);
        assert_eq!(h.rows[0], ("0123456789".into(), true));
        assert_eq!(h.rows[1], ("abcdef".into(), false));
    }
    #[test]
    fn wide_and_combining_characters_keep_geometry_and_soft_wraps() {
        let live = json!({"rows":2,"cols":4,"cells":[[["live"]]],"cursor":{"visible":true}});
        let mut h = History::default();
        h.set("ab界e\u{301}fg\nnew\nlast");
        let (g, offset) = h.grid(&live, 2);
        assert_eq!(offset, 2);
        assert_eq!(g["cells"][0][2][0], "界");
        assert_eq!(g["cells"][0][3][1], true);
        assert_eq!(g["cells"][1][0][0], "e\u{301}");
        assert_eq!(g["wrapped"], json!([true, false]));
        assert_eq!(g["cursor"]["visible"], false);
        assert_eq!(h.grid(&live, 0).0, live);
    }
}
