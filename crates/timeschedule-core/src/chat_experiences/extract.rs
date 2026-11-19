use super::{Candidate, ChatChoice, Kind};
use std::fmt::Write;

#[derive(Clone, Copy)]
struct Block<'a> {
    role: &'a str,
    line: usize,
    text: &'a str,
}

fn blocks(body: &str) -> Vec<Block<'_>> {
    let mut result = Vec::new();
    let mut start = 0;
    let mut current = None;
    let mut offset = 0;
    for line in body.split_inclusive('\n') {
        let header = line
            .trim_end()
            .strip_prefix("## ")
            .and_then(|h| h.split_once(" · 원문 "))
            .filter(|(r, _)| matches!(*r, "사용자" | "AI" | "검사 출력"))
            .and_then(|(r, n)| Some((r, n.strip_suffix('행')?.parse::<usize>().ok()?)));
        if let Some((role, n)) = header {
            if let Some((role, line)) = current {
                result.push(Block {
                    role,
                    line,
                    text: body[start..offset].trim(),
                });
            }
            current = Some((role, n));
            start = offset + line.len();
        }
        offset += line.len();
    }
    if let Some((role, line)) = current {
        result.push(Block {
            role,
            line,
            text: body[start..].trim(),
        });
    }
    result
}

fn contains(text: &str, words: &[&str]) -> bool {
    let lower = text.to_lowercase();
    words.iter().any(|w| lower.contains(w))
}
fn excerpt(text: &str) -> String {
    let text = text.trim();
    let s = if text.ends_with(')') && text.contains("](") {
        text.split_once(" [").map_or(text, |(quote, _)| quote)
    } else {
        text
    };
    match s.char_indices().nth(360) {
        Some((end, _)) => format!("{}…", &s[..end]),
        None => s.into(),
    }
}
fn citation(b: Block<'_>, text: &str) -> String {
    format!("{} ({} · 원문 {}행)", excerpt(text), b.role, b.line)
}

const DESIGN: &[&str] = &[
    "설계",
    "구조",
    "아키텍처",
    "design",
    "architect",
    "trade-off",
];
const TROUBLE: &[&str] = &[
    "오류",
    "장애",
    "막았",
    "실패",
    "버그",
    "재현",
    "트러블",
    "문제",
    "error",
    "fail",
    "regression",
    "bug",
];

