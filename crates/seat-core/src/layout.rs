//! 교실 모양(프리셋, 줄·열, 칸 편집)과 거기서 나오는 자리 목록, 거리 계산.
//!
//! 좌표는 모두 **학생 시점**이다. 1줄이 칠판 쪽(앞), 1열이 창문 쪽(왼쪽)이다.

use serde::{Deserialize, Serialize};

/// 배치 프리셋.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Preset {
    /// 짝: 책상마다 2명. 열 = 자리(사람) 열 수, 1-2열, 3-4열 … 이 한 책상.
    #[default]
    Pair,
    /// 단독: 한 명씩.
    Single,
    /// 모둠: 줄·열 = 모둠의 줄·열 수, 모둠 크기는 `group_size`.
    Group,
}

/// 성별 버튼.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum GenderMode {
    /// 무관
    #[default]
    Any,
    /// 짝: 남녀 짝 / 모둠: 남녀 고르게 섞기 / 단독: 의미 없음
    Mixed,
}

/// 칸 종류. 교사가 칸을 눌러 바꾼다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CellKind {
    /// 학생이 앉는 자리
    #[default]
    Seat,
    /// 책상은 있지만 비워 두는 자리
    Blank,
    /// 책상이 없는 통로
    Aisle,
}

/// 교사가 ② 교실 설정에서 정하는 값 전체. 반별로 저장된다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassroomConfig {
    pub preset: Preset,
    pub rows: u32,
    pub cols: u32,
    pub group_size: u32,
    pub gender: GenderMode,
    /// 자리가 아닌 칸(빈자리·통로) 목록. 좌표는 논리 격자 기준 (줄, 열), 1부터.
    #[serde(default)]
    pub edits: Vec<CellEdit>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CellEdit {
    pub row: u32,
    pub col: u32,
    pub kind: CellKind,
}

impl Default for ClassroomConfig {
    fn default() -> Self {
        Self {
            preset: Preset::Pair,
            rows: 5,
            cols: 6,
            group_size: 4,
            gender: GenderMode::Any,
            edits: Vec::new(),
        }
    }
}

pub const MIN_GROUP_SIZE: u32 = 3;
pub const MAX_DIM: u32 = 40;

impl ClassroomConfig {
    /// 범위를 벗어난 값을 안전한 값으로 고친다.
    pub fn normalized(&self) -> Self {
        let mut c = self.clone();
        c.rows = c.rows.clamp(1, MAX_DIM);
        c.cols = c.cols.clamp(1, MAX_DIM);
        c.group_size = c.group_size.clamp(MIN_GROUP_SIZE, 200);
        c
    }

    /// 칸 하나의 종류를 바꾼다(Seat이면 편집 목록에서 지운다).
    pub fn set_cell(&mut self, row: u32, col: u32, kind: CellKind) {
        self.edits.retain(|e| !(e.row == row && e.col == col));
        if kind != CellKind::Seat {
            self.edits.push(CellEdit { row, col, kind });
        }
    }

    pub fn build(&self) -> Layout {
        Layout::from_config(self)
    }
}

/// 자리 하나.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Seat {
    /// 논리 격자 줄 (1 = 앞)
    pub row: u32,
    /// 논리 격자 열 (1 = 창문 쪽)
    pub col: u32,
    /// 책상(짝) 또는 모둠 번호. 단독이면 자리마다 다르다.
    pub unit: u32,
}

/// 설정에서 계산한 실제 교실. 화면 그리기와 배치 알고리즘이 함께 쓴다.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Layout {
    pub preset: Preset,
    pub gender: GenderMode,
    /// 논리 격자 크기
    pub rows: u32,
    pub cols: u32,
    /// 칸 종류, `cells[(row-1) * cols + (col-1)]`
    pub cells: Vec<CellKind>,
    /// 앉을 수 있는 자리 (cells가 Seat인 칸), 앞줄부터 · 왼쪽부터 순서
    pub seats: Vec<Seat>,
    /// 화면에서 이 열 뒤에 간격을 둘지 (책상·모둠 사이)
    pub col_gap_after: Vec<bool>,
    /// 화면에서 이 줄 뒤에 간격을 둘지 (모둠 사이)
    pub row_gap_after: Vec<bool>,
    /// 모둠일 때: 모둠 하나의 높이(줄)와 모둠 격자 열 수
    group_h: u32,
    group_cols: u32,
}

