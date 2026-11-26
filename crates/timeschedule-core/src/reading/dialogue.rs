//! The dialogue of a day and the lines of a kept original, read from the document
//! store by the rules of the day transcript.
use super::*;
use crate::{
    chat_originals::group_name,
    daily_record::{day_transcript, open_kept, project_dir, read_index, DayTranscript},
    project_records::repo_key,
};
use std::io::{BufRead, Read};

fn block_views(t: &DayTranscript) -> Vec<BlockView> {
    let date = t.date.to_string();
    t.blocks
        .iter()
        .map(|b| BlockView {
            date: date.clone(),
            number: b.number,
            at: b.at.to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
            speaker: b.speaker.label(),
            source: t.sources[b.source].tag.clone(),
            file: t.sources[b.source].files[b.file].clone(),
            line: b.line,
            clipped: b.clipped,
            text: b.text.clone(),
        })
        .collect()
}

fn project_name(t: &DayTranscript) -> ProjectName {
    ProjectName {
        id: t.project_id.clone(),
        title: t.title.clone(),
    }
}

/// The blocks `pick` names of the dialogue of `project` on `date`, from `offset` on.
/// `project` is a project ID or the absolute path of its folder, as for the day
/// transcript, which this reads.
pub fn read_day(
    vault: &Path,
    project: &str,
    date: NaiveDate,
    pick: Pick,
    offset: usize,
    budget: usize,
) -> Result<DayView, String> {
    let t = day_transcript(vault, project, date)?;
    let all = block_views(&t);
    let (focus, chosen): (Option<usize>, Vec<BlockView>) = match pick {
        Pick::All => (None, all),
        Pick::Matching(query) => {
            let query = query_words(query)?;
            (
                None,
                all.into_iter()
                    .filter(|b| contains(&b.text, query))
                    .collect(),
            )
        }
        Pick::Around {
            file,
            line,
            context,
        } => {
            let found = all
                .iter()
                .filter(|b| b.file == file && b.line <= line)
                .map(|b| b.line)
                .max()
                .ok_or_else(|| {
                    format!("{date}에 이 파일의 {line}번 줄이나 그 앞에서 시작하는 블록이 없습니다: {file}")
                })?;
            // One row can make several blocks, such as two failed commands in one
            // Claude Code row: all of them are given, and the first is the focus.
            let at: Vec<usize> = (0..all.len())
                .filter(|&i| all[i].file == file && all[i].line == found)
                .collect();
            let (head, tail) = (at[0], at[at.len() - 1]);
            let number = all[head].number;
            let first = head.saturating_sub(context);
            let last = (tail + context + 1).min(all.len());
            (
                Some(number),
                all.into_iter().take(last).skip(first).collect(),
            )
        }
    };
    let total = chosen.len();
    if offset > 0 && offset >= total {
        return Err(format!("--offset이 블록 수({total}개)를 넘습니다."));
    }
    let (blocks, _) = fit(chosen.into_iter().skip(offset), budget);
    let read = offset + blocks.len();
    let (warnings, warnings_total) = capped(t.warnings.clone());
    Ok(DayView {
        project: project_name(&t),
        date: date.to_string(),
        focus,
        total,
        offset,
        blocks,
        warnings,
        warnings_total,
        next: (read < total).then_some(Offset { offset: read }),
    })
}

/// The blocks of the dialogue of `project` from `from` to `to` that hold `query`, in
/// time order. `offset` blocks of `from` are skipped. Days are read one by one until
/// the page is full, at most `MAX_SEARCH_DAYS` in one search.
pub fn search_days(
    vault: &Path,
    project: &str,
    from: NaiveDate,
    to: NaiveDate,
    query: &str,
    offset: usize,
    budget: usize,
) -> Result<SearchView, String> {
    let query = query_words(query)?;
    if from > to {
        return Err("시작 날짜가 종료 날짜보다 늦습니다.".into());
    }
    if (to - from).num_days() >= MAX_SEARCH_DAYS {
        return Err(format!(
            "대화 검색은 한 번에 {MAX_SEARCH_DAYS}일까지 읽습니다. --from과 --to를 좁히세요."
        ));
    }
    let mut view = SearchView {
        project: ProjectName {
            id: String::new(),
            title: String::new(),
        },
        from: from.to_string(),
        to: to.to_string(),
        days_read: 0,
        blocks: Vec::new(),
        warnings: Vec::new(),
        warnings_total: 0,
        next: None,
    };
    let mut warnings = Vec::new();
    let mut used = 0;
    let mut day = from;
    'days: loop {
        let t = day_transcript(vault, project, day)?;
        view.project = project_name(&t);
        view.days_read += 1;
        warnings.extend(t.warnings.iter().map(|w| format!("{day}: {w}")));
        let skip = if day == from { offset } else { 0 };
        let hits = block_views(&t)
            .into_iter()
            .filter(|b| contains(&b.text, query))
            .skip(skip);
        for (given, hit) in hits.enumerate() {
            let size = chars_of(&hit);
            // The first block of a page is given whatever its size.
            if !view.blocks.is_empty() && used + size > budget {
                view.next = Some(DayOffset {
                    from: day.to_string(),
                    offset: skip + given,
                });
                break 'days;
            }
            used += size;
            view.blocks.push(hit);
        }
        if day >= to {
            break;
        }
        day = day.succ_opt().ok_or("날짜를 넘길 수 없습니다.")?;
        // A full page reads no more days: the next page starts on the next one.
        if used >= budget {
            view.next = Some(DayOffset {
                from: day.to_string(),
                offset: 0,
            });
            break;
        }
    }
    (view.warnings, view.warnings_total) = capped(warnings);
    Ok(view)
}

