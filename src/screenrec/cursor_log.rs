//! The cursor log written next to a screen recording, so a later zoom
//! editor can place zooms and redraw the cursor without recording again.
//!
//! Format: UTF-8 JSON lines, file `<video name>.cursor.jsonl`, named in the
//! sidecar's `"cursor_log"`.
//!
//! - Line 1, the header:
//!   `{"v":1,"kind":"vale-cursor-log","fps":30,"source":[1920,1080],"video":[1920,1080]}`.
//!   `source` is the captured area in physical pixels (the region, not the
//!   monitor, when a region is recorded); `video` the encoded size.
//! - Every other line is one array, `t` in milliseconds of video time (pauses
//!   left out, so it lines up with the MP4), `x`/`y` in source pixels from the
//!   source's top-left (they may fall outside it):
//!   - `[t,x,y]` the cursor moved there (only written when it moved);
//!   - `[t,x,y,"ld"]` / `"lu"` left button down / up; `"rd"`/`"ru"` right,
//!     `"md"`/`"mu"` middle;
//!   - `[t,"z",level]` the zoom went to `level` (`0` = off), as recorded.
//!
//! Lines a reader does not know are skipped (`parse_line` returns `None`).
//! About 30 bytes per moving frame: ~6 MB for an hour of constant motion.

use std::io::{self, Write};

use serde_json::Value;

pub const KIND: &str = "vale-cursor-log";
pub const EXTENSION: &str = "cursor.jsonl";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
    Middle,
}

