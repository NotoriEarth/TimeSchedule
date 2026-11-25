use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AppTheme {
    Porcelain,
    #[default]
    #[serde(other)]
    Midnight,
}

/// How daily records are made: 설정 › 하루 정리 만드는 방식.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DailyRecordMode {
    /// The app makes the finished days of the last week by itself.
    Auto,
    /// Only for the selected day, when its button is pressed. An unknown value reads
    /// as this, so no model is called by surprise.
    #[default]
    #[serde(other)]
    Button,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Preferences {
    pub widget_x: Option<i32>,
    pub widget_y: Option<i32>,
    pub widget_locked: bool,
    pub widget_visible: bool,
    pub widget_opacity: u8,
    pub theme: AppTheme,
    pub widget: crate::widget_preferences::WidgetPreferences,
    pub daily_record: DailyRecordMode,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            widget_x: None,
            widget_y: None,
            widget_locked: true,
            widget_visible: true,
            widget_opacity: 82,
            theme: AppTheme::Midnight,
            widget: Default::default(),
            daily_record: DailyRecordMode::Button,
        }
    }
}
pub fn load(directory: &Path) -> Result<Preferences, String> {
    match fs::read(directory.join("preferences.json")) {
        Ok(bytes) => {
            let value: Preferences = serde_json::from_slice(&bytes)
                .map_err(|e| format!("설정 파일을 읽지 못했습니다: {e}"))?;
            validate(&value)?;
            Ok(value)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Preferences::default()),
        Err(e) => Err(e.to_string()),
    }
}
/// What `load_or_repair` made of the stored preferences.
#[derive(Debug)]
pub struct Loaded {
    pub preferences: Preferences,
    /// The values that were unusable and are back at their defaults, by name
    /// (`widget.eventSize`), sorted. Everything else is as the user left it.
    pub reset: Vec<String>,
    /// The file was not a JSON object at all, so nothing could be kept.
    pub unreadable: bool,
    /// Where the file was copied before it was put right.
    pub kept: Option<std::path::PathBuf>,
    /// Something had to be put right but the copy could not be written, so
    /// the file was left as it was.
    pub copy_failed: bool,
}
impl Loaded {
    /// What to tell the user, if anything was put right.
    pub fn notice(&self) -> Option<String> {
        let kept = match &self.kept {
            Some(path) => format!(
                " 원래 파일은 {} 파일로 남겼습니다.",
                path.file_name().unwrap_or_default().to_string_lossy()
            ),
            // Said plainly: the next change of a setting saves over the file.
            None if self.copy_failed => format!(
                " 원래 파일의 사본을 남기지 못해({INVALID_COPY}) 파일은 그대로 두었습니다. 설정을 바꾸면 고친 내용으로 저장됩니다."
            ),
            None => String::new(),
        };
        if self.unreadable {
            Some(format!(
                "설정 파일(preferences.json)의 형식이 깨져 기본 설정으로 시작했습니다.{kept}"
            ))
        } else if self.reset.is_empty() {
            None
        } else {
            Some(format!(
                "설정 파일의 잘못된 값 {}개만 기본값으로 되돌렸습니다({}). 나머지 설정은 그대로입니다.{kept}",
                self.reset.len(),
                self.reset.join(", ")
            ))
        }
    }
}
const INVALID_COPY: &str = "preferences.invalid.json";
/// The stored preferences, with every usable value kept. A value that is of
/// the wrong kind, out of range, or impossible together with another goes back
/// to its default on its own; the strict `load` would discard the whole file
/// for it, and with it the theme, the widget's place and size.
///
/// If anything had to be put right, the file as it was is first copied to
/// `preferences.invalid.json` and then rewritten, so the next start has
/// nothing to repair. Without that copy the file is left untouched.
pub fn load_or_repair(directory: &Path) -> Result<Loaded, String> {
    let path = directory.join("preferences.json");
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Loaded {
                preferences: Preferences::default(),
                reset: Vec::new(),
                unreadable: false,
                kept: None,
                copy_failed: false,
            })
        }
        Err(e) => return Err(e.to_string()),
    };
    // Read as a group of values that are each still unread text. A value that
    // cannot be read at all (a number no number type holds, say) then fails
    // on its own, like any other unusable value.
    let mut loaded = match serde_json::from_slice::<Group>(&bytes) {
        Ok(stored) => {
            let mut usable =
                serde_json::to_value(Preferences::default()).map_err(|e| e.to_string())?;
            let mut reset = Vec::new();
            keep_usable(&mut usable, &[], &stored, &mut reset);
            reset.sort();
            Loaded {
                preferences: accepted(&usable).unwrap_or_default(),
                reset,
                unreadable: false,
                kept: None,
                copy_failed: false,
            }
        }
        Err(_) => Loaded {
            preferences: Preferences::default(),
            reset: Vec::new(),
            unreadable: true,
            kept: None,
            copy_failed: false,
        },
    };
    if loaded.unreadable || !loaded.reset.is_empty() {
        // Rewritten only once the original is safe beside it.
        let copy = directory.join(INVALID_COPY);
        if replace(&copy, &bytes).is_ok() {
            loaded.kept = Some(copy);
            let _ = save(directory, &loaded.preferences);
        } else {
            loaded.copy_failed = true;
        }
    }
    Ok(loaded)
}
/// A JSON object whose values are kept as the text they were written as.
type Group = std::collections::BTreeMap<String, Box<serde_json::value::RawValue>>;
/// `candidate` as preferences, if it reads as such and passes every check.
fn accepted(candidate: &serde_json::Value) -> Option<Preferences> {
    let value: Preferences = serde_json::from_value(candidate.clone()).ok()?;
    validate(&value).ok().map(|_| value)
}
/// Copies into `usable` (complete, valid preferences as JSON) every value of
/// `stored` at `path` that leaves it valid, one at a time, and names in `reset`
/// those that do not. A group of values (the widget, a colour set) is taken
/// value by value, so one bad colour does not cost the others.
fn keep_usable(
    usable: &mut serde_json::Value,
    path: &[&str],
    stored: &Group,
    reset: &mut Vec<String>,
) {
    // Where two values are only wrong together, the one tried first stays.
    // The number of weeks shown comes before the number of past weeks in it.
    let mut keys: Vec<&String> = stored.keys().collect();
    keys.sort_by_key(|key| key.as_str() == "pastWeeks");
    for key in keys {
        let mut here = path.to_vec();
        here.push(key);
        let Some(default) = here
            .iter()
            .try_fold(&*usable, |value, key| value.get(*key))
            .cloned()
        else {
            // A setting this version does not know. Left alone, as before.
            continue;
        };
        let text = stored[key.as_str()].get();
        if default.is_object() {
            if let Ok(group) = serde_json::from_str::<Group>(text) {
                keep_usable(usable, &here, &group, reset);
                continue;
            }
        }
        let fits = serde_json::from_str::<serde_json::Value>(text)
            .ok()
            .and_then(|value| {
                let mut candidate = usable.clone();
                *here
                    .iter()
                    .try_fold(&mut candidate, |value, key| value.get_mut(*key))? = value;
                accepted(&candidate).map(|_| candidate)
            });
        match fits {
            Some(candidate) => *usable = candidate,
            None => reset.push(here.join(".")),
        }
    }
}
/// Puts `bytes` at `path` as a whole: written and flushed beside it, then
/// renamed over it. If that fails, what was at `path` is still there.
fn replace(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".tmp");
    let temporary = std::path::PathBuf::from(temporary);
    let written = fs::File::create(&temporary)
        .and_then(|mut file| file.write_all(bytes).and_then(|_| file.sync_all()))
        .and_then(|_| fs::rename(&temporary, path));
    written.map_err(|e| {
        let _ = fs::remove_file(&temporary);
        e.to_string()
    })
}
fn validate(value: &Preferences) -> Result<(), String> {
    value.widget.validate()?;
    if !(40..=100).contains(&value.widget_opacity) {
        Err("위젯 불투명도는 40~100%여야 합니다.".into())
    } else {
        Ok(())
    }
}
pub fn save(directory: &Path, value: &Preferences) -> Result<(), String> {
    validate(value)?;
    replace(
        &directory.join("preferences.json"),
        &serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?,
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn widget_options_default_and_reject_unsafe_values() {
        let old: Preferences =
            serde_json::from_str(r#"{"widgetX":-400,"widgetOpacity":65}"#).unwrap();
        let value = serde_json::to_value(&old).unwrap();
        assert_eq!(value["widget"]["width"], 960);
        assert_eq!(value["widget"]["weeks"], 4);
        assert_eq!(value["widget"]["rolling"], false);
        assert_eq!(value["widgetX"], -400);
        for patch in [
            serde_json::json!({"width":0}),
            serde_json::json!({"weeks":2,"pastWeeks":2}),
            serde_json::json!({"eventSize":60}),
            serde_json::json!({"dark":{"background":"not-a-color"}}),
        ] {
            let prefs: Preferences =
                serde_json::from_value(serde_json::json!({"widget":patch})).unwrap();
            assert!(validate(&prefs).is_err());
        }
        let custom: Preferences = serde_json::from_value(serde_json::json!({"widget":{
            "width":800,"height":500,"rolling":true,"weeks":5,"pastWeeks":1,
            "font":"Malgun Gothic","dateSize":18,"eventSize":14,"cellGap":4,
            "dark":{"text":"#FAFAFA"},"light":{"text":"#222222"}
        }}))
        .unwrap();
        assert!(validate(&custom).is_ok());
        assert_eq!(
            custom,
            serde_json::from_value(serde_json::to_value(&custom).unwrap()).unwrap()
        );
    }
    #[test]
    fn legacy_preferences_gain_midnight_without_losing_widget_settings() {
        let old = r#"{"widgetX":-1200,"widgetY":50,"widgetLocked":false,"widgetVisible":false,"widgetOpacity":65}"#;
        let prefs: Preferences = serde_json::from_str(old).unwrap();
        let json = serde_json::to_value(prefs).unwrap();
        assert_eq!(json["theme"], "midnight");
        assert_eq!(json["widgetX"], -1200);
        assert_eq!(json["widgetOpacity"], 65);
        assert_eq!(json["widgetVisible"], false);
    }
    #[test]
    fn the_daily_record_mode_is_the_button_unless_auto_was_chosen() {
        let old: Preferences = serde_json::from_str(r#"{"widgetX":-400}"#).unwrap();
        assert_eq!(old.daily_record, DailyRecordMode::Button);
        for (input, expected) in [("auto", "auto"), ("button", "button"), ("later", "button")] {
            let prefs: Preferences =
                serde_json::from_value(serde_json::json!({"dailyRecord": input})).unwrap();
            assert_eq!(
                serde_json::to_value(prefs).unwrap()["dailyRecord"],
                expected
            );
        }
    }
    #[test]
    fn light_theme_round_trips_and_unknown_theme_falls_back() {
        for (input, expected) in [("porcelain", "porcelain"), ("future-theme", "midnight")] {
            let prefs: Preferences =
                serde_json::from_value(serde_json::json!({"theme": input})).unwrap();
            assert_eq!(serde_json::to_value(prefs).unwrap()["theme"], expected);
        }
    }
    #[test]
    fn old_preferences_default_opacity_and_reject_invalid() {
        let old = r#"{"widgetX":-1200,"widgetY":50,"widgetLocked":false,"widgetVisible":true}"#;
        let prefs: Preferences = serde_json::from_str(old).unwrap();
        assert_eq!(prefs.widget_opacity, 82);
        assert_eq!(prefs.widget_x, Some(-1200));
        assert!(validate(&Preferences {
            widget_opacity: 0,
            ..prefs
        })
        .is_err());
    }
    #[test]
    fn replace_preferences_twice_preserves_values() {
        let dir =
            std::env::temp_dir().join(format!("timeschedule-prefs-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        save(&dir, &Preferences::default()).unwrap();
        let changed = Preferences {
            widget_x: Some(-500),
            widget_opacity: 40,
            theme: AppTheme::Porcelain,
            ..Preferences::default()
        };
        save(&dir, &changed).unwrap();
        assert_eq!(load(&dir).unwrap(), changed);
        fs::remove_file(dir.join("preferences.json")).unwrap();
        fs::remove_dir(dir).unwrap();
    }
    fn scratch() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "timeschedule-prefs-repair-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }
    fn stored(dir: &Path, json: &str) -> Loaded {
        fs::write(dir.join("preferences.json"), json).unwrap();
        load_or_repair(dir).unwrap()
    }
    #[test]
    fn one_bad_value_costs_only_itself() {
        let dir = scratch();
        let original = r##"{"widgetX":-1200,"widgetY":50,"widgetLocked":false,"widgetVisible":false,"widgetOpacity":250,"theme":"porcelain","widget":{"width":800,"height":500,"weeks":5,"eventSize":60,"font":"Malgun Gothic","dark":{"text":"#FAFAFA","background":"not-a-color"}}}"##;
        fs::write(dir.join("preferences.json"), original).unwrap();
        assert!(load(&dir).is_err(), "the strict reader refuses the file");
        let loaded = load_or_repair(&dir).unwrap();
        let p = &loaded.preferences;
        // Everything the user set and that is usable stays.
        assert_eq!((p.widget_x, p.widget_y), (Some(-1200), Some(50)));
        assert!(!p.widget_locked && !p.widget_visible);
        assert_eq!(p.theme, AppTheme::Porcelain);
        assert_eq!(
            (p.widget.width, p.widget.height, p.widget.weeks),
            (800, 500, 5)
        );
        assert_eq!(p.widget.dark.text.as_deref(), Some("#FAFAFA"));
        // Only the three unusable values are back at their defaults.
        assert_eq!(p.widget_opacity, 82);
        assert_eq!(p.widget.event_size, 13);
        assert_eq!(p.widget.dark.background, None);
        assert_eq!(
            loaded.reset,
            [
                "widget.dark.background",
                "widget.eventSize",
                "widgetOpacity"
            ]
        );
        assert!(!loaded.unreadable);
        // The file is put right, and what it said before is kept for the user.
        assert_eq!(&load(&dir).unwrap(), p);
        let kept = loaded.kept.clone().expect("the original was not kept");
        assert_eq!(kept, dir.join("preferences.invalid.json"));
        assert_eq!(fs::read_to_string(kept).unwrap(), original);
        // Nothing is left to repair, so nothing is reported or rewritten again.
        let again = load_or_repair(&dir).unwrap();
        assert!(again.reset.is_empty() && again.kept.is_none());
        assert_eq!(&again.preferences, p);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn a_value_of_the_wrong_kind_or_a_broken_group_is_reset_alone() {
        let dir = scratch();
        let loaded = stored(
            &dir,
            r#"{"widgetOpacity":"loud","widgetLocked":"yes","widgetX":12,"widget":"none"}"#,
        );
        assert_eq!(loaded.preferences.widget_x, Some(12));
        assert_eq!(loaded.preferences.widget_opacity, 82);
        assert!(loaded.preferences.widget_locked);
        assert_eq!(loaded.preferences.widget, Default::default());
        assert_eq!(loaded.reset, ["widget", "widgetLocked", "widgetOpacity"]);
        // Two values that are each fine but not together: the number of weeks
        // shown stays, the number of past weeks gives way.
        let loaded = stored(
            &dir,
            r#"{"widget":{"weeks":2,"pastWeeks":2,"width":0,"height":500}}"#,
        );
        let widget = &loaded.preferences.widget;
        assert_eq!((widget.weeks, widget.past_weeks), (2, 0));
        assert_eq!((widget.width, widget.height), (960, 500));
        assert_eq!(loaded.reset, ["widget.pastWeeks", "widget.width"]);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn a_file_that_is_not_a_json_object_starts_from_defaults_and_is_kept_aside() {
        for broken in [r#"{"widgetX": -12"#, "[1,2]", ""] {
            let dir = scratch();
            let loaded = stored(&dir, broken);
            assert_eq!(loaded.preferences, Preferences::default(), "{broken:?}");
            assert!(loaded.unreadable, "{broken:?}");
            assert_eq!(
                fs::read_to_string(loaded.kept.expect("the original was not kept")).unwrap(),
                broken
            );
            assert_eq!(load(&dir).unwrap(), Preferences::default());
            fs::remove_dir_all(dir).unwrap();
        }
    }
    #[test]
    fn a_healthy_or_missing_file_is_left_exactly_as_it_is() {
        let dir = scratch();
        let missing = load_or_repair(&dir).unwrap();
        assert_eq!(missing.preferences, Preferences::default());
        assert!(missing.reset.is_empty() && !missing.unreadable && missing.kept.is_none());
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0, "nothing is created");
        // An older, shorter file is filled in from the defaults without a rewrite.
        for healthy in [
            r#"{"widgetX":-400,"futureSetting":true}"#,
            r#"{"theme":"future-theme","widget":{"weeks":6,"pastWeeks":2}}"#,
        ] {
            let loaded = stored(&dir, healthy);
            assert!(loaded.reset.is_empty() && !loaded.unreadable, "{healthy}");
            assert!(loaded.kept.is_none());
            assert_eq!(
                fs::read_to_string(dir.join("preferences.json")).unwrap(),
                healthy
            );
            assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        }
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn whatever_the_file_says_what_the_app_gets_is_valid() {
        let dir = scratch();
        for nasty in [
            r#"{"widgetOpacity":-1,"widgetX":"left","widgetY":1e99,"theme":7}"#,
            r##"{"widget":{"width":99999,"height":-5,"weeks":0,"pastWeeks":9,"dateSize":300,"eventSize":0,"cellGap":255,"cellPadding":21,"lineGap":13,"font":"","dark":{"text":"red","today":"#12345G"},"light":[]}}"##,
            r#"{"widget":{"font":"a\u0007b","rolling":"sometimes","dark":null}}"#,
            r#"{"widget":null,"widgetLocked":null,"widgetVisible":0}"#,
        ] {
            let loaded = stored(&dir, nasty);
            assert!(validate(&loaded.preferences).is_ok(), "{nasty}");
            assert!(!loaded.reset.is_empty(), "{nasty}");
            assert!(load(&dir).is_ok(), "{nasty}: the file was not put right");
        }
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn a_number_too_large_to_read_costs_only_its_own_setting() {
        // JSON allows it, but no number type can hold it. Reading the whole
        // file as one value fails on it, wherever it stands.
        let dir = scratch();
        let loaded = stored(
            &dir,
            r##"{"theme":"porcelain","widgetX":-400,"widgetOpacity":1e400,"futureSetting":1e400,"widget":{"weeks":5,"width":1e400,"dark":{"text":"#FAFAFA","today":1e400}}}"##,
        );
        let p = &loaded.preferences;
        assert!(!loaded.unreadable, "the whole file was given up");
        assert_eq!(p.theme, AppTheme::Porcelain);
        assert_eq!(p.widget_x, Some(-400));
        assert_eq!(p.widget.weeks, 5);
        assert_eq!(p.widget.dark.text.as_deref(), Some("#FAFAFA"));
        assert_eq!(
            loaded.reset,
            ["widget.dark.today", "widget.width", "widgetOpacity"]
        );
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn when_the_original_cannot_be_kept_the_file_stays_and_the_user_is_told() {
        let dir = scratch();
        let original = r#"{"widgetX":7,"widgetOpacity":250}"#;
        // Something else has the copy's name and cannot be written over.
        fs::create_dir(dir.join("preferences.invalid.json")).unwrap();
        let loaded = stored(&dir, original);
        assert_eq!(loaded.preferences.widget_x, Some(7));
        assert_eq!(loaded.preferences.widget_opacity, 82);
        assert!(loaded.kept.is_none());
        assert_eq!(
            fs::read_to_string(dir.join("preferences.json")).unwrap(),
            original,
            "rewritten although the original could not be kept"
        );
        let notice = loaded.notice().unwrap();
        assert!(notice.contains("사본을 남기지 못"), "{notice}");
        assert!(notice.contains("widgetOpacity"), "{notice}");
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn a_copy_that_fails_part_way_leaves_the_earlier_copy_whole() {
        let dir = scratch();
        let first = r#"{"widgetOpacity":250}"#;
        let kept = stored(&dir, first).kept.unwrap();
        assert_eq!(fs::read_to_string(&kept).unwrap(), first);
        // The next copy cannot be written: its working file's name is taken.
        fs::create_dir(dir.join("preferences.invalid.json.tmp")).unwrap();
        let second = stored(&dir, r#"{"widgetOpacity":251}"#);
        assert!(second.kept.is_none());
        assert_eq!(
            fs::read_to_string(&kept).unwrap(),
            first,
            "the earlier copy was emptied or half written"
        );
        // Once it can be written again, the newer original takes its place.
        fs::remove_dir(dir.join("preferences.invalid.json.tmp")).unwrap();
        let third = stored(&dir, r#"{"widgetOpacity":252}"#);
        assert_eq!(
            fs::read_to_string(third.kept.unwrap()).unwrap(),
            r#"{"widgetOpacity":252}"#
        );
        let names: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            !names.iter().any(|name| name.ends_with(".tmp")),
            "{names:?}"
        );
        fs::remove_dir_all(dir).unwrap();
    }
}
