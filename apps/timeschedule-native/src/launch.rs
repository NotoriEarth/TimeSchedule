//! What the command line asks this process to be.

/// The helper modes an AI app runs this EXE in. They print to stdout and exit.
pub const HELPER_MODES: [&str; 3] = ["--mcp-auth-headers", "--mcp-stdio", "--mcp-probe"];

#[derive(Debug, PartialEq)]
pub enum Route {
    /// The diary window. `ignored` is the first argument that meant nothing
    /// here, shortened for display.
    Window { ignored: Option<String> },
    /// The record command line (`record enqueue ...`).
    Record,
    /// One of `HELPER_MODES`.
    Helper,
    /// Looks like a helper mode but is none: a misspelt registration. An AI
    /// app waits for its output, so it gets an error and never a window.
    UnknownHelper,
}

/// What the user is told when the window opened although an argument was given.
pub fn notice(ignored: &str) -> String {
    format!("지원하지 않는 실행 인자를 무시하고 앱을 열었습니다: {ignored}")
}
/// Only `record` and the helper modes are command lines. Anything else used to
/// be answered with the record usage error, which a release build (it has no
/// console) shows nowhere: the app simply did not appear.
pub fn route(args: &[String]) -> Route {
    match args.first().map(String::as_str) {
        None => Route::Window { ignored: None },
        Some("record") => Route::Record,
        Some(mode) if HELPER_MODES.contains(&mode) => Route::Helper,
        Some(switch) if resembles_helper(switch) => Route::UnknownHelper,
        Some(other) => Route::Window {
            ignored: Some(shown(other)),
        },
    }
}
/// Whether `argument` is a switch that names MCP without being a helper mode:
/// `--mcp_stdio`, `-mcp-stdio`, `--MCP-STDIO`. Whoever wrote it meant a helper.
fn resembles_helper(argument: &str) -> bool {
    let name = argument.trim_start_matches(['-', '/']);
    name.len() < argument.len()
        && !name.contains(['\\', '/', ':'])
        && name.to_ascii_lowercase().starts_with("mcp")
}
/// `argument` on one line and not too long to show in the window.
fn shown(argument: &str) -> String {
    const LIMIT: usize = 60;
    let plain: String = argument
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    if plain.chars().count() > LIMIT {
        plain.chars().take(LIMIT).chain(Some('…')).collect()
    } else {
        plain
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn routed(args: &[&str]) -> Route {
        route(&args.iter().map(|arg| arg.to_string()).collect::<Vec<_>>())
    }
    #[test]
    fn an_argument_that_means_nothing_here_still_opens_the_window() {
        assert_eq!(routed(&[]), Route::Window { ignored: None });
        // A file dropped on the EXE, a shortcut's leftover switch, a guess at a
        // help flag. The release build has no console, so answering these with
        // a usage error meant that nothing appeared at all.
        for args in [
            &["C:\\Users\\me\\Desktop\\notes.txt"][..],
            &["--data-dir", "D:\\diary"],
            &["/?"],
            &["-h"],
            &["recrod", "enqueue"],
            &[""],
        ] {
            assert_eq!(
                routed(args),
                Route::Window {
                    ignored: Some(args[0].to_string())
                },
                "{args:?}"
            );
        }
    }
    #[test]
    fn the_record_command_line_and_the_helper_modes_never_open_a_window() {
        for args in [
            &["record"][..],
            &["record", "enqueue", "--input", "x.json"],
            &["record", "unknown"],
        ] {
            assert_eq!(routed(args), Route::Record, "{args:?}");
        }
        for mode in HELPER_MODES {
            assert_eq!(routed(&[mode, "--data-dir", "C:\\data"]), Route::Helper);
        }
        // A misspelt helper is answered with an error, not with a window on
        // the user's screen every time the AI app connects.
        for args in [
            &["--mcp-authheaders"][..],
            &["--mcp-"],
            &["--mcp-stdio2"],
            &["--mcp_stdio", "--data-dir", "C:\\data"],
            &["--mcpstdio"],
            &["-mcp-stdio"],
            &["--MCP-STDIO"],
            &["/mcp-stdio"],
        ] {
            assert_eq!(routed(args), Route::UnknownHelper, "{args:?}");
        }
        // A path or a word that merely contains the letters is not a switch.
        for args in [&["C:\\tools\\mcp\\notes.txt"][..], &["mcp"]] {
            assert!(matches!(routed(args), Route::Window { .. }), "{args:?}");
        }
    }
    #[test]
    fn what_is_shown_of_an_ignored_argument_is_short_and_plain() {
        let long = "x".repeat(500);
        let Route::Window {
            ignored: Some(shown),
        } = routed(&[&long])
        else {
            panic!("not a window");
        };
        assert!(shown.chars().count() <= 61, "{}", shown.chars().count());
        assert!(shown.ends_with('…'));
        let Route::Window {
            ignored: Some(shown),
        } = routed(&["line\none\ttab\u{7}bell"])
        else {
            panic!("not a window");
        };
        assert!(!shown.chars().any(char::is_control), "{shown:?}");
        let notice = notice("C:\\notes.txt");
        assert!(notice.contains("C:\\notes.txt") && notice.contains("무시"));
    }
}
