//! Reproducible search/save timing on generated data. Never touches user data.
//!
//! Run (release, prints a Markdown table):
//!   cargo test --release -p timeschedule-core --test search_perf -- --ignored --nocapture
//! Optional: TS_SEARCH_BENCH_SIZES=1000,10000,50000 (total items per database),
//! TS_SEARCH_BENCH_KEEP=1 keeps the generated databases in %TEMP% for inspection.
//!
//! Each size builds a fresh temporary file database through `Storage::open` (so the
//! app's schema and triggers exist), fills it in one transaction, reopens it with the
//! app's settings (WAL, synchronous=FULL) and times the public API. The text is
//! generated Korean from a fixed seed; absolute numbers depend on the PC and disk.
use std::sync::Arc;
use std::time::{Duration, Instant};
use timeschedule_core::{
    calendar::TaskDetails,
    storage::{EventInput, NoteInput, Storage, TaskInput},
};

const WORDS: &[&str] = &[
    "오늘",
    "작업",
    "내용",
    "정리",
    "확인",
    "결과",
    "테스트",
    "빌드",
    "수정",
    "문제",
    "원인",
    "해결",
    "검증",
    "기록",
    "다음",
    "단계",
    "설정",
    "화면",
    "일정",
    "할일",
    "프로젝트",
    "저장소",
    "커밋",
    "변경",
    "사용자",
    "요청",
    "검토",
    "리뷰",
    "반영",
    "배포",
    "실행",
    "파일",
    "폴더",
    "데이터",
    "검색",
    "성능",
    "측정",
    "개선",
    "구조",
    "오류",
    "로그",
    "메시지",
    "연결",
    "서버",
    "토큰",
    "백업",
    "복구",
    "버전",
    "문서",
    "안내",
    "회의록",
    "공부",
    "정리본",
    "자료",
    "비교",
    "설명",
    "계획",
    "논의",
    "합의",
    "완료",
    "보류",
    "진행",
    "시작",
    "종료",
    "주간",
    "월간",
    "목표",
    "있습니다",
    "했습니다",
    "됩니다",
    "않습니다",
    "위해",
    "그리고",
    "하지만",
    "따라서",
    "먼저",
    "이후",
    "다시",
    "모두",
    "일부",
    "함께",
    "따로",
    "빠르게",
    "천천히",
    "Rust",
    "SQLite",
    "PR",
    "CI",
    "MCP",
    "Git",
    "UI",
    "API",
    "JSON",
    "Windows",
];

/// Deterministic xorshift so every run sees the same data and matches.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    /// About `chars` characters of generated prose; rare markers are injected by rate.
    fn text(&mut self, chars: usize) -> String {
        let mut out = String::with_capacity(chars * 3);
        let mut count = 0;
        while count < chars {
            // Per-word rates chosen so roughly 0.5% of records contain each rare marker.
            let word = match self.below(100_000) {
                0 => "견적",
                1 => "배포일정점검",
                _ => WORDS[self.below(WORDS.len() as u64) as usize],
            };
            out.push_str(word);
            out.push(if self.below(12) == 0 { '\n' } else { ' ' });
            count += word.chars().count() + 1;
        }
        out
    }
}

fn uuid(kind: u64, n: u64) -> String {
    format!("{kind:08x}-0000-4000-8000-{n:012x}")
}
fn date(n: u64) -> String {
    let day = n % 1460; // four years of dates
    let (y, rest) = (2023 + day / 365, day % 365);
    format!("{y:04}-{:02}-{:02}", 1 + rest / 31 % 12, 1 + rest % 28)
}
fn stamp(n: u64) -> String {
    format!(
        "2026-{:02}-{:02}T{:02}:{:02}:00.000Z",
        1 + n % 12,
        1 + n % 28,
        n % 24,
        n % 60
    )
}

struct Built {
    path: std::path::PathBuf,
    task_id: String,
    event_id: String,
    note_id: String,
    bytes: u64,
}

