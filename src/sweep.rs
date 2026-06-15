//! cargo-sweep 래핑: target/ 에서 오래된 산물을 정리하고 결과를 파싱한다.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{disk, safety};

/// 단일 프로젝트의 정리 결과.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SweptProject {
    /// target 디렉토리 경로.
    pub path: String,
    /// 확보한 크기 (예: "516.33 MiB"). 정리 대상 없으면 None.
    pub freed: Option<String>,
}

/// 한 번의 sweep 실행 리포트. 히스토리(JSONL)에 한 줄로 저장된다.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SweepReport {
    /// 실행 시각 (RFC3339).
    pub timestamp: String,
    /// "dry-run" | "live"
    pub mode: String,
    /// 보존 일수.
    pub days: u32,
    /// 스캔 루트.
    pub root: String,
    /// 프로젝트별 결과.
    pub projects: Vec<SweptProject>,
    /// 총 확보량 (예: "42.47 GiB").
    pub total_freed: Option<String>,
    /// 디스크 사용 현황 (실행 전).
    pub disk_before: String,
    /// 디스크 사용 현황 (실행 후).
    pub disk_after: String,
    /// 빌드 중 등으로 스킵했는지.
    pub skipped: bool,
    /// 스킵 사유.
    pub skip_reason: Option<String>,
}