impl Layout {
    pub fn from_config(cfg: &ClassroomConfig) -> Self {
        let cfg = cfg.normalized();
        let (rows, cols, group_h, group_cols) = match cfg.preset {
            Preset::Pair | Preset::Single => (cfg.rows, cfg.cols, 1, 1),
            Preset::Group => {
                let h = cfg.group_size.div_ceil(2);
                (cfg.rows * h, cfg.cols * 2, h, cfg.cols)
            }
        };
        let mut cells = vec![CellKind::Seat; (rows * cols) as usize];
        // 모둠 인원이 홀수면 모둠마다 마지막 칸 하나는 통로
        if cfg.preset == Preset::Group && cfg.group_size % 2 == 1 {
            for gr in 0..cfg.rows {
                for gc in 0..cfg.cols {
                    let r = gr * group_h + group_h; // 모둠 마지막 줄
                    let c = gc * 2 + 2; // 모둠 오른쪽 열
                    cells[((r - 1) * cols + (c - 1)) as usize] = CellKind::Aisle;
                }
            }
        }
        for e in &cfg.edits {
            if e.row >= 1 && e.row <= rows && e.col >= 1 && e.col <= cols {
                cells[((e.row - 1) * cols + (e.col - 1)) as usize] = e.kind;
            }
        }
        let mut seats = Vec::new();
        for r in 1..=rows {
            for c in 1..=cols {
                if cells[((r - 1) * cols + (c - 1)) as usize] == CellKind::Seat {
                    let unit = match cfg.preset {
                        Preset::Pair => (r - 1) * cols.div_ceil(2) + (c - 1) / 2,
                        Preset::Single => (r - 1) * cols + (c - 1),
                        Preset::Group => ((r - 1) / group_h) * group_cols + (c - 1) / 2,
                    };
                    seats.push(Seat { row: r, col: c, unit });
                }
            }
        }
        let col_gap_after = (1..=cols)
            .map(|c| match cfg.preset {
                Preset::Pair | Preset::Group => c % 2 == 0 && c < cols,
                Preset::Single => c < cols,
            })
            .collect();
        let row_gap_after = (1..=rows)
            .map(|r| match cfg.preset {
                Preset::Group => r % group_h == 0 && r < rows,
                _ => false,
            })
            .collect();
        Layout {
            preset: cfg.preset,
            gender: cfg.gender,
            rows,
            cols,
            cells,
            seats,
            col_gap_after,
            row_gap_after,
            group_h,
            group_cols,
        }
    }

    pub fn cell(&self, row: u32, col: u32) -> CellKind {
        self.cells[((row - 1) * self.cols + (col - 1)) as usize]
    }

    pub fn seat_index(&self, row: u32, col: u32) -> Option<usize> {
        self.seats.iter().position(|s| s.row == row && s.col == col)
    }

    /// 두 자리 사이 거리.
    ///
    /// - 0 = 짝 (짝 배치: 같은 책상 / 단독: 같은 줄 바로 옆 / 모둠: 같은 모둠 안 바로 옆)
    /// - 1 = 근처 (앞뒤, 대각선, 통로 건너 바로 옆 / 모둠: 같은 모둠)
    /// - 2 이상 = 멀리 (격자 칸 수. 모둠은 1 + 모둠 격자 칸 수)
    pub fn distance(&self, a: usize, b: usize) -> u32 {
        let (sa, sb) = (&self.seats[a], &self.seats[b]);
        if a == b {
            return 0;
        }
        let dr = sa.row.abs_diff(sb.row);
        let dc = sa.col.abs_diff(sb.col);
        match self.preset {
            Preset::Pair => {
                if sa.unit == sb.unit {
                    0
                } else {
                    dr.max(dc)
                }
            }
            Preset::Single => {
                if dr == 0 && dc == 1 {
                    0
                } else {
                    dr.max(dc)
                }
            }
            Preset::Group => {
                if sa.unit == sb.unit {
                    if dr == 0 && dc == 1 {
                        0
                    } else {
                        1
                    }
                } else {
                    let (ga_r, ga_c) = self.group_pos(sa);
                    let (gb_r, gb_c) = self.group_pos(sb);
                    1 + ga_r.abs_diff(gb_r).max(ga_c.abs_diff(gb_c))
                }
            }
        }
    }

