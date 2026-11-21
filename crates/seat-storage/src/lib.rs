//! 파일 읽고 쓰기. 데이터는 모두 실행 파일이 있는 폴더 아래에 둔다(설계 6장).
//!
//! ```text
//! 자리배치/
//! ├─ 자리배치.exe
//! ├─ 반/<반 이름>/명단.xlsx, Settings.ini
//! └─ system/ model.gguf, template.xlsx, app.dat, state/<반 이름>.dat
//! ```

pub mod docx;
pub mod paths;
pub mod roster;
pub mod settings;
pub mod state;

pub use paths::AppPaths;

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("파일을 읽거나 쓸 수 없어요: {0}")]
    Io(#[from] std::io::Error),
    #[error("명단 파일을 읽을 수 없어요: {0}")]
    Roster(String),
    #[error("{0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, StorageError>;

/// 내용 해시(FNV-1a 64비트). 캐시 무효화용이며 보안 용도가 아니다.
pub fn content_hash(parts: &[&[u8]]) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for p in parts {
        for &b in p.iter() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h ^= 0xff;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

/// 파일을 임시 파일에 쓴 뒤 바꿔치기한다(쓰는 도중 USB가 빠져도 원본이 남도록).
pub fn write_atomic(path: &std::path::Path, data: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}
