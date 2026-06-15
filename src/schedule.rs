//! launchd 스케줄 자동 관리. plist 를 생성하고 bootstrap/bootout 한다.

use anyhow::{bail, Context, Result};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// launchd 라벨 (plist Label 및 launchctl 식별자).
pub const LABEL: &str = "local.oxicleaner";

/// plist 파일 경로: `~/Library/LaunchAgents/local.oxicleaner.plist`
pub fn plist_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Library/LaunchAgents/local.oxicleaner.plist")
}

/// launchd 로그 디렉토리: `~/Library/Logs/oxicleaner/`
pub fn log_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Library/Logs/oxicleaner")
}

/// 스케줄을 활성화(설치/갱신) 한다.
///
/// - `weekday`: 0(일) ~ 6(토)
/// - `hour`: 0 ~ 23
/// - `days`: 보존 일수 (sweep 에 전달)
/// - `root`: 재귀 스캔 루트
/// - `binary`: oxicleaner 실행파일 절대경로 (plist 에 박힘)
pub fn enable(weekday: u32, hour: u32, days: u32, root: &str, binary: &str) -> Result<()> {
    if weekday > 6 {
        bail!("weekday 는 0(일)~6(토) 이어야 합니다: {weekday}");
    }
    if hour > 23 {
        bail!("hour 는 0~23 이어야 합니다: {hour}");
    }

    // 기존 스케줄이 있으면 먼저 언로드 (갱신 용이). 조용히 처리.
    let _ = disable();

    fs::create_dir_all(log_dir())?;

    let plist = render_plist(weekday, hour, days, root, binary);
    let path = plist_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, &plist)?;

    let uid = uid()?;
    let domain = format!("gui/{uid}");
    let status = Command::new("launchctl")
        .args(["bootstrap", &domain, &path.to_string_lossy()])
        .status()
        .context("launchctl bootstrap 실행 실패")?;
    if !status.success() {
        bail!(
            "launchctl bootstrap 실패 — `{}` 를 수동 점검하세요",
            path.display()
        );
    }
    Ok(())
}

/// 스케줄을 비활성화(제거)한다. plist 파일도 삭제. (로드되어 있지 않아도 조용히 통과.)
pub fn disable() -> Result<()> {
    let uid = uid()?;
    let target = format!("gui/{uid}/{LABEL}");
    // bootout 은 이미 로드되어 있지 않으면 에러를 내고 stdout/stderr 에
    // 노이즈를 출력한다. output() 으로 받아 무시한다.
    let _ = Command::new("launchctl")
        .args(["bootout", &target])
        .output();
    let p = plist_path();
    if p.exists() {
        fs::remove_file(&p)?;
    }
    Ok(())
}

/// 스케줄이 현재 launchd 에 로드되어 있는지.
pub fn is_loaded() -> bool {
    let uid = match uid() {
        Ok(u) => u,
        Err(_) => return false,
    };
    let out = Command::new("launchctl")
        .args(["print", &format!("gui/{uid}/{LABEL}")])
        .output();
    match out {
        Ok(o) => o.status.success(),
        Err(_) => false,
    }
}

fn render_plist(weekday: u32, hour: u32, days: u32, root: &str, binary: &str) -> String {
    let home = dirs::home_dir()
        .map(|h| h.to_string_lossy().into_owned())
        .unwrap_or_default();
    let path_env = format!("{home}/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin");
    let log_out = log_dir()
        .join("launchd.out.log")
        .to_string_lossy()
        .into_owned();
    let log_err = log_dir()
        .join("launchd.err.log")
        .to_string_lossy()
        .into_owned();

    // XML 엔티티 이스케이프 — 경로/바이너리에 & < > " 가 있으면 plist 가 깨진다.
    fn esc(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    }
    let binary_esc = esc(binary);
    let root_esc = esc(root);
    let home_esc = esc(&home);
    let path_env_esc = esc(&path_env);

    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{label}</string>

    <key>ProgramArguments</key>
    <array>
        <string>{binary}</string>
        <string>sweep</string>
        <string>--days</string>
        <string>{days}</string>
        <string>--root</string>
        <string>{root}</string>
    </array>

    <key>StartCalendarInterval</key>
    <dict>
        <key>Weekday</key>
        <integer>{weekday}</integer>
        <key>Hour</key>
        <integer>{hour}</integer>
        <key>Minute</key>
        <integer>0</integer>
    </dict>

    <key>ProcessType</key>
    <string>Background</string>
    <key>LowPriorityIO</key>
    <true/>

    <key>StandardOutPath</key>
    <string>{log_out}</string>
    <key>StandardErrorPath</key>
    <string>{log_err}</string>

    <key>EnvironmentVariables</key>
    <dict>
        <key>PATH</key>
        <string>{path_env}</string>
        <key>HOME</key>
        <string>{home}</string>
    </dict>
</dict>
</plist>
"#,
        label = LABEL,
        binary = binary_esc,
        days = days,
        root = root_esc,
        weekday = weekday,
        hour = hour,
        log_out = log_out,
        log_err = log_err,
        path_env = path_env_esc,
        home = home_esc,
    )
}

fn uid() -> Result<String> {
    let out = Command::new("id")
        .arg("-u")
        .output()
        .context("id -u 실행 실패")?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// render_plist: 반드시 유효한 XML plist 여야 한다.
    /// XML 선언, DOCTYPE, plist/schema 가 포함되어야 하고 인자가 위치에 맞게 삽입되어야 함.
    #[test]
    fn test_render_plist_basic() {
        let plist = render_plist(0, 3, 30, "/Volumes/MERCURY/PROJECTS", "/usr/bin/oxicleaner");
        assert!(plist.starts_with(r#"<?xml"#));
        assert!(plist.contains("<integer>0</integer>"), "weekday 0");
        assert!(plist.contains("<integer>3</integer>"), "hour 3");
        // days 는 ProgramArguments 에서 <string> 으로 쓰임
        assert!(plist.contains("<string>30</string>"), "days 30");
        assert!(plist.contains("ProgramArguments"));
        assert!(plist.contains("StartCalendarInterval"));
        assert!(plist.contains("EnvironmentVariables"));
    }

    /// XML escape: 특수 문자가 올바르게 변환되는지.
    #[test]
    fn test_render_plist_xml_escape() {
        let binary = "/Users/x&y/bin/<oxicleaner>\".exe";
        let root = "/path/with/\"quotes\"&ampersands";
        let plist = render_plist(0, 3, 30, root, binary);
        // 원래 문자는 XML 에 없어야 함
        assert!(!plist.contains("&y"), "& 는 &amp; 로 이스케이프되어야 함");
        assert!(
            !plist.contains("<oxicleaner"),
            "< 는 &lt; 로 이스케이프되어야 함"
        );
        assert!(
            !plist.contains("\"quotes"),
            "\" 는 &quot; 로 이스케이프되어야 함"
        );
        // 이스케이프된 버전이 있어야 함
        assert!(plist.contains("x&amp;y"));
        assert!(plist.contains("&lt;oxicleaner"));
        assert!(plist.contains("&quot;quotes"));
        assert!(
            plist.contains("&amp;ampersands"),
            "&amp; 의 & 도 이스케이프 필요"
        );
    }
}