    fn group_pos(&self, s: &Seat) -> (u32, u32) {
        let _ = self.group_cols;
        ((s.row - 1) / self.group_h, (s.col - 1) / 2)
    }

    /// 자리가 남을 때 비워 둘 순서: 뒤쪽부터, 같은 줄이면 복도 쪽(오른쪽)부터.
    pub fn empty_order(&self) -> Vec<usize> {
        let mut idx: Vec<usize> = (0..self.seats.len()).collect();
        idx.sort_by(|&a, &b| {
            let (sa, sb) = (&self.seats[a], &self.seats[b]);
            sb.row.cmp(&sa.row).then(sb.col.cmp(&sa.col))
        });
        idx
    }

    /// 짝 관계(거리 0)인 자리 쌍 목록. 남녀 짝 판정에 쓴다.
    pub fn units(&self) -> Vec<Vec<usize>> {
        let mut map: std::collections::BTreeMap<u32, Vec<usize>> = Default::default();
        for (i, s) in self.seats.iter().enumerate() {
            map.entry(s.unit).or_default().push(i);
        }
        map.into_values().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(preset: Preset, rows: u32, cols: u32) -> ClassroomConfig {
        ClassroomConfig { preset, rows, cols, ..Default::default() }
    }

    #[test]
    fn pair_distance_definitions() {
        let l = cfg(Preset::Pair, 3, 6).build();
        let s = |r, c| l.seat_index(r, c).unwrap();
        assert_eq!(l.distance(s(1, 1), s(1, 2)), 0, "같은 책상 = 짝");
        assert_eq!(l.distance(s(1, 2), s(1, 3)), 1, "통로 건너 바로 옆 = 근처");
        assert_eq!(l.distance(s(1, 1), s(2, 1)), 1, "앞뒤 = 근처");
        assert_eq!(l.distance(s(1, 1), s(2, 2)), 1, "대각선 = 근처");
        assert_eq!(l.distance(s(1, 1), s(3, 1)), 2, "두 줄 = 멀리");
        assert_eq!(l.distance(s(1, 1), s(1, 4)), 3);
    }

    #[test]
    fn single_distance() {
        let l = cfg(Preset::Single, 3, 3).build();
        let s = |r, c| l.seat_index(r, c).unwrap();
        assert_eq!(l.distance(s(1, 1), s(1, 2)), 0);
        assert_eq!(l.distance(s(1, 1), s(2, 2)), 1);
        assert_eq!(l.distance(s(1, 1), s(3, 3)), 2);
    }

    #[test]
    fn group_layout_and_distance() {
        let c = ClassroomConfig { preset: Preset::Group, rows: 2, cols: 3, group_size: 5, ..Default::default() };
        let l = c.build();
        assert_eq!((l.rows, l.cols), (6, 6));
        assert_eq!(l.seats.len(), 30);
        assert_eq!(l.cell(3, 2), CellKind::Aisle);
        let s = |r, c| l.seat_index(r, c).unwrap();
        assert_eq!(l.distance(s(1, 1), s(1, 2)), 0);
        assert_eq!(l.distance(s(1, 1), s(3, 1)), 1, "같은 모둠 = 근처");
        assert_eq!(l.distance(s(1, 2), s(1, 3)), 2, "옆 모둠 = 멀리");
        assert_eq!(l.units().len(), 6);
    }

    #[test]
    fn edits_remove_seats() {
        let mut c = cfg(Preset::Pair, 2, 4);
        c.set_cell(1, 1, CellKind::Blank);
        c.set_cell(2, 4, CellKind::Aisle);
        assert_eq!(c.build().seats.len(), 6);
        c.set_cell(1, 1, CellKind::Seat);
        assert_eq!(c.build().seats.len(), 7);
    }

    #[test]
    fn empty_order_back_first() {
        let l = cfg(Preset::Pair, 2, 2).build();
        let o = l.empty_order();
        assert_eq!((l.seats[o[0]].row, l.seats[o[0]].col), (2, 2));
        assert_eq!((l.seats[o[3]].row, l.seats[o[3]].col), (1, 1));
    }
}
