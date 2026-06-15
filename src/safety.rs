//! 빌드 중 감지. cargo/rustc 가 지정 루트 하위에서 돌고 있으면 정리를 미룬다.

use anyhow::Result;
use std::path::Path;
use std::process::Command;

/// `root` 하위 경로에서 실행 중인 cargo/rustc 프로세스를 감지한다.
/// 감지된 프로세스의 전체 명령줄 목록을 반환 (빈 벡터 = 빌드 없음).
pub fn detect_active_builds(root: &Path) -> Result<Vec<String>> {
    // pgrep -fl 은 정규식을 받는다. cargo|rustc 로 둘 다 잡는다.
    let out = Command::new("pgrep").args(["-fl", "cargo|rustc"]).output();

    let mut active = Vec::new();
    let stdout = match out {
        Ok(o) => String::from_utf8_lossy(&o.stdout).into_owned(),
        Err(_) => return Ok(active), // pgrep 없으면 감지 불가 → 통과
    };

    let root_str = root.to_string_lossy();
    for line in stdout.lines() {
        // pgrep 자신은 "pgrep -fl cargo|rustc" 이라 'cargo' 텍스트를 포함하지만,
        // 우리 oxicleaner가 트리거한 cargo-sweep 하위는 제외하지 않는다(아래 별도 처리).
        if line.contains(root_str.as_ref()) {
            active.push(line.to_string());
        }
    }
    Ok(active)
}