fn build(total: u64) -> Built {
    let path = std::env::temp_dir().join(format!(
        "timeschedule-search-perf-{total}-{}.db",
        std::process::id()
    ));
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
    }
    drop(Storage::open(&path).unwrap());
    let mut c = rusqlite::Connection::open(&path).unwrap();
    c.execute_batch("PRAGMA synchronous=OFF;").unwrap();
    // Generation only: the task-detail triggers would make filling quadratic. Keep their
    // SQL, fill the index rows by rowid exactly as the triggers would, then restore them.
    let triggers: Vec<String> = c
        .prepare("SELECT sql FROM sqlite_master WHERE type='trigger' AND name LIKE 'task_details_search_%'")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let tx = c.transaction().unwrap();
    tx.execute_batch(
        "DROP TRIGGER task_details_search_insert; DROP TRIGGER task_details_search_update;",
    )
    .unwrap();
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ total);
    for n in 0..total {
        let title = format!("{} {} {}", rng.text(6).trim(), n, rng.text(4).trim());
        let title: String = title.chars().take(120).collect();
        match n % 20 {
            // 40% development records (real records average ~2,700 characters).
            0..=7 => {
                tx.execute("INSERT INTO diary_entries(id,project_id,date,kind,title,body,source_url,updated_at) VALUES(?1,NULL,?2,'development',?3,?4,'',?5)",
                    rusqlite::params![uuid(1, n), date(n), title, { let n = 1500 + rng.below(2400) as usize; rng.text(n) }, stamp(n)]).unwrap();
            }
            8..=12 => {
                tx.execute(
                    "INSERT INTO events VALUES(?1,?2,?3,NULL,NULL,1,?4,'sage',0,?5)",
                    rusqlite::params![
                        uuid(2, n),
                        title,
                        date(n),
                        {
                            let n = rng.below(400) as usize;
                            rng.text(n)
                        },
                        stamp(n)
                    ],
                )
                .unwrap();
            }
            13..=17 => {
                let id = uuid(3, n);
                let body = {
                    let n = rng.below(400) as usize;
                    rng.text(n)
                };
                tx.execute(
                    "INSERT INTO tasks VALUES(?1,?2,?3,0,?4)",
                    rusqlite::params![id, title, date(n), stamp(n)],
                )
                .unwrap();
                tx.execute(
                    "INSERT INTO task_details VALUES(?1,?2,1,NULL,NULL)",
                    rusqlite::params![id, body],
                )
                .unwrap();
                tx.execute("UPDATE diary_search SET body=?2 WHERE rowid=(SELECT rowid*5+1 FROM tasks WHERE id=?1)", rusqlite::params![id, body]).unwrap();
            }
            _ => {
                tx.execute(
                    "INSERT INTO notes VALUES(?1,?2,?3,?4)",
                    rusqlite::params![
                        uuid(4, n),
                        title,
                        {
                            let n = 300 + rng.below(1200) as usize;
                            rng.text(n)
                        },
                        stamp(n)
                    ],
                )
                .unwrap();
            }
        }
    }
    for sql in &triggers {
        tx.execute_batch(sql).unwrap();
    }
    tx.commit().unwrap();
    c.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);").unwrap();
    drop(c);
    let bytes = std::fs::metadata(&path).unwrap().len();
    Built {
        path,
        task_id: uuid(3, 13),
        event_id: uuid(2, 8),
        note_id: uuid(4, 18),
        bytes,
    }
}

fn stats(mut samples: Vec<Duration>) -> String {
    samples.sort();
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    let median = samples[samples.len() / 2];
    let p95 = samples[(samples.len() * 95 / 100).min(samples.len() - 1)];
    format!(
        "{:.2} | {:.2} | {:.2}",
        ms(median),
        ms(p95),
        ms(*samples.last().unwrap())
    )
}
fn time<T>(runs: usize, mut f: impl FnMut() -> T) -> (Vec<Duration>, T) {
    let mut last = f(); // warm cache once; not counted
    let mut out = Vec::with_capacity(runs);
    for _ in 0..runs {
        let start = Instant::now();
        last = f();
        out.push(start.elapsed());
    }
    (out, last)
}

