#[derive(Clone, Debug)]
pub struct Evidence {
    pub id: String,
    pub title: String,
    pub date: String,
    pub body: String,
    pub source_url: String,
    pub source_ref: Option<String>,
}
pub fn evidence_draft(
    scope: &str,
    records: &[Evidence],
    portfolio: bool,
    truncated: bool,
) -> String {
    let title = if portfolio {
        "포트폴리오 근거 초안"
    } else {
        "근거 요약 초안"
    };
    let mut lines = vec![format!("{scope} · {title}"), "저장된 자료를 날짜순으로 정리한 초안입니다. AI 분석 결과가 아니며, 성과와 배운 점은 직접 확인해 작성해 주세요.".into(), String::new()];
    if records.is_empty() {
        lines.push("이 범위에 저장된 근거가 없습니다.".into());
    }
    let mut sorted: Vec<_> = records.iter().collect();
    sorted.sort_by(|a, b| a.date.cmp(&b.date));
    for record in sorted.iter().take(100) {
        lines.push(format!("{} · {}", record.date, record.title));
        lines.push(format!("기록 ID: {}", record.id));
        if !record.body.is_empty() {
            let mut excerpt: String = record.body.chars().take(600).collect();
            if record.body.chars().count() > 600 {
                excerpt.push_str("\n[본문 발췌 · 원문에서 계속]");
            }
            lines.push(excerpt);
        }
        if let Some(source) = &record.source_ref {
            if !source.is_empty() {
                lines.push(format!("출처: {source}"));
            }
        }
        if !record.source_url.is_empty() {
            lines.push(record.source_url.clone());
        }
        lines.push(String::new());
    }
    if truncated || records.len() > 100 {
        lines
            .push("[최대 100개 근거의 초안입니다. 나머지 기록은 원문 목록에서 확인하세요.]".into());
        lines.push(String::new());
    }
    lines.push("직접 보완할 내용".into());
    lines.push(
        if portfolio {
            "• 프로젝트 목적과 내가 맡은 역할\n• 문제, 시도, 해결 근거\n• 검증된 결과와 배운 점"
        } else {
            "• 이번 기간의 배운 점\n• 해결하지 못한 문제\n• 다음에 할 일"
        }
        .into(),
    );
    lines.join("\n")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sources_preserved_and_limit_explicit() {
        let records: Vec<_> = (0..101)
            .map(|i| Evidence {
                id: format!("id-{i}"),
                title: "한글 기록".into(),
                date: "2026-10-29".into(),
                body: "근거".repeat(700),
                source_url: "https://example.com".into(),
                source_ref: Some("commit:123".into()),
            })
            .collect();
        let before = records[0].body.clone();
        let draft = evidence_draft("프로젝트", &records, true, false);
        assert!(draft.contains("기록 ID: id-99"));
        assert!(!draft.contains("기록 ID: id-100"));
        assert!(draft.contains("최대 100개"));
        assert!(draft.contains("출처: commit:123"));
        assert!(draft.contains("AI 분석 결과가 아니며"));
        assert_eq!(records[0].body, before);
    }
}
