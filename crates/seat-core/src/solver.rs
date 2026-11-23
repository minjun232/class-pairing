//! 배치 알고리즘.
//!
//! 우선순위(설계 8-4)를 **사전식 비용 벡터**로 표현한다.
//!
//! | 단계 | 내용 |
//! |---|---|
//! | 0 | 교사 조건 위반 (0이 아니면 배치 불가) |
//! | 1 | 빈자리가 뒤쪽부터가 아님 |
//! | 2 | 성별 버튼 (남녀 짝 / 남녀 고르게) |
//! | 3 | 학생 요청 (학생마다 요청 수로 나눈 가중치 → 요청이 많은 학생이 유리하지 않음) |
//! | 4 | 직전 짝 다시 짝 |
//!
//! 1. 균등 무작위 순열에서 시작한다(조건이 없으면 이것이 곧 결과 → 완전히 공정).
//! 2. 단계별로 담금질(simulated annealing)을 한다. 각 단계에서는 앞 단계 비용이 나빠지는
//!    교환을 받지 않으므로, 하위 조건은 상위 조건과 부딪히는 만큼만 양보한다.
//! 3. 마지막으로 비용 벡터가 **똑같이 유지되는** 교환만 받아들이는 무작위 걷기를 한다.
//!    교환 제안이 대칭이므로 최적 배치들 사이에서 치우침 없이 섞인다(명단 순서 영향 제거).
//!
//! 시간 대신 반복 횟수로 예산을 정하므로 어떤 플랫폼에서도 결과가 재현 가능하다.

