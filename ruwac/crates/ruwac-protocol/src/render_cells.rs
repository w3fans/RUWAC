//! Opt-in full-frame palette cells. Decode before any drawing/history consumer.
use crate::{MAX_RENDER_FRAME, TerminalLimits};
use serde_json::{Value, json};
use std::io;

pub fn valid_cell(cell: &Value) -> bool {
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

fn valid_color(c: &Value) -> bool {
    c.as_u64().is_some_and(|i| i <= 268)
        || c.as_array()
            .is_some_and(|a| a.len() == 3 && a.iter().all(|n| n.as_u64().is_some_and(|n| n <= 255)))
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// Validate the complete encoded frame and its expansion budget before cloning.
/// Plain frames from preserved engines pass through unchanged.
pub fn expand_frame(frame: &mut Value) -> io::Result<()> {
    let Some(panes) = frame["panes"].as_array() else {
        return Ok(());
    };
    let mut total_cells = 0usize;
    let mut expanded_bytes = 0usize;
    for pane in panes {
        let grid = &pane["grid"];
        let Some(encoding) = grid.get("cell_encoding") else {
            continue;
        };
        if encoding != "palette-v1" || grid.get("cells").is_some() {
            return Err(invalid("Unknown or ambiguous render cell encoding"));
        }
        let rows = grid["rows"]
            .as_u64()
            .and_then(|n| u16::try_from(n).ok())
            .ok_or_else(|| invalid("Invalid render rows"))?;
        let cols = grid["cols"]
            .as_u64()
            .and_then(|n| u16::try_from(n).ok())
            .ok_or_else(|| invalid("Invalid render columns"))?;
        if !TerminalLimits::CURRENT.valid(rows, cols) {
            return Err(invalid("Render dimensions exceed limits"));
        }
        total_cells += rows as usize * cols as usize;
        if total_cells > TerminalLimits::CURRENT.cells {
            return Err(invalid("Render expansion exceeds cell budget"));
        }
        let palette = grid["cell_palette"]
            .as_array()
            .ok_or_else(|| invalid("Missing render palette"))?;
        if palette.is_empty()
            || palette.len() > rows as usize * cols as usize
            || !palette.iter().all(valid_cell)
        {
            return Err(invalid("Invalid render palette cells"));
        }
        let sizes: Vec<usize> = palette
            .iter()
            .map(|cell| serde_json::to_vec(cell).map(|v| v.len() + 1))
            .collect::<Result<_, _>>()?;
        let indices = grid["cell_indices"]
            .as_array()
            .ok_or_else(|| invalid("Missing render indices"))?;
        if indices.len() != rows as usize {
            return Err(invalid("Invalid render row count"));
        }
        for row in indices {
            let row = row
                .as_array()
                .filter(|row| row.len() == cols as usize)
                .ok_or_else(|| invalid("Invalid render column count"))?;
            for index in row {
                let index = index
                    .as_u64()
                    .and_then(|n| usize::try_from(n).ok())
                    .filter(|i| *i < palette.len())
                    .ok_or_else(|| invalid("Invalid render palette index"))?;
                expanded_bytes += sizes[index];
                if expanded_bytes > MAX_RENDER_FRAME {
                    return Err(invalid("Render expansion exceeds byte budget"));
                }
            }
        }
    }
    let Some(panes) = frame["panes"].as_array_mut() else {
        return Ok(());
    };
    for pane in panes {
        let Some(grid) = pane.get_mut("grid") else {
            continue;
        };
        if grid.get("cell_encoding").is_none() {
            continue;
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
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame() -> Value {
        json!({"revision":42,"panes":[{"id":"pane-fixture","grid":{"rows":1,"cols":2,"cursor":{"visible":true},"cell_encoding":"palette-v1","cell_palette":[["界",false,[1,2,3],257,true,false,{"italic":true,"dim":false,"hidden":false,"strike":false,"width":2,"underline":"curly","underline_color":[4,5,6]}],["",true,256,257,false,false]],"cell_indices":[[0,1]]}}]})
    }
    #[test]
    fn exact_glyph_styles_and_metadata_expand() {
        let mut value = frame();
        let palette = value["panes"][0]["grid"]["cell_palette"].clone();
        expand_frame(&mut value).unwrap();
        assert_eq!(value["panes"][0]["grid"]["cells"], json!([palette]));
        assert_eq!(value["revision"], 42);
        assert_eq!(value["panes"][0]["grid"]["cursor"]["visible"], true);
        assert!(value["panes"][0]["grid"].get("cell_encoding").is_none());
    }
    #[test]
    fn old_plain_and_unchanged_frames_are_untouched() {
        for mut value in [
            json!({"panes":[{"ended":true}]}),
            json!({"revision":1,"unchanged":true}),
            json!({"panes":[{"grid":{"cells":[[["x",false,256,257,false,false]]]}}]}),
        ] {
            let before = value.clone();
            expand_frame(&mut value).unwrap();
            assert_eq!(value, before);
        }
    }
    #[test]
    fn corrupt_or_ambiguous_frames_fail_before_mutating() {
        for (field, replacement) in [
            ("cell_encoding", json!("unknown")),
            ("rows", json!(513)),
            ("cols", json!(1025)),
            ("cell_palette", json!([])),
            (
                "cell_palette",
                json!([["bad", false, 999, 257, false, false]]),
            ),
            ("cell_indices", json!([[0, -1]])),
            ("cell_indices", json!([[0, 2]])),
            ("cell_indices", json!([[0]])),
            ("cell_indices", json!([[0, 1], [0, 1]])),
            ("cells", json!([])),
        ] {
            let mut value = frame();
            value["panes"][0]["grid"][field] = replacement;
            let before = value.clone();
            assert!(expand_frame(&mut value).is_err(), "{field}");
            assert_eq!(value, before);
        }
    }
    #[test]
    fn expansion_byte_budget_precedes_clone_allocation() {
        let mut value = frame();
        let grid = &mut value["panes"][0]["grid"];
        grid["rows"] = json!(128);
        grid["cols"] = json!(128);
        grid["cell_palette"] = json!([["x".repeat(4096), false, 256, 257, false, false]]);
        grid["cell_indices"] = json!(vec![vec![0; 128]; 128]);
        assert!(
            expand_frame(&mut value)
                .unwrap_err()
                .to_string()
                .contains("byte budget")
        );
        assert!(value["panes"][0]["grid"].get("cells").is_none());
    }
    #[test]
    fn aggregate_cell_budget_covers_multiple_panes() {
        let mut value = frame();
        let grid = &mut value["panes"][0]["grid"];
        grid["rows"] = json!(512);
        grid["cols"] = json!(512);
        grid["cell_palette"] = json!([["x", false, 256, 257, false, false]]);
        grid["cell_indices"] = json!(vec![vec![0; 512]; 512]);
        let second = value["panes"][0].clone();
        value["panes"].as_array_mut().unwrap().push(second);
        assert!(
            expand_frame(&mut value)
                .unwrap_err()
                .to_string()
                .contains("cell budget")
        );
    }
    #[test]
    fn shared_clients_request_compact_but_accept_old_plain() {
        let old = json!({"panes":[]});
        let result = crate::client_call("client.render", json!({}), |method, params| {
            assert_eq!(method, "client.render");
            assert_eq!(params["cell_encoding"], "palette-v1");
            Ok(old.clone())
        })
        .unwrap();
        assert_eq!(result, old);
        let result = crate::client_call("client.render", json!({}), |_, _| Ok(frame())).unwrap();
        assert_eq!(result["panes"][0]["grid"]["cells"][0][0][0], "界");
    }
    #[test]
    fn split_close_rerender_also_decodes() {
        let dead = crate::id("pane");
        let live = crate::id("pane");
        let mut count = 0;
        let value = crate::client_call("client.render", json!({}), |method, params| {
            count += 1;
            match count {
                1 => Ok(json!({"panes":[{"id":dead,"ended":true},{"id":live,"ended":false}]})),
                2 => {
                    assert_eq!(method, "pane.close");
                    Ok(json!({"ok":true}))
                }
                3 => {
                    assert_eq!(params["cell_encoding"], "palette-v1");
                    Ok(frame())
                }
                _ => panic!("unbounded repair"),
            }
        })
        .unwrap();
        assert_eq!(value["panes"][0]["grid"]["cells"][0][0][0], "界");
    }
}
