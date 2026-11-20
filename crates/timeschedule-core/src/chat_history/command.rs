// A completed shell command can exit successfully while reporting a stopped service.
// Keep bounded, recognizable output lines. Never copy a command, environment dump or
// an arbitrary successful command's stdout into the model input.
pub(super) fn evidence(
    out: &str,
    code: Option<i64>,
    tool_error: bool,
    git_work_tree: bool,
) -> Option<String> {
    let code_label = code.map_or_else(|| "미상".to_owned(), |n| n.to_string());
    let mut result = format!("종료 코드 {code_label}");
    if out.starts_with("TAP version ") && out.lines().any(|s| s.starts_with("# tests ")) {
        return Some(format!("{result}\n{out}"));
    }
    let failed = tool_error || code.is_some_and(|n| n != 0);
    let mut lines = Vec::new();
    for line in out.lines().map(str::trim) {
        if line.is_empty() || line.len() > 400 || line.chars().any(char::is_control) {
            continue;
        }
        let lower = line.to_ascii_lowercase();
        if line.contains(['"', '\'', '=', '{', '}', '@', '?'])
            || [
                "password",
                "passwd",
                "secret",
                "token",
                "authorization",
                "api_key",
                "api-key",
                "bearer ",
                "cookie",
                "private key",
                "://",
            ]
            .iter()
            .any(|word| lower.contains(word))
        {
            continue;
        }
        let status = [
            " - accepting connections",
            " - rejecting connections",
            " - no response",
        ]
        .iter()
        .any(|suffix| lower.ends_with(suffix))
            || lower.contains(": stopped (")
            || lower.starts_with("test result: ")
            || lower.starts_with("http/")
                && lower
                    .split_whitespace()
                    .nth(1)
                    .is_some_and(|s| s.len() == 3 && s.bytes().all(|b| b.is_ascii_digit()));
        let diagnostic = [
            "error:",
            "fatal:",
            "failed to ",
            "cannot connect",
            "connection refused",
            "timed out",
            "timeout ",
            "not found",
            "connection was reset",
            "internal server error",
            "오류",
            "실패",
        ]
        .iter()
        .any(|term| lower.contains(term));
        let git_status = git_work_tree && matches!(line, "true" | "false");
        if (status || git_status || failed && diagnostic) && !lines.contains(&line) {
            lines.push(line);
            if lines.len() == 3 {
                break;
            }
        }
    }
    if lines.is_empty() && !failed {
        return None;
    }
    for line in lines {
        result.push('\n');
        result.push_str(line);
    }
    Some(result)
}
