//! 인터랙티브 설정 마법사 (`oxicleaner setup`).
//!
//! inquire 로 화살표 키 기반 TUI 를 구성한다. 흐름:
//!   환영 → 루트 → 보존일수 → 스케줄 켜기 → (요일/시각/즉시실행)
//!   → 요약 → 최종 확인 → 적용(config 저장 + enable + 옵션 sweep).

use anyhow::Result;
use inquire::validator::{CustomTypeValidator, StringValidator, Validation};
use inquire::{Confirm, CustomType, Select, Text};
use std::path::PathBuf;

use crate::{config, disk, history, schedule, sweep};

/// 마법사가 수집한 설정.
struct Setup {
    root: PathBuf,
    days: u32,
    schedule: bool,
    weekday: u32,
    hour: u32,
    run_now: bool,
}

/// 요일 옵션 (표시 텍스트, 정수값). Select 항목으로 쓰인다.
#[derive(Clone, Copy)]
struct WeekdayOption {
    name: &'static str,
    value: u32,
}

const WEEKDAYS: [WeekdayOption; 7] = [
    WeekdayOption {
        name: "일요일",
        value: 0,
    },
    WeekdayOption {
        name: "월요일",
        value: 1,
    },
    WeekdayOption {
        name: "화요일",
        value: 2,
    },
    WeekdayOption {
        name: "수요일",
        value: 3,
    },
    WeekdayOption {
        name: "목요일",
        value: 4,
    },
    WeekdayOption {
        name: "금요일",
        value: 5,
    },
    WeekdayOption {
        name: "토요일",
        value: 6,
    },
];

impl std::fmt::Display for WeekdayOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

/// 보존 일수 옵션 한 줄.
struct DaysOption {
    label: &'static str,
    value: u32,
    hint: &'static str,
}

impl std::fmt::Display for DaysOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}  —  {}", self.label, self.hint)
    }
}

/// 시각 옵션.
struct HourOption {
    hour: u32,
    hint: &'static str,
}

impl std::fmt::Display for HourOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:02}:00  —  {}", self.hour, self.hint)
    }
}

/// 마법사 실행. ESC/Ctrl+C 로 중단 시 inquire 가 에러를 반환한다.
pub fn run() -> Result<()> {
    print_banner();
    print_current_state();

    let setup = collect()?;

    if !confirm_summary(&setup)? {
        println!("\n취소했습니다. 아무것도 변경하지 않았습니다.");
        return Ok(());
    }

    apply(setup)
}

/// 모든 프롬프트를 순서대로 묻는다.
fn collect() -> Result<Setup> {
    let root = ask_root()?;
    let days = ask_days()?;
    let schedule = Confirm::new("launchd 주기 스케줄을 켤까요?")
        .with_default(true)
        .with_help_message("끄면 수동 실행만 가능")
        .prompt()?;

    let (weekday, hour, run_now) = if schedule {
        let weekday = Select::new("실행 요일:", WEEKDAYS.to_vec())
            .with_help_message("↑↓ 로 선택, Enter 로 확정")
            .prompt()?
            .value;

        let hours = vec![
            HourOption {
                hour: 0,
                hint: "자정",
            },
            HourOption {
                hour: 1,
                hint: "새벽",
            },
            HourOption {
                hour: 2,
                hint: "새벽",
            },
            HourOption {
                hour: 3,
                hint: "새벽 (추천)",
            },
            HourOption {
                hour: 4,
                hint: "새벽",
            },
            HourOption {
                hour: 5,
                hint: "이른 아침",
            },
        ];
        let hour = Select::new("실행 시각:", hours)
            .with_help_message("새벽 시간대 추천 — 빌드가 없을 확률이 높음")
            .prompt()?
            .hour;

        let run_now = Confirm::new("마법사 종료 직후 첫 정리를 즉시 실행할까요?")
            .with_help_message("건너뛰면 다음 주기(또는 수동 실행)에 정리")
            .with_default(true)
            .prompt()?;
        (weekday, hour, run_now)
    } else {
        (0, 3, false)
    };

    Ok(Setup {
        root,
        days,
        schedule,
        weekday,
        hour,
        run_now,
    })
}

