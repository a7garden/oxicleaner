//! ~/.oxicleaner/ 설정 관리.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// oxicleaner 설정. `~/.oxicleaner/config.toml` 에 저장된다.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// 재귀 스캔 루트 디렉토리.
    pub root: PathBuf,
    /// 보존 일수. 이 기간 이내에 사용된 산물은 삭제하지 않는다.
    #[serde(default = "default_days")]
    pub days: u32,
}

fn default_days() -> u32 {
    30
}

impl Default for Config {
    fn default() -> Self {
        Self {
            root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            days: 30,
        }
    }
}

/// 설정 디렉토리: `~/.oxicleaner/`
pub fn config_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".oxicleaner")
}

/// 설정 파일 경로: `~/.oxicleaner/config.toml`
pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

/// 설정을 불러온다. 파일이 없으면 `None`.
pub fn load() -> Result<Option<Config>> {
    let p = config_path();
    if !p.exists() {
        return Ok(None);
    }
    let s = fs::read_to_string(&p)?;
    let cfg: Config = toml::from_str(&s)?;
    Ok(Some(cfg))
}

/// 설정을 저장한다.
pub fn save(cfg: &Config) -> Result<()> {
    let dir = config_dir();
    fs::create_dir_all(&dir)?;
    let s = toml::to_string_pretty(cfg)?;
    fs::write(config_path(), s)?;
    Ok(())
}

/// CLI 인자로 주어진 값이 있으면 그것을, 없으면 config.toml 의 값을 쓴다.
/// 둘 다 없으면 기본값.
pub fn resolve(cli_root: Option<&Path>, cli_days: Option<u32>) -> (PathBuf, u32) {
    let cfg = load().ok().flatten();
    let root = cli_root
        .map(PathBuf::from)
        .or_else(|| cfg.as_ref().map(|c| c.root.clone()))
        .unwrap_or_else(|| Config::default().root);
    let days = cli_days
        .or_else(|| cfg.as_ref().map(|c| c.days))
        .unwrap_or(30);
    (root, days)
}