fn extract_turn(
    chat: &ChatChoice,
    turn: &[Block<'_>],
    kind: Kind,
    origin: &str,
) -> Option<Candidate> {
    let request = *turn.first()?;
    let intent = match kind {
        Kind::Design => DESIGN,
        Kind::Troubleshooting => TROUBLE,
    };
    if !contains(request.text, intent) {
        return None;
    }
    // A correction replaces the earlier explanation; never present an older result as the correction.
    let latest = turn.iter().rposition(|b| b.role == "사용자")?;
    let replies = &turn[latest + 1..];
    let response = replies
        .iter()
        .rev()
        .find(|b| b.role == "AI" && !b.text.trim_end().ends_with("하겠습니다."));
    let output = replies.iter().rev().find(|b| b.role == "검사 출력");
    let answers = response
        .into_iter()
        .flat_map(|b| b.text.lines().map(move |l| (*b, l.trim())))
        .filter(|(_, l)| {
            !l.is_empty()
                && !l.starts_with(['#', '`', '['])
                && !l.contains("표본")
                && !l.starts_with("파일은 수정")
        })
        .collect::<Vec<_>>();
    let result_words = match kind {
        Kind::Design => &[
            "저장", "공유", "분리", "선택", "사용", "도입", "설계", "결정", "use", "chose",
            "separate", "design",
        ][..],
        Kind::Troubleshooting => &[
            "검사",
            "테스트",
            "종료",
            "해결",
            "수정",
            "교체",
            "수정했",
            "복구",
            "fixed",
            "test",
            "resolved",
        ][..],
    };
    let result = answers.iter().find(|(_, l)| {
        contains(l, result_words)
            && !contains(l, &["추정", "확인된 설계 이유", "하겠습니다", "할 예정"])
    });
    let has_output = kind == Kind::Troubleshooting && output.is_some();
    if result.is_none() && !has_output {
        return None;
    }
    let prompt = request.text.lines().find(|line| !line.trim().is_empty())?;
    let problem = prompt.find(". ").map_or(prompt, |end| &prompt[..end + 1]);
    let mut body = format!("배경과 문제: {}\n", citation(request, problem));
    let reason = answers.iter().find(|(_, l)| {
        contains(
            l,
            &[
                "확인된 근거",
                "확인된 설계 이유",
                "때문",
                "하기 위해",
                "선택한 이유",
                "선택 이유",
                "because",
                "in order to",
            ],
        ) && !contains(l, &["추정", "추측", "미확인"])
    });
    if let Some((b, l)) = reason {
        writeln!(body, "의사결정: {}", citation(*b, l)).unwrap();
    } else {
        body.push_str("의사결정: 판단 이유를 명시한 원문 근거가 부족하다.\n");
    }
    if let Some((b, l)) = result {
        writeln!(
            body,
            "{}: {}",
            if kind == Kind::Design {
                "설계 결정"
            } else {
                "해결 과정"
            },
            citation(*b, l)
        )
        .unwrap();
    } else {
        body.push_str("해결 과정: 원인과 수정 과정은 확인되지 않았다.\n");
    }
    if kind == Kind::Troubleshooting {
        if let Some(b) = output {
            let summary = b
                .text
                .lines()
                .filter(|s| {
                    s.starts_with("종료 코드 ")
                        || s.starts_with("# Subtest:")
                        || s.starts_with("# pass ")
                        || s.starts_with("# fail ")
                        || s.starts_with("# skipped ")
                })
                .collect::<Vec<_>>();
            writeln!(
                body,
                "검증 결과: {} (검사 출력 · 원문 {}행)",
                summary.join(" / "),
                b.line
            )
            .unwrap();
        } else {
            body.push_str(
                "검증 결과: 이 요청에 연결된 실제 검사 출력은 수집본에서 확인되지 않았다.\n",
            );
        }
        if let Some((b, l)) = answers.iter().find(|(_, l)| {
            contains(
                l,
                &[
                    "실행하지",
                    "재현하지",
                    "미확인",
                    "확인하지",
                    "not tested",
                    "not run",
                ],
            )
        }) {
            writeln!(body, "검증 범위와 한계: {}", citation(*b, l)).unwrap();
        }
    }
    writeln!(
        body,
        "\n## 원문 발췌\n\nWindows Codex 채팅 「{}」 · 기록 {}",
        chat.title.trim_start_matches("Codex 채팅 · "),
        chat.id
    )
    .unwrap();
    writeln!(body, "{origin}").unwrap();
    for b in turn.iter().filter(|b| {
        b.line == request.line
            || b.line == turn[latest].line
            || response.is_some_and(|a| a.line == b.line)
            || (has_output && output.is_some_and(|o| o.line == b.line))
    }) {
        let width = b
            .text
            .lines()
            .map(|s| s.trim_start().chars().take_while(|&c| c == '~').count())
            .max()
            .unwrap_or(0)
            + 1;
        let fence = "~".repeat(width.max(3));
        writeln!(
            body,
            "\n{} · 원문 {}행\n{fence}\n{}\n{fence}",
            b.role, b.line, b.text
        )
        .unwrap();
    }
    Some(Candidate {
        kind,
        title: format!(
            "{} · {}",
            chat.title.trim_start_matches("Codex 채팅 · "),
            kind.label()
        ),
        body,
    })
}

pub(super) fn candidates(chat: &ChatChoice, source: &str) -> Vec<Candidate> {
    let blocks = blocks(source);
    let mut starts = blocks
        .iter()
        .enumerate()
        .filter_map(|(i, b)| {
            let correction = ["방금 답변", "이전 답변", "앞선 답변", "revise your"]
                .iter()
                .any(|prefix| b.text.to_lowercase().starts_with(prefix));
            (b.role == "사용자" && (i == 0 || !correction)).then_some(i)
        })
        .collect::<Vec<_>>();
    starts.push(blocks.len());
    [Kind::Design, Kind::Troubleshooting]
        .into_iter()
        .filter_map(|kind| {
            starts.windows(2).rev().find_map(|range| {
                extract_turn(
                    chat,
                    &blocks[range[0]..range[1]],
                    kind,
                    source
                        .lines()
                        .nth(1)
                        .filter(|s| s.starts_with("원문: "))
                        .unwrap_or(""),
                )
            })
        })
        .collect()
}
