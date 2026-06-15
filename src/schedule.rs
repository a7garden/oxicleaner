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
        binary = binary,
        days = days,
        root = root,
        weekday = weekday,
        hour = hour,
        log_out = log_out,
        log_err = log_err,
        path_env = path_env,
        home = home,
    )
}

fn uid() -> Result<String> {
    let out = Command::new("id")
        .arg("-u")
        .output()
        .context("id -u 실행 실패")?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}