/// 루트 디렉토리 입력. 기본값은 config 또는 cwd.
fn ask_root() -> Result<PathBuf> {
    let (default_root, _) = config::resolve(None, None);
    let raw: String = Text::new("스캔 루트 디렉토리:")
        .with_default(&default_root.to_string_lossy())
        .with_help_message("이 경로 하위의 모든 Cargo 프로젝트 target/ 을 정리합니다")
        .with_validator(PathExistsValidator)
        .prompt()?;
    Ok(PathBuf::from(raw.trim()))
}

/// 보존 일수 선택.
fn ask_days() -> Result<u32> {
    let options = vec![
        DaysOption {
            label: "14일 (적극 정리)",
            value: 14,
            hint: "최근 2주 빌드만 보존 — 디스크 가장 아낌",
        },
        DaysOption {
            label: "30일 (균형, 추천)",
            value: 30,
            hint: "한 달 치 캐시 보존 — 일반적 워크로드에 적합",
        },
        DaysOption {
            label: "60일 (보수적)",
            value: 60,
            hint: "두 달 치 보존 — 드문 빌드에 안전",
        },
        DaysOption {
            label: "직접 입력",
            value: 0,
            hint: "원하는 일수를 숫자로 지정",
        },
    ];

    let choice = Select::new("최근 며칠 치 산물을 보존할까요?", options)
        .with_help_message("이 기간 이내 산물은 유지, 그 이전만 삭제")
        .prompt()?;

    if choice.value == 0 {
        let n: u32 = CustomType::<u32>::new("보존 일수:")
            .with_placeholder("예: 21")
            .with_validator(RangeValidator { min: 1, max: 365 })
            .with_error_message("1~365 사이의 정수를 입력하세요")
            .prompt()?;
        Ok(n)
    } else {
        Ok(choice.value)
    }
}

/// 요약 출력 후 최종 확인.
fn confirm_summary(s: &Setup) -> Result<bool> {
    println!("\n{}", "─".repeat(54));
    println!("  설정 요약");
    println!("{}", "─".repeat(54));
    println!("  스캔 루트 : {}", s.root.display());
    println!("  보존 일수 : {}일", s.days);
    if s.schedule {
        let wname = WEEKDAYS
            .iter()
            .find(|w| w.value == s.weekday)
            .map(|w| w.name)
            .unwrap_or("?");
        println!("  스케줄    : 매주 {} {:02}:00", wname, s.hour);
        println!(
            "  즉시 실행 : {}",
            if s.run_now {
                "예 (종료 직후 첫 정리)"
            } else {
                "아니오 (다음 주기에)"
            }
        );
    } else {
        println!("  스케줄    : 끔 (수동으로만 실행)");
    }
    println!("{}", "─".repeat(54));

    Ok(Confirm::new("이대로 적용할까요?")
        .with_default(true)
        .prompt()?)
}

