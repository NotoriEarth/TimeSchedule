use std::{
    collections::{HashSet, VecDeque},
    io::{BufRead, Read},
    path::Path,
};
pub(super) struct Header {
    pub id: String,
    pub cwd: String,
    pub date: String,
}
pub(super) struct Transcript {
    pub id: String,
    pub date: String,
    pub title: String,
    pub body: String,
}

// Skip large protocol blobs without allocating their full line. Only completed,
// user-visible chat events enter a diary record.
const LINE_LIMIT: u64 = 8 * 1024 * 1024;
pub(super) fn line(input: &mut impl BufRead, buf: &mut Vec<u8>) -> Result<bool, String> {
    buf.clear();
    input
        .take(LINE_LIMIT + 1)
        .read_until(b'\n', buf)
        .map_err(|e| e.to_string())?;
    if buf.len() as u64 > LINE_LIMIT {
        if buf.last() != Some(&b'\n') {
            input.skip_until(b'\n').map_err(|e| e.to_string())?;
        }
        buf.clear();
        return Ok(true);
    }
    Ok(buf.last() == Some(&b'\n'))
}

fn clip(s: &str, bytes: usize) -> String {
    if s.len() <= bytes {
        return s.into();
    }
    let boundary = s.floor_char_boundary(bytes);
    format!("{}… (이하 원문 참조)", &s[..boundary])
}

// Keep the start and recent turns when a transcript exceeds the diary editor's limit.
#[derive(Default)]
struct Excerpts {
    first: String,
    last: VecDeque<String>,
    last_bytes: usize,
    omitted: bool,
}
impl Excerpts {
    fn add(&mut self, block: String) {
        if self.last.is_empty() && !self.omitted && self.first.len() + block.len() <= 40_000 {
            self.first.push_str(&block);
            return;
        }
        self.last_bytes += block.len();
        self.last.push_back(block);
        while self.last_bytes > 48_000 {
            self.last_bytes -= self.last.pop_front().unwrap().len();
            self.omitted = true;
        }
    }
    fn finish(self) -> String {
        let mut s = self.first;
        if self.omitted {
            s.push_str("\n(중간 대화는 원문 참조)\n");
        }
        for b in self.last {
            s.push_str(&b);
        }
        s
    }
}

#[derive(Default)]
pub(super) struct ChatText {
    excerpts: Excerpts,
    first_request: Option<String>,
    ids: HashSet<String>,
}
impl ChatText {
    pub fn add(&mut self, id: &str, label: &str, n: usize, text: &str) {
        if text.trim().is_empty() || !self.ids.insert(id.to_owned()) {
            return;
        }
        if label == "사용자" && self.first_request.is_none() {
            self.first_request = text
                .lines()
                .map(str::trim)
                .find(|s| !s.is_empty())
                .map(|s| s.chars().take(100).collect());
        }
        let mut block = format!("\n## {label} · 원문 {n}행\n");
        for line in clip(text, 16_000).split_inclusive('\n') {
            // Quoted transcript headings are content, never provenance for a new block.
            if line.starts_with("## ") && line.contains(" · 원문 ") {
                block.push_str("> ");
            }
            block.push_str(line);
        }
        block.push('\n');

        self.excerpts.add(block);
    }
    pub fn finish(
        self,
        header: Header,
        path: &Path,
        index_title: &str,
        provider: super::Provider,
    ) -> Option<Transcript> {
        let fallback = self.first_request?;
        let name = if index_title.trim().is_empty() {
            fallback.as_str()
        } else {
            index_title.trim()
        };
        let name: String = name.chars().filter(|c| !c.is_control()).take(150).collect();
        let (prefix, origin) = match provider {
            super::Provider::Codex => ("Codex", "Windows Codex"),
            super::Provider::Claude => ("Claude Code", "Claude Code"),
        };
        Some(Transcript {
            id: provider.session_key(&header.id),
            date: header.date,
            title: format!("{prefix} 채팅 · {name}"),
            body: format!(
                "{origin} · {}\n원문: {}\n{}",
                header.id,
                path.display(),
                self.excerpts.finish()
            ),
        })
    }
}
