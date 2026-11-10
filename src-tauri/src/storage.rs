//! JSON 파일 저장 (spec 8장 저장 규칙).
//!
//! - 임시 파일에 쓴 뒤 이름을 바꿔 덮어쓴다. 쓰다가 꺼져도 기존 파일이 깨지지 않는다.
//! - 읽지 못한 파일은 `.bak`으로 이름을 바꿔 남기고 빈 상태로 시작한다. 조용히 지우지 않는다.

use std::{fs, io::Write, path::Path};

use serde::{de::DeserializeOwned, Serialize};

/// 파일을 읽는다. 없으면 `Ok(None)`. 읽거나 해석하지 못하면 `.bak`으로 옮기고 `Err(설명)`.
pub fn load<T: DeserializeOwned>(path: &Path) -> Result<Option<T>, String> {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(keep_broken(path, &e.to_string())),
    };
    serde_json::from_str(&text).map(Some).map_err(|e| keep_broken(path, &e.to_string()))
}

fn keep_broken(path: &Path, why: &str) -> String {
    // 이전에 남긴 .bak이 있으면 덮어쓰지 않고 시각을 붙인 이름을 쓴다
    let mut bak = path.with_extension("json.bak");
    if bak.exists() {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        bak = path.with_extension(format!("json.{t}.bak"));
    }
    let moved = fs::rename(path, &bak).is_ok();
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("파일");
    if moved {
        format!("{name}을(를) 읽지 못해 빈 상태로 시작했습니다({why}). 원래 파일은 {}에 남겨 두었습니다.", bak.display())
    } else {
        format!("{name}을(를) 읽지 못해 빈 상태로 시작했습니다({why}). 원래 파일은 그 자리에 그대로 있습니다.")
    }
}

/// 임시 파일에 쓰고 이름을 바꿔 덮어쓴다.
pub fn save<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let fail = |e: &dyn std::fmt::Display| format!("{}을(를) 저장하지 못했습니다({e}). 디스크 공간과 폴더 권한을 확인해 주세요.", path.display());
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| fail(&e))?;
    }
    let json = serde_json::to_vec_pretty(value).map_err(|e| fail(&e))?;
    let tmp = path.with_extension("json.tmp");
    {
        let mut f = fs::File::create(&tmp).map_err(|e| fail(&e))?;
        f.write_all(&json).map_err(|e| fail(&e))?;
        f.sync_all().map_err(|e| fail(&e))?;
    }
    fs::rename(&tmp, path).map_err(|e| fail(&e))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("tbs-storage-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn save_then_load() {
        let p = dir("roundtrip").join("a.json");
        save(&p, &vec![1, 2, 3]).unwrap();
        assert_eq!(load::<Vec<i32>>(&p).unwrap(), Some(vec![1, 2, 3]));
        assert!(!p.with_extension("json.tmp").exists());
    }

    #[test]
    fn missing_file_is_none() {
        let p = dir("missing").join("none.json");
        assert_eq!(load::<Vec<i32>>(&p).unwrap(), None);
    }

    #[test]
    fn broken_file_is_kept_as_bak() {
        let p = dir("broken").join("b.json");
        fs::write(&p, "{ not json").unwrap();
        assert!(load::<Vec<i32>>(&p).is_err());
        assert!(!p.exists());
        assert_eq!(fs::read_to_string(p.with_extension("json.bak")).unwrap(), "{ not json");
        // 두 번째로 깨져도 첫 .bak을 덮어쓰지 않는다
        fs::write(&p, "second").unwrap();
        assert!(load::<Vec<i32>>(&p).is_err());
        assert_eq!(fs::read_to_string(p.with_extension("json.bak")).unwrap(), "{ not json");
        let baks = fs::read_dir(p.parent().unwrap()).unwrap().filter(|e| e.as_ref().unwrap().file_name().to_string_lossy().ends_with(".bak")).count();
        assert_eq!(baks, 2);
    }
}