/// Lines of a kept original of `project` as they are in the file, from line `line` (and
/// `offset` characters into it) on: `count` lines, held to `budget` characters. `file`
/// is a path of the project's `index.jsonl`, as the day transcript and a daily record
/// give it.
pub fn read_original(
    vault: &Path,
    project: &str,
    file: &str,
    line_number: usize,
    count: usize,
    offset: usize,
    budget: usize,
) -> Result<OriginalView, String> {
    if line_number == 0 {
        return Err("--line은 1부터 셉니다.".into());
    }
    let (dir, info) = project_dir(vault, project)?;
    let entries = read_index(&dir)?;
    let entry = entries
        .iter()
        .find(|e| e.file == file)
        .ok_or_else(|| format!("이 프로젝트의 보관 목록에 없는 파일입니다: {file}"))?;
    let mut input = open_kept(&dir, entry)?;
    let earlier = earlier_contents(&dir, file);
    // A line that is not there may be in an earlier content of the file.
    let missing = |lines: usize| {
        let mut why = format!("이 파일은 {lines}줄입니다. --line {line_number}이 없습니다.");
        match &earlier {
            Some(names) if !names.is_empty() => {
                why += &format!(
                    " 이 파일은 내용이 바뀐 적이 있고 이전 내용이 남아 있습니다: {}",
                    names.join(", ")
                );
            }
            None => why += " 이전 내용이 남아 있는지는 폴더를 읽지 못해 확인하지 못했습니다.",
            _ => {}
        }
        why
    };
    let mut buf = Vec::new();
    for skipped in 1..line_number {
        // A last line with no line end is not a line, however long.
        if !rest_of_line(&mut input)? {
            return Err(missing(skipped - 1));
        }
    }
    let mut view = OriginalView {
        project: ProjectName {
            id: info.project_id,
            title: info.title,
        },
        file: file.to_owned(),
        earlier: earlier.clone(),
        lines: Vec::new(),
        next: None,
    };
    let mut room = budget.max(1);
    let mut skip = offset;
    let mut left = count.max(1);
    let mut number = line_number;
    loop {
        let Some((text, long, lossy)) = whole_line(&mut input, &mut buf)? else {
            if view.lines.is_empty() {
                return Err(missing(number - 1));
            }
            break;
        };
        // The line is there, and no room is left for it.
        if room == 0 {
            view.next = Some(LineOffset {
                line: number,
                offset: 0,
                lines: left,
            });
            break;
        }
        let chars: Vec<char> = text.chars().collect();
        if skip > chars.len() || (skip == chars.len() && skip > 0) {
            return Err(format!(
                "--offset이 {number}번 줄 길이({}자)를 넘습니다.",
                chars.len()
            ));
        }
        let end = (skip + room).min(chars.len());
        room -= end - skip;
        let clipped = end < chars.len();
        view.lines.push(LineView {
            line: number,
            chars: chars.len(),
            offset: skip,
            text: chars[skip..end].iter().collect(),
            clipped,
            long,
            lossy,
        });
        if clipped {
            view.next = Some(LineOffset {
                line: number,
                offset: end,
                lines: left,
            });
            break;
        }
        left -= 1;
        if left == 0 {
            break;
        }
        skip = 0;
        number += 1;
    }
    Ok(view)
}

