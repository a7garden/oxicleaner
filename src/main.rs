//! oxicleaner — Recursive Rust `target/` cleaner with launchd scheduling.
//!
//! cargo-sweep 을 래핑하여 여러 Cargo 프로젝트의 target/ 에서 오래된 빌드
//! 산물만 정리하고, launchd 로 주기 자동 실행을 관리하며, 정리 이력을 기록한다.

mod cli;
mod config;
mod disk;
mod history;
mod safety;
mod schedule;
mod sweep;

fn main() {
    match <cli::Cli as clap::Parser>::parse().run() {
        Ok(()) => {}
        Err(e) => {
            eprintln!("oxicleaner: {e:#}");
            std::process::exit(1);
        }
    }
}
