//! One power setting shared by the GUI and separately launched MCP helpers.
use serde::{Deserialize, Serialize};
use std::{fs, io, path::Path};

const FILE: &str = "mcp-power.json";

#[derive(Serialize, Deserialize, PartialEq, Eq)]
pub struct Power {
    pub enabled: bool,
    // A bridge from before an off/on cycle must close even if it missed "off".
    generation: String,
}

impl Power {
    fn read(dir: &Path) -> Result<Option<Self>, String> {
        match fs::read(dir.join(FILE)) {
            Ok(bytes) => serde_json::from_slice(&bytes).map(Some).map_err(|_| {
                "MCP 전원 설정을 읽지 못했습니다. 설정에서 전원을 다시 선택하세요.".into()
            }),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(format!("MCP 전원 설정 읽기 실패 · {error}")),
        }
    }

    fn legacy() -> Self {
        Self {
            enabled: std::env::var("TIMESCHEDULE_MCP_DISABLED").as_deref() != Ok("1"),
            generation: "initial".into(),
        }
    }

    pub fn for_helper(dir: &Path) -> Result<Self, String> {
        Ok(Self::read(dir)?.unwrap_or_else(Self::legacy))
    }

    /// Only called by the GUI holding the instance lock. Once initialized,
    /// even a Claude process with a different inherited environment agrees.
    pub fn initialize(dir: &Path) -> Result<Self, String> {
        if let Some(power) = Self::read(dir)? {
            return Ok(power);
        }
        let power = Self::legacy();
        power.save(dir)?;
        Ok(power)
    }

    pub fn set(dir: &Path, enabled: bool) -> Result<Self, String> {
        let power = Self {
            enabled,
            generation: uuid::Uuid::new_v4().to_string(),
        };
        power.save(dir)?;
        Ok(power)
    }

    fn save(&self, dir: &Path) -> Result<(), String> {
        crate::mcp::write_json_atomic(&dir.join(FILE), self)
            .map_err(|error| format!("MCP 전원 설정 저장 실패 · {error}"))
    }
}