/// The next line of `input` without its line end, whether it is longer than
/// `LINE_LIMIT` and was cut there, and whether bytes that are not UTF-8 were replaced. `None` at the end of the file and for a last line
/// with no line end, which the tool may still be writing: the day transcript does not
/// count it as a line either.
fn whole_line(
    input: &mut impl BufRead,
    buf: &mut Vec<u8>,
) -> Result<Option<(String, bool, bool)>, String> {
    buf.clear();
    // Room for the line, `\r` and `\n`: a line of exactly the limit is not long,
    // whichever line end it has.
    input
        .take(LINE_LIMIT + 2)
        .read_until(b'\n', buf)
        .map_err(|e| e.to_string())?;
    if buf.last() == Some(&b'\n') {
        buf.pop();
        if buf.last() == Some(&b'\r') {
            buf.pop();
        }
    } else if buf.len() as u64 <= LINE_LIMIT + 1 || !rest_of_line(input)? {
        return Ok(None);
    }
    let long = buf.len() as u64 > LINE_LIMIT;
    if long {
        buf.truncate(LINE_LIMIT as usize);
        // Not into the middle of a letter: that would end the text with one that is
        // not in the file.
        if let Err(e) = std::str::from_utf8(buf) {
            if e.error_len().is_none() {
                buf.truncate(e.valid_up_to());
            }
        }
    }
    let text = String::from_utf8_lossy(buf);
    let lossy = matches!(text, std::borrow::Cow::Owned(_));
    Ok(Some((text.into_owned(), long, lossy)))
}

/// The earlier contents of the kept `file` below the project folder `dir`: the
/// `<name>.prev-…` files the archive keeps when it replaces a file with content that
/// does not go on from it.
fn earlier_contents(dir: &Path, file: &str) -> Option<Vec<String>> {
    let (folder, name) = file.rsplit_once('/').unwrap_or(("", file));
    let place = crate::chat_originals::kept_path(dir, file).ok()?;
    let list = std::fs::read_dir(place.parent()?).ok()?;
    let mut found = Vec::new();
    for entry in list {
        let entry = entry.ok()?;
        let other = entry.file_name().to_string_lossy().into_owned();
        let earlier = other
            .strip_prefix(name)
            .and_then(|rest| rest.strip_prefix(".prev-"))
            .is_some_and(previous_stamp);
        if earlier && entry.file_type().ok()?.is_file() {
            found.push(if folder.is_empty() {
                other
            } else {
                format!("{folder}/{other}")
            });
        }
    }
    found.sort();
    Some(found)
}

/// Whether `rest` is what the archive puts after `.prev-`: a UTC time `YYYYMMDDTHHMMSSZ`,
/// and `-<n>` when that name was taken. Another file whose name only starts the same,
/// such as `out.zst.prev-note.zst`, is not an earlier content.
fn previous_stamp(rest: &str) -> bool {
    let (stamp, n) = rest.split_once('-').unwrap_or((rest, "1"));
    let b = stamp.as_bytes();
    b.len() == 16
        && b[8] == b'T'
        && b[15] == b'Z'
        && b[..8].iter().chain(&b[9..15]).all(u8::is_ascii_digit)
        && !n.is_empty()
        && n.bytes().all(|c| c.is_ascii_digit())
}

/// Reads past the rest of a line; whether it had a line end.
fn rest_of_line(input: &mut impl BufRead) -> Result<bool, String> {
    loop {
        let seen = input.fill_buf().map_err(|e| e.to_string())?;
        if seen.is_empty() {
            return Ok(false);
        }
        if let Some(at) = seen.iter().position(|&b| b == b'\n') {
            input.consume(at + 1);
            return Ok(true);
        }
        let n = seen.len();
        input.consume(n);
    }
}

/// How many files the kept originals of `folder` list, and how many days of daily
/// records it has, from the store at `vault`. Counted, not read. 0 for a folder that is
/// not there; `None` for one that could not be read, or is reached through a link.
pub fn store_counts(vault: &Path, folder: &str) -> (Option<usize>, Option<usize>) {
    let group = group_name(&repo_key(folder));
    // Nothing is read through a link, as for the code that reads these folders. An
    // error other than "not found" is not taken for an empty folder.
    let plain = |parts: &[&str]| -> Result<Option<std::path::PathBuf>, ()> {
        let mut path = vault.to_path_buf();
        for part in parts {
            path.push(part);
            match std::fs::symlink_metadata(&path) {
                Ok(meta) if meta.file_type().is_symlink() => return Err(()),
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(_) => return Err(()),
            }
        }
        Ok(Some(path))
    };
    let files = match plain(&["originals", &group, "index.jsonl"]) {
        Ok(None) => Some(0),
        Ok(Some(index)) => std::fs::read_to_string(index)
            .ok()
            .map(|text| text.lines().filter(|l| !l.trim().is_empty()).count()),
        Err(()) => None,
    };
    let days = match plain(&["daily", &group]) {
        Ok(None) => Some(0),
        Ok(Some(daily)) => std::fs::read_dir(daily).ok().and_then(|list| {
            let mut days = 0;
            for entry in list {
                // An entry that cannot be read leaves the count unknown, not smaller.
                let entry = entry.ok()?;
                let name = entry.file_name().to_string_lossy().into_owned();
                let dated = name
                    .strip_suffix(".json")
                    .is_some_and(|d| crate::storage::validate_date(d).is_ok());
                if dated && entry.file_type().ok()?.is_file() {
                    days += 1;
                }
            }
            Some(days)
        }),
        Err(()) => None,
    };
    (files, days)
}