impl Button {
    fn letter(self) -> char {
        match self {
            Button::Left => 'l',
            Button::Right => 'r',
            Button::Middle => 'm',
        }
    }

    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    fn from_letter(c: char) -> Option<Button> {
        match c {
            'l' => Some(Button::Left),
            'r' => Some(Button::Right),
            'm' => Some(Button::Middle),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Entry {
    Move {
        t: u64,
        x: i32,
        y: i32,
    },
    Down {
        t: u64,
        x: i32,
        y: i32,
        button: Button,
    },
    Up {
        t: u64,
        x: i32,
        y: i32,
        button: Button,
    },
    /// `level` 0 means the zoom went off.
    Zoom {
        t: u64,
        level: f64,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub fps: u32,
    pub source: (u32, u32),
    pub video: (u32, u32),
}

pub fn encode_header(h: &Header) -> String {
    format!(
        "{{\"v\":1,\"kind\":\"{KIND}\",\"fps\":{},\"source\":[{},{}],\"video\":[{},{}]}}",
        h.fps, h.source.0, h.source.1, h.video.0, h.video.1
    )
}

// The log's reader: phase 4 reads a recording's log back; until then the
// debug CLI and the tests do.
#[cfg_attr(not(debug_assertions), allow(dead_code))]
pub fn parse_header(line: &str) -> Option<Header> {
    let v: Value = serde_json::from_str(line).ok()?;
    if v.get("kind")?.as_str()? != KIND || v.get("v")?.as_u64()? != 1 {
        return None;
    }
    let pair = |key: &str| -> Option<(u32, u32)> {
        let a = v.get(key)?.as_array()?;
        Some((a.first()?.as_u64()? as u32, a.get(1)?.as_u64()? as u32))
    };
    Some(Header {
        fps: v.get("fps")?.as_u64()? as u32,
        source: pair("source")?,
        video: pair("video")?,
    })
}

/// A level with at most two decimals, without a trailing `.0`.
fn level_text(level: f64) -> String {
    let r = (level * 100.0).round() / 100.0;
    if r.fract() == 0.0 {
        format!("{}", r as i64)
    } else {
        format!("{r}")
    }
}

pub fn encode_entry(e: &Entry) -> String {
    match *e {
        Entry::Move { t, x, y } => format!("[{t},{x},{y}]"),
        Entry::Down { t, x, y, button } => format!("[{t},{x},{y},\"{}d\"]", button.letter()),
        Entry::Up { t, x, y, button } => format!("[{t},{x},{y},\"{}u\"]", button.letter()),
        Entry::Zoom { t, level } => format!("[{t},\"z\",{}]", level_text(level)),
    }
}

#[cfg_attr(not(debug_assertions), allow(dead_code))]
pub fn parse_line(line: &str) -> Option<Entry> {
    let v: Value = serde_json::from_str(line).ok()?;
    let a = v.as_array()?;
    let t = a.first()?.as_u64()?;
    if a.get(1)?.as_str() == Some("z") {
        return Some(Entry::Zoom {
            t,
            level: a.get(2)?.as_f64()?,
        });
    }
    let x = i32::try_from(a.get(1)?.as_i64()?).ok()?;
    let y = i32::try_from(a.get(2)?.as_i64()?).ok()?;
    match a.get(3) {
        None => Some(Entry::Move { t, x, y }),
        Some(tag) => {
            let mut chars = tag.as_str()?.chars();
            let button = Button::from_letter(chars.next()?)?;
            let entry = match chars.next()? {
                'd' => Entry::Down { t, x, y, button },
                'u' => Entry::Up { t, x, y, button },
                _ => return None,
            };
            chars.next().is_none().then_some(entry)
        }
    }
}

/// Writes the log as the recording goes. Moves to the same pixel are
/// written once.
pub struct CursorLog<W: Write> {
    out: W,
    last: Option<(i32, i32)>,
}

impl<W: Write> CursorLog<W> {
    pub fn new(mut out: W, header: &Header) -> io::Result<CursorLog<W>> {
        writeln!(out, "{}", encode_header(header))?;
        Ok(CursorLog { out, last: None })
    }

    pub fn record(&mut self, e: Entry) -> io::Result<()> {
        match e {
            Entry::Move { x, y, .. } => {
                if self.last == Some((x, y)) {
                    return Ok(());
                }
                self.last = Some((x, y));
            }
            Entry::Down { x, y, .. } | Entry::Up { x, y, .. } => self.last = Some((x, y)),
            Entry::Zoom { .. } => {}
        }
        writeln!(self.out, "{}", encode_entry(&e))
    }

    pub fn finish(mut self) -> io::Result<W> {
        self.out.flush()?;
        Ok(self.out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header() -> Header {
        Header {
            fps: 30,
            source: (1920, 1080),
            video: (1280, 720),
        }
    }

    #[test]
    fn the_header_round_trips() {
        let line = encode_header(&header());
        assert_eq!(
            line,
            r#"{"v":1,"kind":"vale-cursor-log","fps":30,"source":[1920,1080],"video":[1280,720]}"#
        );
        assert_eq!(parse_header(&line), Some(header()));
        assert_eq!(parse_header(r#"{"v":2,"kind":"vale-cursor-log"}"#), None);
        assert_eq!(parse_header("[1,2,3]"), None);
    }

    #[test]
    fn entries_round_trip() {
        let all = [
            Entry::Move { t: 0, x: 10, y: -4 },
            Entry::Down {
                t: 33,
                x: 10,
                y: 20,
                button: Button::Left,
            },
            Entry::Up {
                t: 120,
                x: 11,
                y: 20,
                button: Button::Right,
            },
            Entry::Down {
                t: 130,
                x: 1,
                y: 2,
                button: Button::Middle,
            },
            Entry::Zoom { t: 500, level: 2.5 },
            Entry::Zoom { t: 900, level: 0.0 },
        ];
        for e in all {
            assert_eq!(parse_line(&encode_entry(&e)), Some(e), "{e:?}");
        }
        assert_eq!(encode_entry(&all[1]), r#"[33,10,20,"ld"]"#);
        assert_eq!(encode_entry(&all[4]), r#"[500,"z",2.5]"#);
        assert_eq!(
            encode_entry(&Entry::Zoom { t: 1, level: 2.0 }),
            r#"[1,"z",2]"#
        );
    }

    #[test]
    fn unknown_lines_are_skipped() {
        assert_eq!(parse_line(r#"[1,2,3,"xd"]"#), None);
        assert_eq!(parse_line(r#"[1,2,3,"lx"]"#), None);
        assert_eq!(parse_line(r#"[1,2,3,"ldd"]"#), None);
        assert_eq!(parse_line(r#"{"a":1}"#), None);
        assert_eq!(parse_line("garbage"), None);
        assert_eq!(parse_line("[-1,2,3]"), None);
    }

    #[test]
    fn the_log_skips_moves_to_the_same_pixel() {
        let mut log = CursorLog::new(Vec::new(), &header()).unwrap();
        log.record(Entry::Move { t: 0, x: 5, y: 5 }).unwrap();
        log.record(Entry::Move { t: 33, x: 5, y: 5 }).unwrap();
        log.record(Entry::Move { t: 66, x: 6, y: 5 }).unwrap();
        log.record(Entry::Down {
            t: 70,
            x: 6,
            y: 5,
            button: Button::Left,
        })
        .unwrap();
        log.record(Entry::Move { t: 99, x: 6, y: 5 }).unwrap();
        let text = String::from_utf8(log.finish().unwrap()).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 4);
        assert_eq!(parse_header(lines[0]), Some(header()));
        assert_eq!(lines[1], "[0,5,5]");
        assert_eq!(lines[2], "[66,6,5]");
        assert_eq!(lines[3], r#"[70,6,5,"ld"]"#);
    }
}
