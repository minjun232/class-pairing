//! 폴더 구조와 반 목록.

use crate::{roster, Result, StorageError};
use std::path::{Path, PathBuf};

pub const CLASSES_DIR: &str = "반";
pub const SYSTEM_DIR: &str = "system";
pub const ROSTER_FILE: &str = "명단.xlsx";
pub const SETTINGS_FILE: &str = "Settings.ini";
pub const MODEL_FILE: &str = "model.gguf";
pub const TEMPLATE_FILE: &str = "template.xlsx";

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub root: PathBuf,
}

impl AppPaths {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// 실행 파일이 있는 폴더 기준.
    pub fn from_exe() -> std::io::Result<Self> {
        let exe = std::env::current_exe()?;
        let dir = exe.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
        Ok(Self::new(dir))
    }

    pub fn classes(&self) -> PathBuf {
        self.root.join(CLASSES_DIR)
    }
    pub fn system(&self) -> PathBuf {
        self.root.join(SYSTEM_DIR)
    }
    pub fn model(&self) -> PathBuf {
        self.system().join(MODEL_FILE)
    }
    pub fn template(&self) -> PathBuf {
        self.system().join(TEMPLATE_FILE)
    }
    pub fn app_dat(&self) -> PathBuf {
        self.system().join("app.dat")
    }
    pub fn state_file(&self, class: &str) -> PathBuf {
        self.system().join("state").join(format!("{class}.dat"))
    }
    pub fn class_dir(&self, class: &str) -> PathBuf {
        self.classes().join(class)
    }
    pub fn roster(&self, class: &str) -> PathBuf {
        self.class_dir(class).join(ROSTER_FILE)
    }
    pub fn settings(&self, class: &str) -> PathBuf {
        self.class_dir(class).join(SETTINGS_FILE)
    }

    /// `반/` 아래 폴더 목록 (사람이 읽는 순서: 2-3 < 2-10).
    pub fn list_classes(&self) -> Result<Vec<String>> {
        let dir = self.classes();
        if !dir.exists() {
            std::fs::create_dir_all(&dir)?;
        }
        let mut v: Vec<String> = std::fs::read_dir(&dir)?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| !n.starts_with('.'))
            .collect();
        v.sort_by(|a, b| natural_cmp(a, b));
        Ok(v)
    }

    /// 새 반 폴더를 만든다: 명단.xlsx(템플릿 복사) + 빈 Settings.ini
    pub fn add_class(&self, name: &str) -> Result<()> {
        let name = name.trim();
        validate_class_name(name)?;
        let dir = self.class_dir(name);
        if dir.exists() {
            return Err(StorageError::Invalid("이미 있는 반 이름이에요".into()));
        }
        std::fs::create_dir_all(&dir)?;
        let template = self.template();
        if !template.exists() {
            roster::write_template(&template)?;
        }
        std::fs::copy(&template, self.roster(name))?;
        crate::settings::write_empty(&self.settings(name))?;
        Ok(())
    }
}

fn validate_class_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(StorageError::Invalid("반 이름을 적어 주세요".into()));
    }
    if name.chars().count() > 40
        || name.chars().any(|c| matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control())
        || name.ends_with('.')
        || name.starts_with('.')
    {
        return Err(StorageError::Invalid("반 이름에 쓸 수 없는 글자가 있어요".into()));
    }
    Ok(())
}

/// 숫자 부분은 수로 비교하는 정렬.
pub fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    fn chunks(s: &str) -> Vec<(bool, String)> {
        let mut v: Vec<(bool, String)> = Vec::new();
        for c in s.chars() {
            let d = c.is_ascii_digit();
            match v.last_mut() {
                Some((ld, buf)) if *ld == d => buf.push(c),
                _ => v.push((d, c.to_string())),
            }
        }
        v
    }
    let (ca, cb) = (chunks(a), chunks(b));
    for (x, y) in ca.iter().zip(cb.iter()) {
        let o = if x.0 && y.0 {
            let (nx, ny) = (x.1.trim_start_matches('0'), y.1.trim_start_matches('0'));
            nx.len().cmp(&ny.len()).then(nx.cmp(ny))
        } else {
            x.1.cmp(&y.1)
        };
        if o != std::cmp::Ordering::Equal {
            return o;
        }
    }
    ca.len().cmp(&cb.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_sort() {
        let mut v = vec!["2-10", "2-3", "1-1", "2-5"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, ["1-1", "2-3", "2-5", "2-10"]);
    }

    #[test]
    fn add_and_list_classes_with_korean_path() {
        let t = tempfile::tempdir().unwrap();
        let p = AppPaths::new(t.path().join("자리배치"));
        p.add_class("2-3").unwrap();
        p.add_class("2-10반").unwrap();
        assert!(p.add_class("2-3").is_err());
        assert!(p.add_class("a/b").is_err());
        assert_eq!(p.list_classes().unwrap(), ["2-3", "2-10반"]);
        assert!(p.roster("2-3").exists());
        assert!(p.settings("2-3").exists());
        assert!(p.template().exists());
        assert!(crate::roster::read_roster(&p.roster("2-3")).unwrap().is_empty());
    }
}
