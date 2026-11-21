//! system/state/<반>.dat, system/app.dat (JSON).
//!
//! 학생 요청은 여기에 저장하지 않는다(메모리에만 둔다).

use crate::{write_atomic, Result};
use seat_core::{ClassroomConfig, Rule};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AppState {
    pub last_class: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlacedName {
    pub name: String,
    pub row: u32,
    pub col: u32,
}

/// 확정한 좌석표 (직전 좌석표 1회분)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfirmedSeating {
    pub date: String,
    pub classroom: ClassroomConfig,
    pub seats: Vec<PlacedName>,
    /// 짝(거리 0) 이름 쌍 — 다음 배치에서 "직전 짝 피하기"에 쓴다
    pub pairs: Vec<(String, String)>,
    /// 확정할 때 저장한 Word 파일 이름 (다시 저장할 때 같은 파일에 쓴다)
    #[serde(default)]
    pub file: Option<String>,
}

/// 교사 조건 해석 캐시. `key`(명단·모델·해석기 버전)가 바뀌면 통째로 버린다.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct InterpretCache {
    pub key: String,
    pub entries: BTreeMap<String, Vec<Rule>>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ClassState {
    #[serde(default)]
    pub classroom: ClassroomConfig,
    #[serde(default)]
    pub previous: Option<ConfirmedSeating>,
    #[serde(default)]
    pub teacher_cache: InterpretCache,
}

/// 읽기. 파일이 없거나 깨졌으면 기본값(새로 시작).
pub fn load<T: for<'de> Deserialize<'de> + Default>(path: &Path) -> T {
    std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub fn save<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let data = serde_json::to_vec(value).map_err(|e| crate::StorageError::Invalid(e.to_string()))?;
    write_atomic(path, &data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_or_broken_is_default() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("state").join("2-3.dat");
        assert_eq!(load::<ClassState>(&p), ClassState::default());
        let mut s = ClassState::default();
        s.classroom.rows = 7;
        s.teacher_cache.entries.insert("a".into(), vec![]);
        save(&p, &s).unwrap();
        assert_eq!(load::<ClassState>(&p), s);
        std::fs::write(&p, b"garbage").unwrap();
        assert_eq!(load::<ClassState>(&p), ClassState::default());
    }
}
