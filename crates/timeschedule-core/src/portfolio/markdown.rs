// Only explicit field labels assign a passage to a criterion. Unstructured
// notes retain their opening paragraph and full source without inferred claims.
const CRITERIA: &[(&str, &[&str])] = &[
    ("배경·문제", &["배경", "배경과 문제"]),
    ("목표·성공 기준", &["목표", "성공 기준"]),
    (
        "개인 역할·기여",
        &["개인 기여", "개인 역할", "내 역할", "내가 맡은 역할"],
    ),
    ("기술 선택·설계", &["설계", "설계 결정", "기술 선택"]),
    ("주요 구현", &["구현", "한 일", "변경 내용"]),
    (
        "문제 해결",
        &["해결 과정", "확인한 문제와 수정", "시도와 실패"],
    ),
    (
        "검증·성과",
        &["검증", "검증 결과", "검증 범위와 한계", "테스트 결과"],
    ),
    ("근거", &["관련 자료"]),
    ("협업·의사결정", &["협업", "의사결정"]),
    ("회고·개선", &["회고", "배운 점", "배운 점과 관련 자료"]),
    ("자동 처리 범위", &[]),
    ("사람이 개입하는 조건", &["사용자 개입 조건"]),
    ("실패 복구", &["중단 복구"]),
];

fn criterion(label: &str) -> Option<&'static str> {
    let label = label.trim().trim_matches('*').trim();
    CRITERIA
        .iter()
        .find(|(name, aliases)| *name == label || aliases.contains(&label))
        .map(|v| v.0)
}

fn labeled_line(text: &str) -> Option<(&'static str, &str)> {
    let (label, value) = if text.starts_with('|') {
        text.trim_matches('|').split_once('|')?
    } else {
        text.trim_start_matches("- ").split_once([':', '：'])?
    };
    Some((criterion(label)?, value.trim()))
}

pub(super) fn facts(body: &str) -> Vec<(&'static str, &str)> {
    let mut facts = Vec::new();
    let mut section = None;
    let mut fence = None;
    let mut offset = 0;
    // Keep the whole labeled section, including qualifiers on following lines.
    for line in body.split_inclusive('\n') {
        let text = line.trim();
        let start = offset;
        offset += line.len();
        if let Some(c) = text.chars().next().filter(|c| matches!(c, '~' | '`')) {
            let len = text.chars().take_while(|&x| x == c).count();
            if len >= 3 {
                match fence {
                    None => fence = Some((c, len)),
                    Some((marker, n))
                        if c == marker && len >= n && text.chars().all(|x| x == c) =>
                    {
                        fence = None
                    }
                    _ => {}
                }
                continue;
            }
        }
        if fence.is_some() {
            continue;
        }
        let field = labeled_line(text);
        if text.starts_with('#') || field.is_some() {
            if let Some((key, begin)) = section.take() {
                let value = body[begin..start].trim();
                if !value.is_empty() {
                    facts.push((key, value));
                }
            }
            if let Some((key, value)) = field {
                if !value.is_empty() {
                    facts.push((key, value));
                }
            } else if let Some(key) = criterion(text.trim_start_matches('#')) {
                section = Some((key, offset));
            }
        }
    }
    if let Some((key, begin)) = section {
        let value = body[begin..].trim();
        if !value.is_empty() {
            facts.push((key, value));
        }
    }
    facts
}

pub(super) fn needs_role(body: &str) -> bool {
    !facts(body).iter().any(|(key, value)| {
        *key == "개인 역할·기여" && !matches!(value.trim(), "미확인" | "미정" | "미입력" | "-")
    })
}

pub(super) fn opening(body: &str) -> Option<&str> {
    // Preserve whole paragraphs (including measurement conditions).
    let mut begin = None;
    let mut end = 0;
    let mut index = 0;
    for line in body.split_inclusive('\n') {
        let text = line.trim();
        if text.starts_with(['#', '|']) || text.is_empty() || labeled_line(text).is_some() {
            if begin.is_some() {
                break;
            }
            if text.starts_with('#') && criterion(text.trim_start_matches('#')).is_some() {
                break;
            }
            if labeled_line(text).is_some() {
                break;
            }
        } else {
            if text.starts_with("```") || text.starts_with("~~~") {
                break;
            }
            begin.get_or_insert(index);
            end = index + line.trim_end().len();
        }
        index += line.len();
    }
    begin.map(|i| body[i..end].trim())
}

pub(super) fn plain(text: &str) -> String {
    let mut out = String::new();
    for c in text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
    {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\\' | '[' | ']' | '*' | '_' | '`' | '#' | '|' | '!' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

pub(super) fn source(out: &mut String, anchor: &str, title: &str, body: &str) {
    use std::fmt::Write;
    writeln!(out, "<a id=\"{anchor}\"></a>\n\n## {}\n", plain(title)).unwrap();
    verbatim(out, body);
}

pub(super) fn verbatim(out: &mut String, text: &str) {
    for line in text.lines() {
        out.push_str("    ");
        out.push_str(line);
        out.push('\n');
    }
    out.push('\n');
}

pub(super) fn web_link(text: &str) -> Option<String> {
    let url = url::Url::parse(text).ok()?;
    (matches!(url.scheme(), "https" | "http")
        && url.has_host()
        && url.username().is_empty()
        && url.password().is_none())
    .then(|| url.to_string().replace('<', "%3C").replace('>', "%3E"))
}