#[test]
#[ignore = "timing benchmark; run explicitly with --ignored --nocapture in release"]
fn search_and_save_timings_on_generated_data() {
    let sizes: Vec<u64> = std::env::var("TS_SEARCH_BENCH_SIZES")
        .unwrap_or_else(|_| "1000,10000".into())
        .split(',')
        .map(|v| v.trim().parse().unwrap())
        .collect();
    let runs = 15;
    println!("\n| 항목 수 | 측정 | 결과 수 | 중앙값 ms | p95 ms | 최대 ms |\n|---:|---|---:|---:|---:|---:|");
    for total in sizes {
        let started = Instant::now();
        let built = build(total);
        let build_time = started.elapsed();
        let store = Arc::new(Storage::open(&built.path).unwrap());
        for (label, query) in [
            ("검색 3글자+ 흔함 `회의록`", "회의록"),
            ("검색 3글자+ 드묾 `배포일정점검`", "배포일정점검"),
            ("검색 3글자+ 없음 `존재하지않음`", "존재하지않음"),
            ("검색 2글자 흔함 `회의`", "회의"),
            ("검색 2글자 드묾 `견적`", "견적"),
            ("검색 2글자 없음 `꿹뷁`", "꿹뷁"),
            ("검색 1글자 `회`", "회"),
            ("검색 2글자 영문 `PR`", "PR"),
        ] {
            let (samples, hits) = time(runs, || store.search_diary(query, 50).unwrap().len());
            println!("| {total} | {label} | {hits} | {} |", stats(samples));
        }
        let mut body_flip = false;
        let (samples, _) = time(runs, || {
            body_flip = !body_flip;
            let edit = store.task_edit(&built.task_id).unwrap();
            let details = TaskDetails {
                body: if body_flip {
                    "측정 본문 가"
                } else {
                    "측정 본문 나"
                }
                .into(),
                ..edit.details.clone()
            };
            store
                .save_calendar_task(
                    TaskInput {
                        id: Some(built.task_id.clone()),
                        title: edit.task.title.clone(),
                        date: edit.task.date.clone(),
                        completed: false,
                    },
                    details,
                    Some(&edit),
                )
                .unwrap();
        });
        println!(
            "| {total} | 할 일 상세 저장 `save_calendar_task` | - | {} |",
            stats(samples)
        );
        let event = store.get_event(&built.event_id).unwrap();
        let (samples, _) = time(runs, || {
            body_flip = !body_flip;
            store
                .save_event(EventInput {
                    id: Some(event.id.clone()),
                    title: event.title.clone(),
                    date: event.date.clone(),
                    start_time: None,
                    end_time: None,
                    all_day: true,
                    notes: if body_flip { "가" } else { "나" }.into(),
                    color: "sage".into(),
                    completed: false,
                })
                .unwrap();
        });
        println!(
            "| {total} | 비교: 일정 저장 `save_event` | - | {} |",
            stats(samples)
        );
        let (samples, _) = time(runs, || {
            body_flip = !body_flip;
            store
                .save_note(NoteInput {
                    id: Some(built.note_id.clone()),
                    title: "측정 메모".into(),
                    body: if body_flip { "가" } else { "나" }.into(),
                })
                .unwrap();
        });
        println!(
            "| {total} | 비교: 메모 저장 `save_note` | - | {} |",
            stats(samples)
        );
        // The UI refresh shares the one connection; measure how long it waits behind a
        // short-query search that started just before it.
        let waits = (0..runs)
            .map(|_| {
                let searching = Arc::clone(&store);
                let worker =
                    std::thread::spawn(move || searching.search_diary("회의", 50).unwrap());
                std::thread::sleep(Duration::from_millis(2));
                let start = Instant::now();
                store.calendar_days("2026-11-01", "2026-11-30").unwrap();
                let waited = start.elapsed();
                worker.join().unwrap();
                waited
            })
            .collect();
        println!(
            "| {total} | 2글자 검색 중 월 화면 새로 고침 대기 `calendar_days` | - | {} |",
            stats(waits)
        );
        let (samples, _) = time(runs, || {
            store.calendar_days("2026-11-01", "2026-11-30").unwrap()
        });
        println!(
            "| {total} | 비교: 검색 없을 때 `calendar_days` | - | {} |",
            stats(samples)
        );
        println!(
            "| {total} | (DB {:.1} MB, 생성 {:.1}초) | | | | |",
            built.bytes as f64 / 1_048_576.0,
            build_time.as_secs_f64()
        );
        drop(store);
        if std::env::var_os("TS_SEARCH_BENCH_KEEP").is_some() {
            println!("kept: {}", built.path.display());
            continue;
        }
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", built.path.display()));
        }
    }
}
