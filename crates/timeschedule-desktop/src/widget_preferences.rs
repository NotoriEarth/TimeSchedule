//! Portable widget settings; no rendering or HWND types in persisted JSON.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ColorOverrides {
    pub background: Option<String>,
    pub text: Option<String>,
    pub today: Option<String>,
    pub sunday: Option<String>,
    pub saturday: Option<String>,
    pub border: Option<String>,
}

pub fn valid_rgb(value: &str) -> bool {
    value.len() == 7
        && value.starts_with('#')
        && value.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WidgetPreferences {
    pub width: u32,
    pub height: u32,
    pub rolling: bool,
    pub weeks: u8,
    pub past_weeks: u8,
    pub font: String,
    pub date_size: u8,
    pub event_size: u8,
    pub cell_gap: u8,
    pub cell_padding: u8,
    pub line_gap: u8,
    pub dark: ColorOverrides,
    pub light: ColorOverrides,
}
impl Default for WidgetPreferences {
    fn default() -> Self {
        Self {
            width: 960,
            height: 640,
            rolling: false,
            weeks: 4,
            past_weeks: 0,
            font: "Malgun Gothic".into(),
            date_size: 15,
            event_size: 11,
            cell_gap: 1,
            cell_padding: 6,
            line_gap: 3,
            dark: ColorOverrides::default(),
            light: ColorOverrides::default(),
        }
    }
}
impl WidgetPreferences {
    pub fn validate(&self) -> Result<(), String> {
        if !(320..=3840).contains(&self.width) || !(220..=2160).contains(&self.height) {
            return Err("위젯 크기는 가로 320~3840, 세로 220~2160으로 입력하세요.".into());
        }
        if !(2..=8).contains(&self.weeks) || self.past_weeks > 2 || self.past_weeks >= self.weeks {
            return Err("표시 주 수와 지난주 수가 유효하지 않습니다.".into());
        }
        if !(10..=32).contains(&self.date_size)
            || !(9..=24).contains(&self.event_size)
            || self.cell_gap > 16
            || self.cell_padding > 20
            || self.line_gap > 12
        {
            return Err("글자 크기 또는 셀 간격이 허용 범위를 벗어났습니다.".into());
        }
        if self.font.trim().is_empty()
            || self.font.len() > 256
            || self.font.chars().any(char::is_control)
        {
            return Err("글꼴 이름이 유효하지 않습니다.".into());
        }
        for colors in [&self.dark, &self.light] {
            for rgb in [
                &colors.background,
                &colors.text,
                &colors.today,
                &colors.sunday,
                &colors.saturday,
                &colors.border,
            ]
            .into_iter()
            .flatten()
            {
                if !valid_rgb(rgb) {
                    return Err("색상은 #RRGGBB 형식으로 입력하세요.".into());
                }
            }
        }
        Ok(())
    }
}
