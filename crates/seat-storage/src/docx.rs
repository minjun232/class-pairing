//! Word(.docx) 좌석표. LLM을 쓰지 않고 표를 직접 만든다.
//!
//! **교사 시점**으로 출력한다: 학생 시점을 180도 돌려 맨 뒷줄이 위, 창문 쪽이 오른쪽,
//! 칠판·교탁이 아래에 온다. 한글(hwp)에서도 열리도록 기본 요소(문단, 표)만 쓴다.

use crate::Result;
use docx_rs::*;
use seat_core::{CellKind, Layout};
use std::path::Path;

const FONT: &str = "맑은 고딕";

fn run(text: &str, size_pt: usize, bold: bool) -> Run {
    let r = Run::new()
        .add_text(text)
        .size(size_pt * 2)
        .fonts(RunFonts::new().ascii(FONT).hi_ansi(FONT).east_asia(FONT).cs(FONT));
    if bold {
        r.bold()
    } else {
        r
    }
}

fn centered(r: Run) -> Paragraph {
    Paragraph::new().add_run(r).align(AlignmentType::Center)
}

fn seat_borders() -> TableCellBorders {
    let b = |p| TableCellBorder::new(p).border_type(BorderType::Single).size(8).color("444444");
    TableCellBorders::with_empty()
        .set(b(TableCellBorderPosition::Top))
        .set(b(TableCellBorderPosition::Bottom))
        .set(b(TableCellBorderPosition::Left))
        .set(b(TableCellBorderPosition::Right))
}

/// `names[자리 번호]` = 그 자리에 앉은 학생 이름 (빈자리는 None)
pub fn write_seating(path: &Path, class_name: &str, date: &str, layout: &Layout, names: &[Option<String>]) -> Result<()> {
    // A4 가로, 여백 1.5cm
    let (page_w, page_h, margin) = (16838u32, 11906u32, 850i32);
    let usable_w = page_w as usize - 2 * margin as usize;

    // 교사 시점 열 순서: 오른쪽(학생 시점) 열이 왼쪽에 온다.
    let cols: Vec<u32> = (1..=layout.cols).rev().collect();
    let rows: Vec<u32> = (1..=layout.rows).rev().collect();
    // 간격 열/줄: 학생 시점에서 c 뒤 간격 → 교사 시점에서는 c 앞에 간격
    let gap_w = 300usize;
    let n_gaps = layout.col_gap_after.iter().filter(|&&g| g).count();
    let cell_w = ((usable_w - n_gaps * gap_w) / layout.cols.max(1) as usize).min(2600);
    let name_pt = if cell_w >= 1800 { 14 } else if cell_w >= 1300 { 12 } else { 10 };

    let mut grid = Vec::new();
    // 교사 시점 i번째 열(학생 시점 c) 앞에 간격이 오는 조건: 학생 시점 c 뒤에 간격
    let gap_before = |c: u32| c < layout.cols && layout.col_gap_after[c as usize - 1];
    for &c in &cols {
        if gap_before(c) {
            grid.push(gap_w);
        }
        grid.push(cell_w);
    }

    let mut table_rows = Vec::new();
    for &r in &rows {
        let mut cells = Vec::new();
        for &c in &cols {
            if gap_before(c) {
                cells.push(TableCell::new().width(gap_w, WidthType::Dxa).set_borders(TableCellBorders::with_empty()));
            }
            let kind = layout.cell(r, c);
            let mut cell = TableCell::new().width(cell_w, WidthType::Dxa).vertical_align(VAlignType::Center);
            match kind {
                CellKind::Aisle => {
                    cell = cell.set_borders(TableCellBorders::with_empty()).add_paragraph(Paragraph::new());
                }
                _ => {
                    let name = layout
                        .seat_index(r, c)
                        .and_then(|i| names.get(i).cloned().flatten())
                        .unwrap_or_default();
                    cell = cell.set_borders(seat_borders()).add_paragraph(centered(run(&name, name_pt, true)));
                }
            }
            cells.push(cell);
        }
        table_rows.push(TableRow::new(cells).row_height(700.0).height_rule(HeightRule::AtLeast));
        // 모둠 사이 간격 줄 (학생 시점 r-1 뒤 간격 → 교사 시점 r 아래)
        if r > 1 && layout.row_gap_after[r as usize - 2] {
            let n = grid.len();
            let spacer: Vec<TableCell> = grid
                .iter()
                .map(|&w| TableCell::new().width(w, WidthType::Dxa).set_borders(TableCellBorders::with_empty()))
                .collect();
            debug_assert_eq!(spacer.len(), n);
            table_rows.push(TableRow::new(spacer).row_height(250.0).height_rule(HeightRule::Exact));
        }
    }

    let table = Table::without_borders(table_rows).set_grid(grid).align(TableAlignmentType::Center);

    let doc = Docx::new()
        .page_size(page_w, page_h)
        .page_orient(PageOrientationType::Landscape)
        .page_margin(PageMargin::new().top(margin).bottom(margin).left(margin).right(margin))
        .default_fonts(RunFonts::new().ascii(FONT).hi_ansi(FONT).east_asia(FONT).cs(FONT))
        .add_paragraph(centered(run(&format!("{class_name} 자리 배치"), 20, true)))
        .add_paragraph(centered(run(date, 11, false)))
        .add_paragraph(Paragraph::new())
        .add_table(table)
        .add_paragraph(Paragraph::new())
        .add_paragraph(centered(run("교탁 · 칠판", 12, true)));

    let mut buf = std::io::Cursor::new(Vec::new());
    doc.build().pack(&mut buf).map_err(|e| crate::StorageError::Invalid(e.to_string()))?;
    crate::write_atomic(path, &buf.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use seat_core::{ClassroomConfig, Preset};

    #[test]
    fn writes_docx() {
        let t = tempfile::tempdir().unwrap();
        for preset in [Preset::Pair, Preset::Single, Preset::Group] {
            let mut cfg = ClassroomConfig { preset, rows: 3, cols: 4, ..Default::default() };
            cfg.set_cell(1, 1, seat_core::CellKind::Aisle);
            let l = cfg.build();
            let names: Vec<Option<String>> = (0..l.seats.len()).map(|i| (i % 5 != 0).then(|| format!("학생{i}"))).collect();
            let p = t.path().join(format!("{preset:?}.docx"));
            write_seating(&p, "2-3", "2026. 11. 20.", &l, &names).unwrap();
            let bytes = std::fs::read(&p).unwrap();
            assert!(bytes.starts_with(b"PK"));
        }
    }
}
