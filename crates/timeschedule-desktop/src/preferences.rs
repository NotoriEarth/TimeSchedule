use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Preferences {
    pub widget_x: Option<i32>,
    pub widget_y: Option<i32>,
    pub widget_locked: bool,
    pub widget_visible: bool,
    pub widget_opacity: u8,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            widget_x: None,
            widget_y: None,
            widget_locked: true,
            widget_visible: true,
            widget_opacity: 82,
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
fn validate(value: &Preferences) -> Result<(), String> {
    if !(40..=100).contains(&value.widget_opacity) {
        Err("위젯 불투명도는 40~100%여야 합니다.".into())
    } else {
        Ok(())
    }
}
pub fn save(directory: &Path, value: &Preferences) -> Result<(), String> {
    use std::io::Write;
    validate(value)?;
    let temporary = directory.join("preferences.json.tmp");
    let mut file = fs::File::create(&temporary).map_err(|e| e.to_string())?;
    file.write_all(&serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);
    fs::rename(temporary, directory.join("preferences.json")).map_err(|e| e.to_string())
}
#[cfg(test)]
mod tests {
    use super::*;
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
            ..Preferences::default()
        };
        save(&dir, &changed).unwrap();
        assert_eq!(load(&dir).unwrap(), changed);
        fs::remove_file(dir.join("preferences.json")).unwrap();
        fs::remove_dir(dir).unwrap();
    }
}
