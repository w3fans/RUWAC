//! UTF-8 word boundaries shared by terminal frontends.
use unicode_segmentation::UnicodeSegmentation;

pub type Point = (usize, usize);

/// UTF-8 boundaries are independent of a renderer's display-cell widths.
pub fn byte_span(text: &str, byte: usize) -> Option<(usize, usize)> {
    text.split_word_bound_indices()
        .find(|(start, segment)| (*start..start + segment.len()).contains(&byte))
        .map(|(start, segment)| (start, start + segment.len()))
}
