//! 디스크 사용량 조회 (df 래핑).

use std::path::Path;
use std::process::Command;

/// 주어진 경로가 속한 파일시스템의 사용 현황을 "NN% 사용, XXg 여유" 형태로 반환.
/// macOS `df -h` 출력 기준:
///   Filesystem   Size   Used  Avail  Capacity ...
///   /dev/disk7s1 931Gi  324Gi  607Gi   35%   ...
pub fn stat(path: &Path) -> String {
    let out = Command::new("df")
        .args(["-h", &path.to_string_lossy()])
        .output();
    let Ok(out) = out else {
        return String::from("unknown");
    };
    if !out.status.success() {
        return String::from("unknown");
    }
    let s = String::from_utf8_lossy(&out.stdout);
    let Some(line) = s.lines().nth(1) else {
        return String::from("unknown");
    };
    let cols: Vec<&str> = line.split_whitespace().collect();
    // cols: [Filesystem, Size, Used, Avail, Capacity, ...]
    if cols.len() >= 5 {
        return format!("{} 사용, {} 여유", cols[4], cols[3]);
    }
    line.to_string()
}
