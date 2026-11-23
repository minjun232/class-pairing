//! 조건의 두 기본 단위: 자리 집합(`allowed`)과 두 사람 거리(`distance`).
//!
//! LLM은 자리 목록을 직접 나열하는 대신 [`Place`] 식(앞쪽, 3열, 뒷문 근처 아님 …)으로
//! 자리 집합을 표현하고, 프로그램이 교실 모양에 맞춰 실제 자리 목록으로 바꾼다.
//! 그래서 해석 결과는 교실 모양이 바뀌어도 다시 해석할 필요가 없다.

use crate::layout::{Layout, Preset};
use serde::{Deserialize, Serialize};

/// 애매한 표현의 구역.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Zone {
    #[serde(rename = "앞쪽")]
    Front,
    #[serde(rename = "뒤쪽")]
    Back,
    #[serde(rename = "창가")]
    Window,
    #[serde(rename = "복도")]
    Hallway,
    #[serde(rename = "가운데")]
    Center,
    /// 맨 앞줄
    #[serde(rename = "맨앞")]
    FrontMost,
    /// 맨 뒷줄
    #[serde(rename = "맨뒤")]
    BackMost,
}

/// 고정 시설 (위치는 바꿀 수 없다).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Facility {
    /// 앞쪽 가운데
    #[serde(rename = "칠판")]
    Board,
    /// 앞쪽 창문 쪽(왼쪽)
    #[serde(rename = "교탁")]
    Podium,
    /// 왼쪽 벽
    #[serde(rename = "창문")]
    Window,
    /// 오른쪽 벽 앞
    #[serde(rename = "앞문")]
    FrontDoor,
    /// 오른쪽 벽 뒤
    #[serde(rename = "뒷문")]
    BackDoor,
}

/// 자리 집합 식.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Place {
    Zone(Zone),
    /// 줄 범위 (포함), 1 = 맨 앞
    Rows(u32, u32),
    /// 열 범위 (포함), 1 = 창문 쪽
    Cols(u32, u32),
    /// 특정 자리 (줄, 열)
    Seat(u32, u32),
    Near(Facility),
    Not(Box<Place>),
    Any(Vec<Place>),
    All(Vec<Place>),
}

/// 규칙 하나. 이름은 명단에 적힌 그대로다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Rule {
    Allowed { student: String, place: Place },
    Distance { a: String, b: String, min: u32, max: Option<u32> },
}

impl Rule {
    pub fn names(&self) -> Vec<&str> {
        match self {
            Rule::Allowed { student, .. } => vec![student],
            Rule::Distance { a, b, .. } => vec![a, b],
        }
    }
}

/// 애매한 표현의 기준값. Settings.ini에서 덮어쓸 수 있다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ZoneSettings {
    /// 앞쪽 줄 수 (없으면 2줄, 교실이 크면 앞 1/3)
    pub front_rows: Option<u32>,
    /// 뒤쪽 줄 수 (없으면 2줄)
    pub back_rows: Option<u32>,
    /// 창가 열 수 (없으면 1열)
    pub window_cols: Option<u32>,
    /// 복도 열 수 (없으면 1열)
    pub hallway_cols: Option<u32>,
}

impl ZoneSettings {
    pub fn front(&self, rows: u32) -> u32 {
        self.front_rows.unwrap_or_else(|| (rows / 3).max(2)).clamp(1, rows)
    }
    pub fn back(&self, rows: u32) -> u32 {
        self.back_rows.unwrap_or(2).clamp(1, rows)
    }
    pub fn window(&self, cols: u32) -> u32 {
        self.window_cols.unwrap_or(1).clamp(1, cols)
    }
    pub fn hallway(&self, cols: u32) -> u32 {
        self.hallway_cols.unwrap_or(1).clamp(1, cols)
    }
}

/// 시설 "근처" 판정 범위 (격자 칸 수)
const FACILITY_NEAR: u32 = 2;

impl Place {
    /// 자리 하나가 이 식에 맞는지.
    pub fn contains(&self, layout: &Layout, z: &ZoneSettings, row: u32, col: u32) -> bool {
        let (rows, cols) = (layout.rows, layout.cols);
        match self {
            Place::Zone(zone) => match zone {
                Zone::Front => row <= z.front(rows),
                Zone::Back => row > rows - z.back(rows),
                Zone::Window => col <= window_edge(layout, z),
                Zone::Hallway => col >= hallway_edge(layout, z),
                Zone::Center => {
                    let (lo, hi) = center_cols(cols);
                    col >= lo && col <= hi
                }
                Zone::FrontMost => Some(row) == layout.seats.iter().map(|s| s.row).min(),
                Zone::BackMost => Some(row) == layout.seats.iter().map(|s| s.row).max(),
            },
            Place::Rows(a, b) => row >= *a.min(b) && row <= *a.max(b),
            Place::Cols(a, b) => col >= *a.min(b) && col <= *a.max(b),
            Place::Seat(r, c) => row == *r && col == *c,
            Place::Near(f) => {
                let near_point = |pr: i64, pc: i64| {
                    let dr = (row as i64 - pr).unsigned_abs() as u32;
                    let dc = (col as i64 - pc).unsigned_abs() as u32;
                    dr.max(dc) <= FACILITY_NEAR
                };
                match f {
                    Facility::Board => {
                        let (lo, hi) = center_cols(cols);
                        row <= z.front(rows) && col >= lo && col <= hi
                    }
                    Facility::Podium => near_point(0, 1),
                    Facility::Window => col <= window_edge(layout, z),
                    Facility::FrontDoor => near_point(1, cols as i64 + 1),
                    Facility::BackDoor => near_point(rows as i64, cols as i64 + 1),
                }
            }
            Place::Not(p) => !p.contains(layout, z, row, col),
            Place::Any(ps) => ps.iter().any(|p| p.contains(layout, z, row, col)),
            Place::All(ps) => ps.iter().all(|p| p.contains(layout, z, row, col)),
        }
    }

