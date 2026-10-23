use serde_json::{Value, json};

/// Reflow once per snapshot or geometry change; only visible rows become JSON.
#[derive(Default)]
pub struct History {
    lines: Vec<String>,
    rows: Vec<(String, bool)>,
    cols: usize,
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
        self.cols = 0;
    }
    pub fn set(&mut self, text: &str) {
        self.lines = text.split('\n').map(str::to_owned).collect();
        self.cols = 0;
    }
    pub fn reflow(&mut self, cols: usize) {
        let cols = cols.max(2);
        if self.cols == cols {
            return;
        }
        self.cols = cols;
        self.rows.clear();
        for line in &self.lines {
            let mut row = String::new();
            let mut width = 0;
            for ch in line.chars() {
                let n = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
                if n > 0 && width + n > cols {
                    self.rows.push((std::mem::take(&mut row), true));
                    width = 0;
                }
                row.push(ch);
                width += n;
            }
            self.rows.push((row, false));
        }
    }
    pub fn grid(&mut self, live: &Value, offset: usize) -> (Value, usize) {
        self.reflow(live["cols"].as_u64().unwrap_or(80) as usize);
        let height = live["rows"].as_u64().unwrap_or(24) as usize;
        let offset = offset.min(self.rows.len().saturating_sub(height));
        if offset == 0 || self.is_empty() {
            return (live.clone(), 0);
        }
        let end = self.rows.len().saturating_sub(offset);
        let begin = end.saturating_sub(height);
        let mut grid = live.clone();
        let mut cells = Vec::new();
        let mut wrapped = Vec::new();
        for (line, soft) in &self.rows[begin..end] {
            let mut row: Vec<Value> = Vec::new();
            for ch in line.chars() {
                let width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
                if width == 0 {
                    if let Some(last) = row.iter_mut().rev().find(|c| c[1] != true) {
                        last[0] =
                            json!(last[0].as_str().unwrap_or("").to_owned() + &ch.to_string());
                    }
                    continue;
                }
                row.push(json!([ch.to_string(), false, 256, 256, false, false, {}]));
                for _ in 1..width {
                    row.push(json!(["", true, 256, 256, false, false, {}]));
                }
            }
            cells.push(row);
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