/// sweep 을 실행한다.
///
/// - `dry_run=true` 면 삭제 없이 미리보기.
/// - `force=false` 면 `root` 하위에서 빌드가 돌고 있을 때 스킵.
pub fn run(root: &Path, days: u32, dry_run: bool, force: bool) -> Result<SweepReport> {
    let disk_before = disk::stat(root);

    // 안전장치: 빌드 중이면 스킵.
    if !force && !dry_run {
        let builds = safety::detect_active_builds(root)?;
        if !builds.is_empty() {
            return Ok(SweepReport {
                timestamp: now_iso(),
                mode: mode_str(dry_run).into(),
                days,
                root: root.to_string_lossy().into_owned(),
                projects: vec![],
                total_freed: None,
                disk_before: disk_before.clone(),
                disk_after: disk_before,
                skipped: true,
                skip_reason: Some(format!("{}개 프로세스 빌드 중", builds.len())),
            });
        }
    }

    let sweep_bin = find_cargo_sweep()?;

    let mut cmd = Command::new(&sweep_bin);
    cmd.arg("sweep")
        .arg("--recursive")
        .arg("--time")
        .arg(days.to_string())
        .arg(root);
    if dry_run {
        cmd.arg("--dry-run");
    }
    cmd.env("PATH", enhanced_path());

    let output = cmd
        .output()
        .with_context(|| format!("cargo-sweep 실행 실패: {}", sweep_bin.display()))?;

    if !output.status.success() {
        bail!(
            "cargo-sweep 실패 (exit {:?}): {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let (projects, total) = parse_output(&stdout);

    let disk_after = disk::stat(root);

    Ok(SweepReport {
        timestamp: now_iso(),
        mode: mode_str(dry_run).into(),
        days,
        root: root.to_string_lossy().into_owned(),
        projects,
        total_freed: total,
        disk_before,
        disk_after,
        skipped: false,
        skip_reason: None,
    })
}

/// cargo-sweep stdout 을 파싱한다.
///
/// 예상 형태:
///   [INFO] Searching recursively for Rust project folders
///   [INFO] Would clean: 516.33 MiB from "/path/target"   (dry-run)
///   [INFO] Cleaned 516.33 MiB from "/path/target"        (live)
///   [INFO] Cleaned nothing from "/path/target"
///   [INFO] Total amount: 42.47 GiB
fn parse_output(stdout: &str) -> (Vec<SweptProject>, Option<String>) {
    let mut projects = Vec::new();
    let mut total = None;

    for line in stdout.lines() {
        let l = line.trim();
        let Some(rest) = l.strip_prefix("[INFO] ") else {
            continue;
        };

        if let Some(t) = rest.strip_prefix("Total amount: ") {
            total = Some(t.trim().to_string());
            continue;
        }

        // "... from "/path""  분할
        if let Some((action, path_part)) = rest.split_once(" from \"") {
            let path = path_part.trim_end_matches('"');
            let freed = if action.contains("nothing") {
                None
            } else {
                // dry-run: "Would clean: 516.33 MiB"  →  ": " 뒤
                // live:   "Cleaned 516.33 MiB"       →  공백 뒤
                action
                    .split_once(": ")
                    .or_else(|| action.split_once(' '))
                    .map(|(_, s)| s.trim().to_string())
            };
            projects.push(SweptProject {
                path: path.to_string(),
                freed,
            });
        }
    }

    (projects, total)
}

/// cargo-sweep 바이너리를 찾는다: PATH → ~/.cargo/bin/cargo-sweep 순.
pub fn find_cargo_sweep() -> Result<PathBuf> {
    if let Ok(p) = which("cargo-sweep") {
        return Ok(p);
    }
    if let Some(home) = dirs::home_dir() {
        let candidate = home.join(".cargo/bin/cargo-sweep");
        if candidate.exists() {
            return Ok(candidate);
        }
    }
    bail!("cargo-sweep 을 찾을 수 없습니다. 다음으로 설치하세요:\n  cargo install cargo-sweep")
}

fn which(bin: &str) -> Result<PathBuf> {
    let out = Command::new("/usr/bin/which").arg(bin).output()?;
    if !out.status.success() {
        bail!("not found: {bin}");
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        bail!("not found: {bin}");
    }
    Ok(PathBuf::from(s))
}

/// launchd 좁은 PATH 보정용. ~/.cargo/bin 을 최우선.
fn enhanced_path() -> String {
    let mut parts: Vec<String> = vec![];
    if let Some(home) = dirs::home_dir() {
        parts.push(home.join(".cargo/bin").to_string_lossy().into_owned());
    }
    parts.push("/opt/homebrew/bin".into());
    parts.push("/usr/local/bin".into());
    parts.push("/usr/bin".into());
    parts.push("/bin".into());
    if let Ok(p) = std::env::var("PATH") {
        parts.push(p);
    }
    parts.join(":")
}

fn now_iso() -> String {
    chrono::Local::now().to_rfc3339()
}

fn mode_str(dry_run: bool) -> &'static str {
    if dry_run {
        "dry-run"
    } else {
        "live"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_output_dry_run() {
        let stdout = "[INFO] Would clean: 516.33 MiB from \"/Volumes/MERCURY/PROJECTS/cardion/target\"\n[INFO] Total amount: 516.33 MiB\n";
        let (projects, total) = parse_output(stdout);
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].path, "/Volumes/MERCURY/PROJECTS/cardion/target");
        assert_eq!(projects[0].freed.as_deref(), Some("516.33 MiB"));
        assert_eq!(total.as_deref(), Some("516.33 MiB"));
    }

    #[test]
    fn test_parse_output_live() {
        let stdout = "[INFO] Cleaned 10.10 GiB from \"/Volumes/MERCURY/PROJECTS/oxibrowser/target\"\n[INFO] Total amount: 10.10 GiB\n";
        let (projects, total) = parse_output(stdout);
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].freed.as_deref(), Some("10.10 GiB"));
        assert_eq!(total.as_deref(), Some("10.10 GiB"));
    }

    #[test]
    fn test_parse_output_nothing() {
        let stdout = "[INFO] Cleaned nothing from \"/Volumes/MERCURY/PROJECTS/oxi/target\"\n[INFO] Total amount: 0.00 B\n";
        let (projects, total) = parse_output(stdout);
        assert_eq!(projects.len(), 1);
        assert!(projects[0].freed.is_none());
        assert_eq!(total.as_deref(), Some("0.00 B"));
    }

    #[test]
    fn test_parse_output_multiple_projects() {
        let stdout = "\
[INFO] Cleaned 1.00 GiB from \"/a/target\"
[INFO] Cleaned nothing from \"/b/target\"
[INFO] Cleaned 2.00 GiB from \"/c/target\"
[INFO] Total amount: 3.00 GiB
";
        let (projects, total) = parse_output(stdout);
        assert_eq!(projects.len(), 3);
        assert_eq!(projects[0].freed.as_deref(), Some("1.00 GiB"));
        assert!(projects[1].freed.is_none());
        assert_eq!(projects[2].freed.as_deref(), Some("2.00 GiB"));
        assert_eq!(total.as_deref(), Some("3.00 GiB"));
    }

    #[test]
    fn test_mode_str() {
        assert_eq!(mode_str(true), "dry-run");
        assert_eq!(mode_str(false), "live");
    }

    #[test]
    fn test_find_cargo_sweep_returns_path_or_error() {
        // cargo-sweep 이 설치되어 있으면 Ok, 없으면 Err (둘 다 허용).
        match find_cargo_sweep() {
            Ok(p) => assert!(p.exists()),
            Err(e) => assert!(
                e.to_string().contains("설치"),
                "에러 메시지에 설치 안내가 있어야 함: {e}"
            ),
        }
    }
}
