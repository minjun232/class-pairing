//! 명단.xlsx — 남/여 두 열로 나눠 이름을 적는다.

use crate::{Result, StorageError};
use calamine::{open_workbook_auto, Data, Reader};
use seat_core::{Gender, Student};
use std::path::Path;

fn header_gender(s: &str) -> Option<Gender> {
    let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    match s.as_str() {
        "남" | "남자" | "남학생" | "남성" => Some(Gender::Male),
        "여" | "여자" | "여학생" | "여성" => Some(Gender::Female),
        _ => None,
    }
}

fn cell_text(d: &Data) -> String {
    match d {
        Data::String(s) => s.trim().to_string(),
        Data::Int(i) => i.to_string(),
        Data::Float(f) => f.to_string(),
        _ => String::new(),
    }
}

/// 명단을 읽는다. 첫 시트에서 "남"/"여" 머리글이 있는 줄을 찾고, 그 아래를 이름으로 읽는다.
/// 머리글이 없으면 A열 = 남, B열 = 여로 본다.
pub fn read_roster(path: &Path) -> Result<Vec<Student>> {
    let mut wb = open_workbook_auto(path).map_err(|e| StorageError::Roster(e.to_string()))?;
    let range = match wb.worksheet_range_at(0) {
        Some(r) => r.map_err(|e| StorageError::Roster(e.to_string()))?,
        None => return Ok(vec![]),
    };
    let rows: Vec<Vec<String>> = range.rows().map(|r| r.iter().map(cell_text).collect()).collect();

    let mut header: Option<(usize, Vec<(usize, Gender)>)> = None;
    for (ri, row) in rows.iter().enumerate().take(10) {
        let cols: Vec<(usize, Gender)> =
            row.iter().enumerate().filter_map(|(ci, s)| header_gender(s).map(|g| (ci, g))).collect();
        if !cols.is_empty() {
            header = Some((ri, cols));
            break;
        }
    }
    let (start, cols) = header.unwrap_or((usize::MAX, vec![(0, Gender::Male), (1, Gender::Female)]));
    let first = if start == usize::MAX { 0 } else { start + 1 };

    let mut out: Vec<Student> = Vec::new();
    // 남 열 전체, 그다음 여 열 전체 순서로 읽는다(순서는 배치 결과에 영향 없음).
    for &(ci, g) in &cols {
        for row in rows.iter().skip(first) {
            if let Some(name) = row.get(ci) {
                let name = name.trim();
                if !name.is_empty() {
                    out.push(Student { name: name.to_string(), gender: g });
                }
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    for s in &out {
        if !seen.insert(s.name.as_str()) {
            return Err(StorageError::Invalid(format!(
                "명단에 '{}' 이름이 두 번 있어요. 반에서 쓰는 표기(예: {} A, {} B)로 구분해 주세요.",
                s.name, s.name, s.name
            )));
        }
    }
    Ok(out)
}

/// 명단 템플릿을 만든다.
pub fn write_template(path: &Path) -> Result<()> {
    use rust_xlsxwriter::{Format, FormatAlign, Workbook};
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet();
    let err = |e: rust_xlsxwriter::XlsxError| StorageError::Roster(e.to_string());
    ws.set_name("명단").map_err(err)?;
    let head = Format::new().set_bold().set_align(FormatAlign::Center).set_background_color("#E8EEF7");
    ws.write_string_with_format(0, 0, "남", &head).map_err(err)?;
    ws.write_string_with_format(0, 1, "여", &head).map_err(err)?;
    ws.set_column_width(0, 16).map_err(err)?;
    ws.set_column_width(1, 16).map_err(err)?;
    ws.set_freeze_panes(1, 0).map_err(err)?;
    wb.save(path).map_err(err)?;
    Ok(())
}

/// 프로그램에서 입력한 명단을 저장한다 (머리글 남/여, 한 칸에 한 명).
pub fn save_roster(path: &Path, males: &[String], females: &[String]) -> Result<()> {
    use rust_xlsxwriter::{Format, FormatAlign, Workbook};
    let err = |e: rust_xlsxwriter::XlsxError| StorageError::Roster(e.to_string());
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet();
    ws.set_name("명단").map_err(err)?;
    let head = Format::new().set_bold().set_align(FormatAlign::Center).set_background_color("#E8EEF7");
    ws.write_string_with_format(0, 0, "남", &head).map_err(err)?;
    ws.write_string_with_format(0, 1, "여", &head).map_err(err)?;
    ws.set_column_width(0, 16).map_err(err)?;
    ws.set_column_width(1, 16).map_err(err)?;
    ws.set_freeze_panes(1, 0).map_err(err)?;
    for (i, n) in males.iter().enumerate() {
        ws.write_string(i as u32 + 1, 0, n).map_err(err)?;
    }
    for (i, n) in females.iter().enumerate() {
        ws.write_string(i as u32 + 1, 1, n).map_err(err)?;
    }
    let buf = wb.save_to_buffer().map_err(err)?;
    crate::write_atomic(path, &buf)
}

/// 테스트·예시용: 이름 목록으로 명단 파일을 만든다.
pub fn write_roster(path: &Path, males: &[&str], females: &[&str]) -> Result<()> {
    use rust_xlsxwriter::Workbook;
    let err = |e: rust_xlsxwriter::XlsxError| StorageError::Roster(e.to_string());
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet();
    ws.write_string(0, 0, "남").map_err(err)?;
    ws.write_string(0, 1, "여").map_err(err)?;
    for (i, n) in males.iter().enumerate() {
        ws.write_string(i as u32 + 1, 0, *n).map_err(err)?;
    }
    for (i, n) in females.iter().enumerate() {
        ws.write_string(i as u32 + 1, 1, *n).map_err(err)?;
    }
    wb.save(path).map_err(err)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("명단.xlsx");
        write_roster(&p, &["김민수 A", "김민수 B", "박지훈"], &["이서연", "박하윤"]).unwrap();
        let r = read_roster(&p).unwrap();
        assert_eq!(r.len(), 5);
        assert_eq!(r.iter().filter(|s| s.gender == Gender::Male).count(), 3);
        assert_eq!(r[1].name, "김민수 B");
    }

    #[test]
    fn save_then_read() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("명단.xlsx");
        save_roster(&p, &["김철수".into()], &["이영희".into(), "박하윤".into()]).unwrap();
        let r = read_roster(&p).unwrap();
        assert_eq!(r.len(), 3);
        assert_eq!(r[2].name, "박하윤");
        assert_eq!(r[2].gender, Gender::Female);
    }

    #[test]
    fn duplicate_rejected() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("명단.xlsx");
        write_roster(&p, &["김민수", "김민수"], &[]).unwrap();
        assert!(read_roster(&p).is_err());
    }
}
