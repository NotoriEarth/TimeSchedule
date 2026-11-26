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
/// Reads one row into `buf` with its line end. A row whose text, without `\r\n` or
/// `\n`, is longer than `LINE_LIMIT` is skipped and handed back empty. `false` at the
/// end of the file and for a last row with no line end, unless that row is over the
/// limit: it is skipped like a long finished row (a known limit in `docs/daily-record.md`).
pub(crate) fn line(input: &mut impl BufRead, buf: &mut Vec<u8>) -> Result<bool, String> {
    buf.clear();
    // Room for the text, `\r` and `\n`: a row of exactly the limit is not long,
    // whichever line end it has.
    input
        .take(LINE_LIMIT + 2)
        .read_until(b'\n', buf)
        .map_err(|e| e.to_string())?;
    let end = match buf.as_slice() {
        [.., b'\r', b'\n'] => 2,
        [.., b'\n'] => 1,
        _ => 0,
    };
    if (buf.len() - end) as u64 > LINE_LIMIT {
        if end == 0 {
            input.skip_until(b'\n').map_err(|e| e.to_string())?;
        }
        buf.clear();
        return Ok(true);
    }
    Ok(end > 0)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A row whose text, without its line end, is `body` bytes long, then a short row.
    fn rows(body: usize, end: &str) -> Vec<u8> {
        let mut file = vec![b'x'; body];
        file.extend_from_slice(end.as_bytes());
        file.extend_from_slice(b"next");
        file.extend_from_slice(end.as_bytes());
        file
    }

    #[test]
    fn the_limit_is_on_the_text_without_its_line_end() {
        let limit = LINE_LIMIT as usize;
        let mut wrong = Vec::new();
        for end in ["\n", "\r\n"] {
            for (body, kept) in [
                (limit - 1, true),
                (limit, true),
                (limit + 1, false),
                (limit + 2, false),
            ] {
                let file = rows(body, end);
                let mut input = &file[..];
                let mut buf = Vec::new();
                let first = (line(&mut input, &mut buf).unwrap(), buf.len());
                let expected = (true, if kept { body + end.len() } else { 0 });
                let second = (line(&mut input, &mut buf).unwrap(), buf.clone());
                let after = line(&mut input, &mut buf).unwrap();
                if first != expected || second != (true, format!("next{end}").into_bytes()) || after
                {
                    wrong.push(format!("{body} bytes, {end:?}: read {first:?}"));
                }
            }
        }
        assert!(wrong.is_empty(), "{wrong:#?}");
    }

    #[test]
    fn a_line_end_alone_is_a_row_and_a_second_cr_is_text() {
        let limit = LINE_LIMIT as usize;
        let mut buf = Vec::new();
        for end in ["\n", "\r\n"] {
            let mut input = end.as_bytes();
            assert!(line(&mut input, &mut buf).unwrap());
            assert_eq!(buf, end.as_bytes());
        }
        // `\r\r\n` is one `\r` of text and a CRLF line end.
        let mut file = vec![b'x'; limit];
        file.extend_from_slice(b"\r\r\nnext\n");
        let mut input = &file[..];
        assert!(line(&mut input, &mut buf).unwrap());
        assert!(buf.is_empty());
        assert!(line(&mut input, &mut buf).unwrap());
        assert_eq!(buf, b"next\n");
    }

    #[test]
    fn a_last_row_with_no_line_end_is_unfinished_unless_it_is_over_the_limit() {
        let limit = LINE_LIMIT as usize;
        let mut buf = Vec::new();
        let file = vec![b'x'; limit];
        let mut input = &file[..];
        assert!(!line(&mut input, &mut buf).unwrap());
        assert_eq!(buf.len(), limit);

        let file = vec![b'x'; limit + 1];
        let mut input = &file[..];
        assert!(line(&mut input, &mut buf).unwrap());
        assert!(buf.is_empty());
        assert!(!line(&mut input, &mut buf).unwrap());
    }
}