use crate::layout::{GenderMode, Layout, Preset};
use crate::rules::{Rule, ZoneSettings};
use rand::seq::SliceRandom;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Gender {
    Male,
    Female,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Student {
    pub name: String,
    pub gender: Gender,
}

/// 배치 문제 하나.
#[derive(Debug, Clone, Default)]
pub struct Problem<'a> {
    pub layout: Option<&'a Layout>,
    pub students: &'a [Student],
    /// 교사 조건 (절대 지킴)
    pub teacher_rules: &'a [Rule],
    /// 학생 요청: (요청한 학생 이름, 그 학생의 규칙들)
    pub student_rules: &'a [(String, Vec<Rule>)],
    /// 직전 확정 좌석표의 짝 (이름 쌍)
    pub previous_pairs: &'a [(String, String)],
    pub zones: ZoneSettings,
    /// true면 교사 조건을 모두 지킬 수 없을 때 거부하지 않고, 지킬 수 있는 만큼 지킨 배치를 낸다.
    /// (사용자 결정 2026-11-22: 배치가 막히는 것보다 지킬 수 없는 조건을 무시하는 편이 낫다)
    pub relax_teacher: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Arrangement {
    /// 학생 번호(입력 순서) → 자리 번호(`Layout::seats`)
    pub seat_of: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SolveError {
    #[error("자리가 {0}개 부족해요")]
    NotEnoughSeats(usize),
    #[error("배치할 수 없어요. 설정을 확인해 주세요.")]
    Infeasible,
}

const LEVELS: usize = 5;
type Cost = [u64; LEVELS];
/// 학생 요청 가중치 기준값 (1..=16의 최소공배수: 요청 수로 나눠도 정수)
const REQ_UNIT: u64 = 720_720;

enum Constraint {
    Allowed { s: usize, gap: Vec<u32> },
    Distance { a: usize, b: usize, min: u32, max: u32 },
}

struct Compiled<'a> {
    layout: &'a Layout,
    n: usize,
    hard: Vec<Constraint>,
    /// (가중치, 조건)
    soft: Vec<(u64, Constraint)>,
    prev: Vec<(usize, usize)>,
    gender: Vec<Gender>,
    designated_empty: Vec<bool>,
    units: Vec<Vec<usize>>,
    gender_on: bool,
    male_ratio: (u64, u64),
    /// 남녀 짝: 자리마다 남학생 자리인지 (`Layout::male_cell`)
    seat_male: Vec<bool>,
}

/// 규칙 하나를 내부 조건으로 바꾼다. 명단에 없는 이름이면 None.
fn compile_rule(rule: &Rule, layout: &Layout, z: &ZoneSettings, index: &HashMap<&str, usize>) -> Option<Constraint> {
    match rule {
        Rule::Allowed { student, place } => {
            let s = *index.get(student.as_str())?;
            let ok = place.resolve(layout, z);
            // 각 자리에서 허용 자리까지의 격자 거리 (위반 정도)
            let gap = layout
                .seats
                .iter()
                .map(|seat| {
                    ok.iter()
                        .map(|&o| {
                            let t = &layout.seats[o];
                            seat.row.abs_diff(t.row).max(seat.col.abs_diff(t.col))
                        })
                        .min()
                        .unwrap_or(u32::MAX / 4)
                })
                .collect::<Vec<_>>();
            // 허용 자리 자신의 gap은 0, 그 외에는 최소 1
            let gap = gap
                .into_iter()
                .enumerate()
                .map(|(i, g)| if ok.contains(&i) { 0 } else { g.max(1) })
                .collect();
            Some(Constraint::Allowed { s, gap })
        }
        Rule::Distance { a, b, min, max } => {
            let a = *index.get(a.as_str())?;
            let b = *index.get(b.as_str())?;
            if a == b {
                return None;
            }
            let max = max.unwrap_or(u32::MAX);
            let (min, max) = if min <= &max { (*min, max) } else { (max, *min) };
            Some(Constraint::Distance { a, b, min, max })
        }
    }
}

impl Constraint {
    fn violation(&self, layout: &Layout, pos: &[usize]) -> u64 {
        match self {
            Constraint::Allowed { s, gap } => gap[pos[*s]] as u64,
            Constraint::Distance { a, b, min, max } => {
                let d = layout.distance(pos[*a], pos[*b]);
                if d < *min {
                    (*min - d) as u64
                } else if d > *max {
                    (d - *max) as u64
                } else {
                    0
                }
            }
        }
    }
}

impl<'a> Compiled<'a> {
    /// pos[엔티티] = 자리. 엔티티 0..n은 학생, n..은 빈자리(유령).
    fn eval(&self, pos: &[usize], occ: &[usize]) -> Cost {
        let l = self.layout;
        let mut c = [0u64; LEVELS];
        for h in &self.hard {
            c[0] += h.violation(l, pos);
        }
        for e in self.n..pos.len() {
            if !self.designated_empty[pos[e]] {
                c[1] += 1;
            }
        }
        if self.gender_on {
            match l.preset {
                Preset::Pair => {
                    // 남녀 짝: 자리마다 정해진 성별(열마다 번갈아)과 다른 학생 수
                    for (s, &e) in occ.iter().enumerate() {
                        if e < self.n && (self.gender[e] == Gender::Male) != self.seat_male[s] {
                            c[2] += 1;
                        }
                    }
                }
                Preset::Group => {
                    let (m, total) = self.male_ratio;
                    for u in &self.units {
                        let members: Vec<usize> = u.iter().map(|&s| occ[s]).filter(|&e| e < self.n).collect();
                        let size = members.len() as u64;
                        let males = members.iter().filter(|&&e| self.gender[e] == Gender::Male).count() as u64;
                        // 이상적인 남학생 수 = size * m / total, 허용 범위 [내림, 올림]
                        let lo = size * m / total;
                        let hi = (size * m).div_ceil(total);
                        if males < lo {
                            c[2] += lo - males;
                        } else if males > hi {
                            c[2] += males - hi;
                        }
                    }
                }
                Preset::Single => {}
            }
        }
        for (w, s) in &self.soft {
            if s.violation(l, pos) > 0 {
                c[3] += w;
            }
        }
        for &(a, b) in &self.prev {
            if l.distance(pos[a], pos[b]) == 0 {
                c[4] += 1;
            }
        }
        c
    }

    fn level_active(&self, level: usize) -> bool {
        match level {
            0 => !self.hard.is_empty(),
            1 => self.designated_empty.iter().any(|&e| e),
            2 => self.gender_on,
            3 => !self.soft.is_empty(),
            4 => !self.prev.is_empty(),
            _ => false,
        }
    }
}

const SCALE: Cost = [1, 1, 1, REQ_UNIT, 1];

pub fn solve<R: Rng + ?Sized>(p: &Problem, rng: &mut R) -> Result<Arrangement, SolveError> {
    let layout = p.layout.expect("layout");
    let n = p.students.len();
    let seats = layout.seats.len();
    if n > seats {
        return Err(SolveError::NotEnoughSeats(n - seats));
    }
    let index: HashMap<&str, usize> = p.students.iter().enumerate().map(|(i, s)| (s.name.as_str(), i)).collect();

    let mut hard = Vec::new();
    for r in p.teacher_rules {
        if let Some(c) = compile_rule(r, layout, &p.zones, &index) {
            if let Constraint::Allowed { gap, .. } = &c {
                if !gap.iter().any(|&g| g == 0) {
                    // 앉을 수 있는 자리가 하나도 없는 조건
                    if p.relax_teacher {
                        continue;
                    }
                    return Err(SolveError::Infeasible);
                }
            }
            hard.push(c);
        }
    }
    let mut soft = Vec::new();
    for (who, rules) in p.student_rules {
        if !index.contains_key(who.as_str()) {
            continue;
        }
        let compiled: Vec<Constraint> =
            rules.iter().filter_map(|r| compile_rule(r, layout, &p.zones, &index)).collect();
        let k = compiled.len().max(1) as u64;
        for c in compiled {
            soft.push((REQ_UNIT / k, c));
        }
    }
    let prev = p
        .previous_pairs
        .iter()
        .filter_map(|(a, b)| Some((*index.get(a.as_str())?, *index.get(b.as_str())?)))
        .filter(|(a, b)| a != b)
        .collect();
    let mut designated_empty = vec![false; seats];
    for &s in layout.empty_order().iter().take(seats - n) {
        designated_empty[s] = true;
    }
    let males = p.students.iter().filter(|s| s.gender == Gender::Male).count() as u64;
    let gender_on = layout.gender == GenderMode::Mixed && layout.preset != Preset::Single && n > 0;
    let cm = Compiled {
        layout,
        n,
        hard,
        soft,
        prev,
        gender: p.students.iter().map(|s| s.gender).collect(),
        designated_empty,
        units: layout.units(),
        gender_on,
        male_ratio: (males, n.max(1) as u64),
        seat_male: layout.seats.iter().map(|s| layout.male_cell(s.row, s.col)).collect(),
    };

    if n == 0 {
        return Ok(Arrangement { seat_of: vec![] });
    }

    let iters_per_level = (seats * seats * 40).clamp(4_000, 80_000);
    let mut best: Option<(Cost, Vec<usize>)> = None;
    let mut restart = 0;
    loop {
        let (cost, pos) = run_once(&cm, rng, iters_per_level);
        if best.as_ref().map_or(true, |(b, _)| cost < *b) {
            best = Some((cost, pos));
        }
        restart += 1;
        let b = &best.as_ref().unwrap().0;
        let all_zero = b.iter().all(|&x| x == 0);
        if all_zero || (restart >= 3 && b[0] == 0) || restart >= 12 {
            break;
        }
    }
    let (cost, mut pos) = best.unwrap();
    if cost[0] > 0 && !p.relax_teacher {
        return Err(SolveError::Infeasible);
    }
    // 최적 배치들 사이의 무작위 걷기
    let mut occ = occupancy(&pos, seats);
    let walk = (seats * seats * 30).clamp(2_000, 60_000);
    for _ in 0..walk {
        let i = rng.gen_range(0..seats);
        let j = rng.gen_range(0..seats);
        if i == j || (occ[i] >= n && occ[j] >= n) {
            continue;
        }
        swap(&mut pos, &mut occ, i, j);
        if cm.eval(&pos, &occ) != cost {
            swap(&mut pos, &mut occ, i, j);
        }
    }
    Ok(Arrangement { seat_of: pos[..n].to_vec() })
}

fn occupancy(pos: &[usize], seats: usize) -> Vec<usize> {
    let mut occ = vec![usize::MAX; seats];
    for (e, &s) in pos.iter().enumerate() {
        occ[s] = e;
    }
    occ
}

fn swap(pos: &mut [usize], occ: &mut [usize], i: usize, j: usize) {
    let (a, b) = (occ[i], occ[j]);
    occ.swap(i, j);
    pos[a] = j;
    pos[b] = i;
}

fn run_once<R: Rng + ?Sized>(cm: &Compiled, rng: &mut R, iters: usize) -> (Cost, Vec<usize>) {
    let seats = cm.layout.seats.len();
    let n = cm.n;
    // 균등 무작위 시작: 학생은 사용 자리에, 유령은 지정 빈자리에
    let mut used: Vec<usize> = (0..seats).filter(|&s| !cm.designated_empty[s]).collect();
    used.shuffle(rng);
    let mut empty: Vec<usize> = (0..seats).filter(|&s| cm.designated_empty[s]).collect();
    empty.shuffle(rng);
    let mut pos: Vec<usize> = used.into_iter().chain(empty).collect();
    debug_assert_eq!(pos.len(), seats);
    let mut occ = occupancy(&pos, seats);
    let mut cur = cm.eval(&pos, &occ);

    for level in 0..LEVELS {
        if !cm.level_active(level) || cur[level] == 0 {
            continue;
        }
        let mut t: f64 = 2.0;
        let t_end: f64 = 0.02;
        let decay = (t_end / t).powf(1.0 / iters as f64);
        let mut best_c = cur;
        let mut best_pos = pos.clone();
        for _ in 0..iters {
            t *= decay;
            let i = rng.gen_range(0..seats);
            let j = rng.gen_range(0..seats);
            if i == j || (occ[i] >= n && occ[j] >= n) {
                continue;
            }
            swap(&mut pos, &mut occ, i, j);
            let new = cm.eval(&pos, &occ);
            let accept = match new[..level].cmp(&cur[..level]) {
                std::cmp::Ordering::Less => true,
                std::cmp::Ordering::Greater => false,
                std::cmp::Ordering::Equal => {
                    if new[level] <= cur[level] {
                        true
                    } else {
                        let delta = (new[level] - cur[level]) as f64 / SCALE[level] as f64;
                        rng.gen::<f64>() < (-delta / t).exp()
                    }
                }
            };
            if accept {
                cur = new;
                if cur[..=level] < best_c[..=level] {
                    best_c = cur;
                    best_pos.clone_from(&pos);
                    if best_c[..=level].iter().all(|&x| x == 0) {
                        break;
                    }
                }
            } else {
                swap(&mut pos, &mut occ, i, j);
            }
        }
        pos = best_pos;
        occ = occupancy(&pos, seats);
        cur = cm.eval(&pos, &occ);
    }
    (cur, pos)
}

/// 교사 조건을 모두 지키는지 (테스트·검증용).
pub fn satisfies(layout: &Layout, z: &ZoneSettings, students: &[Student], rules: &[Rule], arr: &Arrangement) -> bool {
    let index: HashMap<&str, usize> = students.iter().enumerate().map(|(i, s)| (s.name.as_str(), i)).collect();
    rules.iter().all(|r| match compile_rule(r, layout, z, &index) {
        None => true,
        Some(c) => c.violation(layout, &arr.seat_of) == 0,
    })
}

/// 배치에서 짝(거리 0) 이름 쌍을 뽑는다. 확정 시 "직전 짝"으로 저장한다.
pub fn pairs_of(layout: &Layout, students: &[Student], arr: &Arrangement) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for i in 0..students.len() {
        for j in i + 1..students.len() {
            if layout.distance(arr.seat_of[i], arr.seat_of[j]) == 0 {
                out.push((students[i].name.clone(), students[j].name.clone()));
            }
        }
    }
    out
}