    /// 이 식에 맞는 자리 번호 목록.
    pub fn resolve(&self, layout: &Layout, z: &ZoneSettings) -> Vec<usize> {
        layout
            .seats
            .iter()
            .enumerate()
            .filter(|(_, s)| self.contains(layout, z, s.row, s.col))
            .map(|(i, _)| i)
            .collect()
    }
}

/// 짝 배치에서 이 열이 속한 책상의 (첫 열, 끝 열). 1-2열, 3-4열 … 이 한 책상이고,
/// 열 수가 홀수면 마지막 열은 혼자다.
fn desk_of(layout: &Layout, col: u32) -> (u32, u32) {
    let start = col - (col - 1) % 2;
    (start, (start + 1).min(layout.cols))
}

/// 창가 = 왼쪽 몇 열. 짝 배치에서는 책상 단위로 넓힌다 (창가 1열 → 1~2열 책상).
/// 남녀 짝에서 창가를 원한 여학생이 창가 책상의 여학생 자리에 앉을 수 있게.
fn window_edge(layout: &Layout, z: &ZoneSettings) -> u32 {
    let n = z.window(layout.cols);
    if layout.preset == Preset::Pair { desk_of(layout, n).1 } else { n }
}

/// 복도 = 오른쪽 몇 열(이 열부터). 짝 배치에서는 책상 단위로 넓힌다.
fn hallway_edge(layout: &Layout, z: &ZoneSettings) -> u32 {
    let first = layout.cols - z.hallway(layout.cols) + 1;
    if layout.preset == Preset::Pair { desk_of(layout, first).0 } else { first }
}

/// 가운데 열 범위: 가운데 1/3 (최소 1열)
fn center_cols(cols: u32) -> (u32, u32) {
    let third = cols / 3;
    let lo = third + 1;
    let hi = (cols - third).max(lo);
    (lo, hi)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{ClassroomConfig, Preset};

    fn layout() -> Layout {
        ClassroomConfig { preset: Preset::Pair, rows: 6, cols: 6, ..Default::default() }.build()
    }

    fn rc(l: &Layout, v: Vec<usize>) -> Vec<(u32, u32)> {
        v.into_iter().map(|i| (l.seats[i].row, l.seats[i].col)).collect()
    }

    #[test]
    fn zones() {
        let l = layout();
        let z = ZoneSettings::default();
        assert_eq!(Place::Zone(Zone::Front).resolve(&l, &z).len(), 12);
        assert_eq!(Place::Zone(Zone::Back).resolve(&l, &z).len(), 12);
        // 짝 배치: 창가·복도는 책상 단위 (1~2열, 5~6열)
        assert_eq!(Place::Zone(Zone::Window).resolve(&l, &z).len(), 12);
        assert!(rc(&l, Place::Zone(Zone::Window).resolve(&l, &z)).iter().all(|&(_, c)| c <= 2));
        assert!(rc(&l, Place::Zone(Zone::Hallway).resolve(&l, &z)).iter().all(|&(_, c)| c >= 5));
        // 열 수가 홀수면 마지막 열은 혼자 → 복도 = 그 열만
        let odd = ClassroomConfig { preset: Preset::Pair, rows: 2, cols: 5, ..Default::default() }.build();
        assert!(rc(&odd, Place::Zone(Zone::Hallway).resolve(&odd, &z)).iter().all(|&(_, c)| c == 5));
        // 단독 배치는 지금처럼 열 단위
        let single = ClassroomConfig { preset: Preset::Single, rows: 2, cols: 4, ..Default::default() }.build();
        assert!(rc(&single, Place::Zone(Zone::Window).resolve(&single, &z)).iter().all(|&(_, c)| c == 1));
        assert!(rc(&l, Place::Zone(Zone::Center).resolve(&l, &z)).iter().all(|&(_, c)| c == 3 || c == 4));
        let z3 = ZoneSettings { front_rows: Some(3), ..Default::default() };
        assert_eq!(Place::Zone(Zone::Front).resolve(&l, &z3).len(), 18);
    }

    #[test]
    fn big_room_front_is_third() {
        let l = ClassroomConfig { preset: Preset::Single, rows: 9, cols: 4, ..Default::default() }.build();
        assert_eq!(Place::Zone(Zone::Front).resolve(&l, &ZoneSettings::default()).len(), 12);
    }

    #[test]
    fn facilities() {
        let l = layout();
        let z = ZoneSettings::default();
        let podium = rc(&l, Place::Near(Facility::Podium).resolve(&l, &z));
        assert!(podium.contains(&(1, 1)) && podium.contains(&(2, 3)));
        assert!(!podium.contains(&(3, 1)) && !podium.contains(&(1, 4)));
        let back = rc(&l, Place::Near(Facility::BackDoor).resolve(&l, &z));
        assert!(back.contains(&(6, 6)) && back.contains(&(4, 5)));
        assert!(!back.contains(&(3, 6)));
        let not_back = Place::Not(Box::new(Place::Near(Facility::BackDoor))).resolve(&l, &z);
        assert_eq!(not_back.len() + back.len(), 36);
    }

    #[test]
    fn rule_json_roundtrip() {
        let r = Rule::Allowed { student: "김민수".into(), place: Place::Any(vec![Place::Zone(Zone::Front), Place::Seat(3, 2)]) };
        let s = serde_json::to_string(&r).unwrap();
        assert_eq!(serde_json::from_str::<Rule>(&s).unwrap(), r);
    }
}
