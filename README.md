# oxicleaner

[![Crates.io](https://img.shields.io/badge/crates.io-coming%20soon-orange)](https://crates.io)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

> **Recursive Rust `target/` cleaner with launchd scheduling.**
>
> Wraps [cargo-sweep](https://github.com/holmgr/cargo-sweep) to clean stale build
> artifacts across **all** Cargo projects under a root, while preserving recently
> used artifacts so your next build stays fast. Installs and manages its own macOS
> `launchd` schedule, records history, and refuses to run while a build is in
> progress.

`oxicleaner` was built for the [oxios](https://github.com/a7garden/oxios) dev
machine, where dozens of worktrees and sibling Rust projects had quietly amassed
**~280 GB** of duplicated, feature-combination-specific build artifacts.

## Why

`cargo` doesn't garbage-collect `target/`. Every feature combination, toolchain,
profile, and codegen-unit split produces a fresh set of `.rlib` / `.rcgu.o`
files. Over months these accumulate until a single project can balloon to dozens
of duplicate variants of the same crate:

| crate | duplicate `.rlib` variants found |
|-------|----------------------------------:|
| oxios_gateway | 124 |
| oxios_kernel | 115 |
| oxios_ouroboros | 97 |
| … | … |

`oxicleaner` automates the cleanup so the disk never fills up again — on a
schedule, safely, with a record of what it did.

## How it works

1. **Recursive scan** — finds every `Cargo.toml` under the configured root and
   the matching `target/` for each.
2. **cargo-sweep** — for each project, deletes build artifacts **older than N
   days** (default 30). Anything you built recently is kept, so the next
   incremental build is still fast.
3. **Safety guard** — if any `cargo`/`rustc` process is running under the root,
   the run is skipped (rescheduled to next cycle) instead of risking corruption.
4. **launchd schedule** — `oxicleaner enable` writes and loads its own plist.
   The scheduled binary lives in `~/.oxicleaner/` (outside any `target/`), so it
   can never delete itself.
5. **History** — every run (including skips) is appended to
   `~/.oxicleaner/history.jsonl` and queryable via `oxicleaner history`.

## Install

```bash
# 1. runtime dependency
cargo install cargo-sweep

# 2. build oxicleaner
git clone https://github.com/a7garden/oxicleaner
cd oxicleaner
cargo build --release

# 3. (optional) put it on PATH
cp target/release/oxicleaner ~/.cargo/bin/
```

> **Platform:** macOS only (launchd scheduling). The `sweep` command itself
> works anywhere cargo-sweep does.

## Usage

```bash
# 🆕 Interactive setup wizard (recommended first run)
oxicleaner setup
#   → walks you through root, retention, schedule, weekday/time
#   → writes config.toml, enables launchd, optionally runs first sweep

# Clean now, keeping artifacts from the last 30 days
oxicleaner sweep
oxicleaner sweep --days 60
oxicleaner sweep --dry-run          # preview only — deletes nothing
oxicleaner sweep --root ~/projects  # override scan root

# Enable the weekly schedule (non-interactive)
oxicleaner enable
oxicleaner enable --weekday 5 --hour 4 --days 45   # Fridays 04:00, keep 45d

# Inspect
oxicleaner status       # is the schedule loaded? what did the last run do?
oxicleaner history      # recent runs (timestamp, mode, freed, disk delta)
oxicleaner history -n 20

# Disable the schedule
oxicleaner disable

# Run with no subcommand → sweep (handy for the scheduled invocation)
oxicleaner
```

### Flags

| Flag | Scope | Description |
|------|-------|-------------|
| `-r, --root <PATH>` | global | Scan root (defaults to `config.toml`) |
| `--days <N>` | sweep, enable | Keep artifacts newer than N days |
| `--dry-run` | sweep | Preview without deleting |
| `--force` | sweep | Run even if a build is in progress |
| `--weekday <0-6>` | enable | 0=Sun … 6=Sat (default 0) |
| `--hour <0-23>` | enable | Hour of day (default 3) |
| `-n, --limit <N>` | history | Number of records to show |

## File locations

| Path | Purpose |
|------|---------|
| `~/.oxicleaner/config.toml` | Root + retention setting (written by `enable`) |
| `~/.oxicleaner/oxicleaner` | Scheduled binary copy (kept out of any `target/`) |
| `~/.oxicleaner/history.jsonl` | One JSON line per run |
| `~/Library/LaunchAgents/local.oxicleaner.plist` | launchd schedule |
| `~/Library/Logs/oxicleaner/` | launchd stdout/stderr + scheduled-run output |

## Example output

```
$ oxicleaner sweep --dry-run --days 30
oxicleaner: root=/Volumes/MERCURY/PROJECTS, keep=30d, mode=dry-run

   ✓   10.10 GiB  /Volumes/MERCURY/PROJECTS/cardion/src-tauri/target
   ✓   17.17 GiB  /Volumes/MERCURY/PROJECTS/clawgarden/target
      —          /Volumes/MERCURY/PROJECTS/oxi/target
   ✓    4.04 GiB  /Volumes/MERCURY/PROJECTS/session-a-web-platform/target
   ✓   10.66 GiB  /Volumes/MERCURY/PROJECTS/session-b-cdp-perf/target

총 확보: 42.47 GiB   (disk: 35% 사용, 607Gi 여유 → 35% 사용, 607Gi 여유)
```

```
$ oxicleaner history
시각                모드             확보    disk 변화
----------------------------------------------------------------------
2026-06-15 16:11   live      42.47 GiB  35% 사용, 607Gi 여유 → 32% 사용, 634Gi 여유
2026-06-14 03:00   live       0.00 B    32% 사용, 634Gi 여유 → 32% 사용, 634Gi 여유
2026-06-07 03:00   SKIP       프로세스 빌드 중
```

## Building

```bash
cargo build --release
cargo test
cargo clippy -D warnings
cargo fmt --check
```

## Alternatives

- **`cargo clean`** — deletes the entire `target/`. Reliable, but forces a full
  rebuild every time. Fine for occasional use, painful as a schedule.
- **`cargo-sweep`** directly — the underlying engine. `oxicleaner` adds the
  multi-project recursive scan, launchd scheduling, build-detection safety guard,
  and history.

## License

MIT. See [LICENSE](LICENSE).
