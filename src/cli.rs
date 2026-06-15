//! CLI 정의 (clap) 및 명령 디스패치.

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};

use crate::{config, history, schedule, sweep};

#[derive(Parser)]
#[command(
    name = "oxicleaner",
    version,
    about = "Recursive Rust target/ cleaner with launchd scheduling",
    long_about = None,
    // 서브커맨드 없이 `oxicleaner` 만 쳐도 sweep 이 실행된다.
    subcommand_required = false,
)]
pub struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// 스캔 루트 (기본: config.toml 의 root). install/sweep 공통.
    #[arg(long, short, global = true)]
    root: Option<PathBuf>,
}

#[derive(Subcommand)]
pub enum Command {
    /// target/ 정리 실행 (서브커맨드 없이 `oxicleaner` 만 쳐도 동일).
    Sweep {
        /// 보존 일수 — 이 기간 이내 산물은 유지.
        #[arg(long, default_value_t = 30)]
        days: u32,
        /// 삭제 없이 미리보기.
        #[arg(long)]
        dry_run: bool,
        /// 빌드 중이어도 강제 실행.
        #[arg(long)]
        force: bool,
    },

    /// launchd 스케줄 설치/갱신.
    Install {
        /// 요일: 0(일) ~ 6(토). 기본 일요일.
        #[arg(long, default_value_t = 0)]
        weekday: u32,
        /// 시각(시): 0 ~ 23. 기본 새벽 3시.
        #[arg(long, default_value_t = 3)]
        hour: u32,
        /// 보존 일수.
        #[arg(long, default_value_t = 30)]
        days: u32,
    },

    /// launchd 스케줄 제거.
    Uninstall,

    /// 스케줄 로드 여부 + 마지막 정리 결과.
    Status,

    /// 정리 이력 조회.
    History {
        /// 표시할 건수.
        #[arg(long, short, default_value_t = 10)]
        limit: usize,
    },
}

impl Cli {
    pub fn run(self) -> Result<()> {
        match self.command {
            None => {
                let (root, days) = config::resolve(self.root.as_deref(), None);
                run_sweep(&root, days, false, false)
            }
            Some(Command::Sweep {
                days,
                dry_run,
                force,
            }) => {
                let (root, cfg_days) = config::resolve(self.root.as_deref(), Some(days));
                run_sweep(&root, cfg_days, dry_run, force)
            }
            Some(Command::Install {
                weekday,
                hour,
                days,
            }) => {
                let (root, cfg_days) = config::resolve(self.root.as_deref(), Some(days));
                let cfg = config::Config {
                    root: root.clone(),
                    days: cfg_days,
                };
                config::save(&cfg)?;

                // 핵심: 스케줄러가 가리킬 바이너리를 ~/.oxicleaner/oxicleaner 로 복사한다.
                // oxicleaner 자신의 target/ 도 sweep 대상이므로 target/ 안의 바이너리를
                // 직접 가리키면 자기 자신을 지워버리는 사고가 발생한다.
                let installed_bin = install_self_binary()?;
                schedule::install(
                    weekday,
                    hour,
                    cfg_days,
                    &root.to_string_lossy(),
                    &installed_bin.to_string_lossy(),
                )?;

                println!("✅ 스케줄 설치 완료");
                println!("   주기    : 매주 {} {:02}:00", weekday_name(weekday), hour);
                println!("   루트    : {}", root.display());
                println!("   보존    : {}일", cfg_days);
                println!("   바이너리: {}", installed_bin.display());
                println!("   로그    : {}/", schedule::log_dir().display());
                println!();
                println!("   확인    : oxicleaner status");
                println!("   즉시실행: launchctl start {}", schedule::LABEL);
                Ok(())
            }
            Some(Command::Uninstall) => {
                schedule::uninstall()?;
                println!("✅ 스케줄 제거 완료 ({})", schedule::LABEL);
                Ok(())
            }
            Some(Command::Status) => {
                if schedule::is_loaded() {
                    println!("스케줄: ✅ 로드됨 ({})", schedule::LABEL);
                    println!("  plist: {}", schedule::plist_path().display());
                } else {
                    println!("스케줄: ❌ 미설치");
                }
                println!();
                match history::read(1) {
                    Ok(recs) if !recs.is_empty() => {
                        let last = &recs[0];
                        println!("마지막 실행: {}", last.timestamp);
                        println!("  모드: {}", last.mode);
                        if last.skipped {
                            println!(
                                "  결과: 스킵 — {}",
                                last.skip_reason.as_deref().unwrap_or("?")
                            );
                        } else if let Some(t) = &last.total_freed {
                            println!(
                                "  확보: {} (disk: {} → {})",
                                t, last.disk_before, last.disk_after
                            );
                        }
                    }
                    _ => println!("실행 이력 없음"),
                }
                Ok(())
            }
            Some(Command::History { limit }) => {
                let recs = history::read(limit)?;
                if recs.is_empty() {
                    println!("이력 없음");
                    return Ok(());
                }
                println!("{:<19} {:<8} {:>12}  disk 변화", "시각", "모드", "확보");
                println!("{}", "-".repeat(70));
                for r in recs {
                    let total = r.total_freed.clone().unwrap_or_else(|| {
                        if r.skipped {
                            "SKIP".into()
                        } else {
                            "0".into()
                        }
                    });
                    let disk = if r.skipped {
                        r.disk_before.clone()
                    } else {
                        format!("{} → {}", r.disk_before, r.disk_after)
                    };
                    println!(
                        "{:<19} {:<8} {:>12}  {}",
                        fmt_ts(&r.timestamp),
                        r.mode,
                        total,
                        disk
                    );
                }
                Ok(())
            }
        }
    }
}

