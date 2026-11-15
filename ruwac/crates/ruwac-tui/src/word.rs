//! Map canonical UTF-8 word boundaries to copy-renderer display columns.
use unicode_width::UnicodeWidthStr;

#[path = "word_boundary.rs"]
mod boundary;
use boundary::{Point, byte_span};

/// Select one Unicode word-boundary segment, joining only real soft wraps.
/// Input is (canonical row, UTF-8 byte offset); output endpoints are inclusive
/// display columns in the copy renderer, independent of live terminal widths.
pub fn selection(rows: &[(&str, bool)], at: Point) -> Option<(Point, Point)> {
    let text = rows.get(at.0)?.0;
    if at.1 >= text.len() {
        return None;
    }
    let mut first = at.0;
    while first > 0 && rows[first - 1].1 {
        first -= 1;
    }
    let mut last = at.0;
    while rows[last].1 && last + 1 < rows.len() {
        last += 1;
    }
    let mut line = String::new();
    let mut byte = at.1;
    for (row, (text, _)) in rows.iter().enumerate().take(last + 1).skip(first) {
        if row < at.0 {
            byte += text.len();
        }
        line.push_str(text);
    }
    let (start, end) = byte_span(&line, byte)?;
    let mut offset = 0;
    let mut a = None;
    let mut z = None;
    for (row, (text, _)) in rows.iter().enumerate().take(last + 1).skip(first) {
        if (offset..offset + text.len()).contains(&start) {
            a = Some((row, UnicodeWidthStr::width(&text[..start - offset])));
        }
        if offset < end && end <= offset + text.len() {
            z = Some((
                row,
                UnicodeWidthStr::width(&text[..end - offset]).checked_sub(1)?,
            ));
        }
        offset += text.len();
    }
    Some((a?, z?))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn words_and_punctuation_use_unicode_boundaries() {
        let rows = [("before alpha_beta-42 after", false)];
        assert_eq!(selection(&rows, (0, 10)), Some(((0, 7), (0, 16))));
        assert_eq!(selection(&rows, (0, 17)), Some(((0, 17), (0, 17))));
        assert_eq!(selection(&rows, (0, 18)), Some(((0, 18), (0, 19))));
    }
    #[test]
    fn combined_accents_and_wide_continuations_remain_whole() {
        assert_eq!(
            selection(&[("a čebe\u{301}la z", false)], (0, 5)),
            Some(((0, 2), (0, 7)))
        );
        assert_eq!(
            selection(&[("a 界 z", false)], (0, 3)),
            Some(((0, 2), (0, 3)))
        );
        assert_eq!(
            selection(&[("a 👩‍💻 z", false)], (0, 3)),
            Some(((0, 2), (0, 3)))
        );
    }
    #[test]
    fn any_cell_fragment_of_a_joined_emoji_selects_the_entire_word() {
        let text = "before 👩‍💻 after";
        for byte in ["before ".len(), "before 👩‍".len()] {
            assert_eq!(
                selection(&[(text, false)], (0, byte)),
                Some(((0, 7), (0, 8)))
            );
        }
        assert_eq!(
            selection(&[("before 👩‍", true), ("💻 after", false)], (1, 0)),
            Some(((0, 7), (1, 1)))
        );
    }
    #[test]
    fn only_soft_wraps_join_words() {
        let rows = [("ab", true), ("cde ", false), ("fg", false)];
        assert_eq!(selection(&rows, (1, 1)), Some(((0, 0), (1, 2))));
        assert_eq!(selection(&rows, (2, 0)), Some(((2, 0), (2, 1))));
        assert_eq!(
            selection(&[("ab", false), ("cd", false)], (1, 0)),
            Some(((1, 0), (1, 1)))
        );
    }
    #[test]
    fn blank_cells_and_outside_coordinates_are_bounded() {
        assert_eq!(
            selection(&[("x   y", false)], (0, 2)),
            Some(((0, 1), (0, 3)))
        );
        for at in [(0, 0), (1, 0), (0, usize::MAX)] {
            assert_eq!(selection(&[("", false)], at), None);
        }
    }
}