/// 수집한 설정을 실제로 적용.
fn apply(s: Setup) -> Result<()> {
    let cfg = config::Config {
        root: s.root.clone(),
        days: s.days,
    };
    config::save(&cfg)?;
    println!("\n✓ config.toml 저장됨 (~/.oxicleaner/config.toml)");

    if s.schedule {
        let installed_bin = crate::cli::install_self_binary()?;
        schedule::enable(
            s.weekday,
            s.hour,
            s.days,
            &s.root.to_string_lossy(),
            &installed_bin.to_string_lossy(),
        )?;
        let wname = WEEKDAYS
            .iter()
            .find(|w| w.value == s.weekday)
            .map(|w| w.name)
            .unwrap_or("?");
        println!("✓ launchd 스케줄 활성화 (매주 {} {:02}:00)", wname, s.hour);
    } else if schedule::is_loaded() {
        schedule::disable()?;
        println!("✓ 기존 스케줄 비활성화됨");
    } else {
        println!("• 스케줄은 켜지 않음");
    }

    if s.run_now {
        println!("\n첫 정리 실행 중...");
        let report = sweep::run(&s.root, s.days, false, false)?;
        print_sweep_report(&report);
        if !report.skipped {
            history::append(&report)?;
        }
    }

    println!("\n{}", "=".repeat(54));
    println!("  🎉 설정 완료!");
    println!("{}", "=".repeat(54));
    println!("\n  다음 명령으로 확인하세요:");
    println!("    oxicleaner status           # 스케줄 + 마지막 실행");
    println!("    oxicleaner history          # 정리 이력");
    println!("    oxicleaner sweep --dry-run  # 미리보기");
    println!();
    Ok(())
}

fn print_sweep_report(report: &sweep::SweepReport) {
    if report.skipped {
        println!(
            "  ⏭  스킵: {}",
            report.skip_reason.as_deref().unwrap_or("?")
        );
        return;
    }
    for p in &report.projects {
        match &p.freed {
            Some(f) => println!("  ✓ {:>10}  {}", f, p.path),
            None => println!("    {:>10}  {}", "—", p.path),
        }
    }
    println!();
    if let Some(t) = &report.total_freed {
        println!(
            "  총 확보: {}  (disk: {} → {})",
            t, report.disk_before, report.disk_after
        );
    } else {
        println!("  정리 대상 없음  (disk: {})", report.disk_before);
    }
}

fn print_banner() {
    println!();
    println!("{}", "=".repeat(54));
    println!("  oxicleaner 설정 마법사");
    println!("{}", "=".repeat(54));
    println!("  Rust target/ 자동 정리를 설정합니다.");
    println!("  ESC 또는 Ctrl+C 로 언제든 중단할 수 있습니다.");
    println!();
}

fn print_current_state() {
    let loaded = schedule::is_loaded();
    let status = if loaded { "✅ 켜짐" } else { "❌ 꺼짐" };
    println!("  현재 스케줄: {}", status);

    if let Ok(Some(cfg)) = config::load() {
        println!(
            "  현재 config: root={}, days={}",
            cfg.root.display(),
            cfg.days
        );
    } else {
        println!("  현재 config: (없음 — 기본값 사용 중)");
    }

    let (default_root, _) = config::resolve(None, None);
    println!("  디스크     : {}", disk::stat(&default_root));
    println!();
}

// ── inquire 용 검증자들 ────────────────────────────────────────────────

/// 경로 입력 검증: 존재하는 디렉토리여야 함. (Text/String 용)
#[derive(Clone)]
struct PathExistsValidator;

impl StringValidator for PathExistsValidator {
    fn validate(
        &self,
        input: &str,
    ) -> Result<Validation, Box<dyn std::error::Error + Send + Sync>> {
        if std::path::Path::new(input.trim()).is_dir() {
            Ok(Validation::Valid)
        } else {
            Ok(Validation::Invalid(
                format!("존재하는 디렉토리 경로를 입력하세요: {input}").into(),
            ))
        }
    }
}

/// 정수 범위 검증 (CustomType<u32> 용).
#[derive(Clone)]
struct RangeValidator {
    min: u32,
    max: u32,
}

impl CustomTypeValidator<u32> for RangeValidator {
    fn validate(&self, n: &u32) -> Result<Validation, Box<dyn std::error::Error + Send + Sync>> {
        if *n >= self.min && *n <= self.max {
            Ok(Validation::Valid)
        } else {
            Ok(Validation::Invalid(
                format!(
                    "{min}~{max} 사이의 값이어야 합니다: {n}",
                    min = self.min,
                    max = self.max
                )
                .into(),
            ))
        }
    }
}
