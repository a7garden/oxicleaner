//! 정리 이력 관리 (`~/.oxicleaner/history.jsonl`).

use anyhow::Result;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use crate::config::config_dir;
use crate::sweep::SweepReport;

/// 히스토리 파일 경로: `~/.oxicleaner/history.jsonl`
pub fn history_path() -> PathBuf {
    config_dir().join("history.jsonl")
}

/// 실행 리포트를 히스토리에 한 줄(JSONL) 추가한다.
pub fn append(report: &SweepReport) -> Result<()> {
    let path = history_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let line = serde_json::to_string(report)?;
    let mut f = OpenOptions::new().create(true).append(true).open(&path)?;
    writeln!(f, "{line}")?;
    Ok(())
}

/// 최근 `limit` 건의 이력을 최신순으로 불러온다.
pub fn read(limit: usize) -> Result<Vec<SweepReport>> {
    let path = history_path();
    if !path.exists() {
        return Ok(vec![]);
    }
    let s = fs::read_to_string(&path)?;
    let mut records: Vec<SweepReport> = s
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    records.reverse(); // 최신 먼저
    records.truncate(limit);
    Ok(records)
}
