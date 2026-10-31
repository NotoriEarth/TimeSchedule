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
    value.widget.validate()?;
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
}
