//! Settings.ini — 교사 조건. 한 줄에 하나씩 자연어로 쓴다.
//!
//! - `;` 또는 `#`으로 시작하는 줄: 주석
//! - `[General]` 같은 섹션 머리글: 무시
//! - `앞쪽 = 3`, `뒤쪽 = 2`, `창가 = 1`, `복도 = 1`: 애매한 표현의 기준값 덮어쓰기
//! - 나머지 줄: 조건 하나
//!
//! 메모장 버전에 따라 UTF-8(BOM 유무), UTF-16, CP949(ANSI)로 저장되므로 모두 읽는다.

use crate::Result;
use seat_core::ZoneSettings;
use std::path::Path;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TeacherSettings {
    pub conditions: Vec<String>,
    pub zones: ZoneSettings,
}

/// 바이트를 글자로 바꾼다.
pub fn decode(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return encoding_rs::UTF_16LE.decode(bytes).0.into_owned();
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return encoding_rs::UTF_16BE.decode(bytes).0.into_owned();
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        // encoding_rs의 EUC-KR은 Windows 코드 페이지 949(CP949)와 같다.
        Err(_) => encoding_rs::EUC_KR.decode(bytes).0.into_owned(),
    }
}

pub fn parse(text: &str) -> TeacherSettings {
    let mut out = TeacherSettings::default();
    for line in text.lines() {
        let line = line.trim().trim_start_matches('\u{feff}').trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let k: String = k.chars().filter(|c| !c.is_whitespace()).collect();
            let v = v.trim().trim_end_matches(|c| c == '줄' || c == '열').trim();
            if let Ok(n) = v.parse::<u32>() {
                let n = Some(n.max(1));
                match k.as_str() {
                    "앞쪽" | "앞" => {
                        out.zones.front_rows = n;
                        continue;
                    }
                    "뒤쪽" | "뒤" => {
                        out.zones.back_rows = n;
                        continue;
                    }
                    "창가" | "창문쪽" => {
                        out.zones.window_cols = n;
                        continue;
                    }
                    "복도" | "복도쪽" => {
                        out.zones.hallway_cols = n;
                        continue;
                    }
                    _ => {}
                }
            }
        }
        out.conditions.push(line.to_string());
    }
    out
}

/// 파일을 읽는다. 없으면 빈 설정. (원본 바이트도 돌려준다: 캐시 해시용)
pub fn read(path: &Path) -> Result<(TeacherSettings, Vec<u8>)> {
    match std::fs::read(path) {
        Ok(b) => Ok((parse(&decode(&b)), b)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok((TeacherSettings::default(), vec![])),
        Err(e) => Err(e.into()),
    }
}

/// 새 반의 빈 Settings.ini (메모장에서 바로 열리도록 UTF-8 BOM + CRLF)
pub fn write_empty(path: &Path) -> Result<()> {
    crate::write_atomic(path, "\u{feff}[General]\r\n".as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "[General]\r\n; 메모\r\n철수랑 영희 멀리\r\n\r\n앞쪽 = 3\r\n민수 시력 안 좋음\r\n창가=2열\r\n";

    #[test]
    fn parse_lines_and_overrides() {
        let s = parse(SAMPLE);
        assert_eq!(s.conditions, ["철수랑 영희 멀리", "민수 시력 안 좋음"]);
        assert_eq!(s.zones.front_rows, Some(3));
        assert_eq!(s.zones.window_cols, Some(2));
        assert_eq!(s.zones.back_rows, None);
    }

    #[test]
    fn encodings() {
        let utf8 = SAMPLE.as_bytes().to_vec();
        let mut bom = vec![0xEF, 0xBB, 0xBF];
        bom.extend_from_slice(&utf8);
        let cp949 = encoding_rs::EUC_KR.encode(SAMPLE).0.into_owned();
        let mut utf16 = vec![0xFF, 0xFE];
        for u in SAMPLE.encode_utf16() {
            utf16.extend_from_slice(&u.to_le_bytes());
        }
        for b in [utf8, bom, cp949, utf16] {
            assert_eq!(parse(&decode(&b)).conditions.len(), 2);
        }
    }
}
