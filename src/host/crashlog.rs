//! Crash safety for arena resets: every recorded original block is appended to
//! `resets/<game>.jsonl` (first line: the world). A clean reset deletes the file; on
//! start-up any file left behind is restored. File IO only; no host calls.

use std::io::Write;

use serde::{Deserialize, Serialize};

use crate::model::BlockPos;

#[derive(Serialize, Deserialize)]
struct Header {
    world: String,
}

#[derive(Serialize, Deserialize)]
struct Entry {
    x: i32,
    y: i32,
    z: i32,
    s: u16,
}

fn dir() -> String {
    format!("{}/resets", super::storage::data_dir())
}

fn path(game: &str) -> String {
    format!("{}/{game}.jsonl", dir())
}

pub fn append(game: &str, world: &str, pos: BlockPos, state: u16) {
    let p = path(game);
    let new = std::fs::metadata(&p).is_err();
    let _ = std::fs::create_dir_all(dir());
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&p);
    let Ok(mut f) = file else { return };
    let mut line = String::new();
    if new {
        line.push_str(
            &serde_json::to_string(&Header {
                world: world.into(),
            })
            .unwrap_or_default(),
        );
        line.push('\n');
    }
    line.push_str(
        &serde_json::to_string(&Entry {
            x: pos.x,
            y: pos.y,
            z: pos.z,
            s: state,
        })
        .unwrap_or_default(),
    );
    line.push('\n');
    let _ = f.write_all(line.as_bytes());
}

pub fn remove(game: &str) {
    let _ = std::fs::remove_file(path(game));
}

/// A leftover log: (game, world, blocks in restore order).
pub type Leftover = (String, String, Vec<(BlockPos, u16)>);

pub fn leftovers() -> Vec<Leftover> {
    let Ok(entries) = std::fs::read_dir(dir()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let Some(game) = name.strip_suffix(".jsonl") else {
            continue;
        };
        let Ok(text) = std::fs::read_to_string(e.path()) else {
            continue;
        };
        if let Some((world, blocks)) = parse(&text) {
            out.push((game.to_string(), world, blocks));
        }
    }
    out
}

fn parse(text: &str) -> Option<(String, Vec<(BlockPos, u16)>)> {
    let mut lines = text.lines();
    let header: Header = serde_json::from_str(lines.next()?).ok()?;
    let mut seen = std::collections::HashSet::new();
    let mut blocks: Vec<(BlockPos, u16)> = lines
        .filter_map(|l| serde_json::from_str::<Entry>(l).ok()) // a torn last line is skipped
        .map(|e| (BlockPos::new(e.x, e.y, e.z), e.s))
        .filter(|(p, _)| seen.insert(*p))
        .collect();
    blocks.reverse();
    Some((header.world, blocks))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_skips_torn_lines_and_keeps_first_state() {
        let text = "{\"world\":\"w\"}\n{\"x\":1,\"y\":2,\"z\":3,\"s\":5}\n{\"x\":1,\"y\":2,\"z\":3,\"s\":9}\n{\"x\":4,\"y\":";
        let (w, b) = parse(text).unwrap();
        assert_eq!(w, "w");
        assert_eq!(b, vec![(BlockPos::new(1, 2, 3), 5)]);
    }
}