/// sweep 실행 + 리포트 출력 + 히스토리 기록.
fn run_sweep(root: &Path, days: u32, dry_run: bool, force: bool) -> Result<()> {
    eprintln!(
        "oxicleaner: root={}, keep={}d, mode={}",
        root.display(),
        days,
        if dry_run { "dry-run" } else { "live" }
    );

    let report = sweep::run(root, days, dry_run, force)?;

    if report.skipped {
        println!(
            "⏭  스킵: {}",
            report.skip_reason.as_deref().unwrap_or("(사유 없음)")
        );
        // 스킵도 이력에 남긴다 (왜 안 돌았는지 추적).
        history::append(&report)?;
        return Ok(());
    }

    println!();
    for p in &report.projects {
        match &p.freed {
            Some(f) => println!("  ✓ {:>10}  {}", f, p.path),
            None => println!("    {:>10}  {}", "—", p.path),
        }
    }

    println!();
    if let Some(t) = &report.total_freed {
        println!(
            "총 확보: {}   (disk: {} → {})",
            t, report.disk_before, report.disk_after
        );
    } else {
        println!("정리 대상 없음   (disk: {})", report.disk_before);
    }

    if !dry_run {
        history::append(&report)?;
    }
    Ok(())
}

fn weekday_name(weekday: u32) -> &'static str {
    match weekday {
        0 => "일요일",
        1 => "월요일",
        2 => "화요일",
        3 => "수요일",
        4 => "목요일",
        5 => "금요일",
        6 => "토요일",
        _ => "?",
    }
}

/// RFC3339 타임스탬프를 "YYYY-MM-DD HH:MM" 으로 간소화. 파싱 실패시 원본 반환.
fn fmt_ts(ts: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(ts)
        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|_| ts.to_string())
}

/// 현재 실행 중인 oxicleaner 바이너리를 `~/.oxicleaner/oxicleaner` 로 복사한다.
///
/// 스케줄러(sweep)가 프로젝트 target/ 안의 바이너리를 직접 가리키면
/// 자기 자신을 지워버리는 사고가 생긴다. config 디렉토리는 sweep 대상이
/// 아니므로 안전하다.
fn install_self_binary() -> Result<PathBuf> {
    let src = std::env::current_exe()?;
    let dest = config::config_dir().join("oxicleaner");
    std::fs::create_dir_all(config::config_dir())?;
    std::fs::copy(&src, &dest)?;
    // 실행 권한 보장.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&dest)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&dest, perms)?;
    }
    Ok(dest)
}
