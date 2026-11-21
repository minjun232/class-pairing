use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use seat_core::*;

fn students(m: usize, f: usize) -> Vec<Student> {
    let mut v = Vec::new();
    for i in 0..m {
        v.push(Student { name: format!("남{i}"), gender: Gender::Male });
    }
    for i in 0..f {
        v.push(Student { name: format!("여{i}"), gender: Gender::Female });
    }
    v
}

fn pair_layout(rows: u32, cols: u32, gender: GenderMode) -> Layout {
    ClassroomConfig { preset: Preset::Pair, rows, cols, gender, ..Default::default() }.build()
}

fn solve_with(
    layout: &Layout,
    st: &[Student],
    teacher: &[Rule],
    req: &[(String, Vec<Rule>)],
    prev: &[(String, String)],
    seed: u64,
) -> Result<Arrangement, SolveError> {
    let p = Problem {
        layout: Some(layout),
        students: st,
        teacher_rules: teacher,
        student_rules: req,
        previous_pairs: prev,
        zones: ZoneSettings::default(),
    };
    solve(&p, &mut StdRng::seed_from_u64(seed))
}

fn all_distinct(arr: &Arrangement) -> bool {
    let mut s = arr.seat_of.clone();
    s.sort();
    s.windows(2).all(|w| w[0] != w[1])
}

fn random_place(rng: &mut StdRng) -> Place {
    match rng.gen_range(0..6) {
        0 => Place::Zone([Zone::Front, Zone::Back, Zone::Window, Zone::Hallway, Zone::Center][rng.gen_range(0..5)]),
        1 => {
            let a = rng.gen_range(1..=6);
            Place::Rows(a, (a + rng.gen_range(0..3)).min(6))
        }
        2 => {
            let a = rng.gen_range(1..=6);
            Place::Cols(a, (a + rng.gen_range(0..3)).min(6))
        }
        3 => Place::Near([Facility::Podium, Facility::Board, Facility::FrontDoor, Facility::BackDoor][rng.gen_range(0..4)]),
        4 => Place::Not(Box::new(Place::Near(Facility::BackDoor))),
        _ => Place::Seat(rng.gen_range(1..=6), rng.gen_range(1..=6)),
    }
}

fn random_rules(rng: &mut StdRng, st: &[Student], k: usize) -> Vec<Rule> {
    (0..k)
        .map(|_| {
            let a = st[rng.gen_range(0..st.len())].name.clone();
            let b = st[rng.gen_range(0..st.len())].name.clone();
            match rng.gen_range(0..4) {
                0 => Rule::Allowed { student: a, place: random_place(rng) },
                1 => Rule::Distance { a, b, min: 0, max: Some(0) },
                2 => Rule::Distance { a, b, min: 0, max: Some(1) },
                _ => Rule::Distance { a, b, min: 2, max: None },
            }
        })
        .collect()
}

/// 무작위 교사 조건으로 반복 배치: 결과가 나오면 교사 조건 위반은 0건이어야 한다.
fn teacher_never_violated(runs: u64) {
    let layout = pair_layout(6, 6, GenderMode::Mixed);
    let st = students(17, 16);
    let mut ok = 0;
    for seed in 0..runs {
        let mut rng = StdRng::seed_from_u64(seed * 7919);
        let k = rng.gen_range(0..6);
        let teacher = random_rules(&mut rng, &st, k);
        let req: Vec<(String, Vec<Rule>)> =
            st.iter().take(10).map(|s| (s.name.clone(), random_rules(&mut rng, &st, 1))).collect();
        match solve_with(&layout, &st, &teacher, &req, &[], seed) {
            Ok(arr) => {
                assert!(all_distinct(&arr));
                assert!(satisfies(&layout, &ZoneSettings::default(), &st, &teacher, &arr), "seed {seed}: {teacher:?}");
                ok += 1;
            }
            Err(SolveError::Infeasible) => {}
            Err(e) => panic!("{e}"),
        }
    }
    assert!(ok as f64 > runs as f64 * 0.7, "대부분은 배치되어야 함: {ok}/{runs}");
}

#[test]
fn teacher_rules_never_violated() {
    teacher_never_violated(150);
}

#[test]
#[ignore = "오래 걸림: cargo test --release -- --ignored"]
fn teacher_rules_never_violated_many() {
    teacher_never_violated(3000);
}

#[test]
fn fairness_without_rules() {
    // 조건이 없으면 학생 0번의 자리가 고르게 분포해야 한다.
    let layout = pair_layout(3, 4, GenderMode::Any);
    let st = students(6, 6);
    let runs = 12_000;
    let mut count = vec![0u32; 12];
    for seed in 0..runs {
        let arr = solve_with(&layout, &st, &[], &[], &[], seed).unwrap();
        count[arr.seat_of[0]] += 1;
    }
    let exp = runs as f64 / 12.0;
    let chi2: f64 = count.iter().map(|&c| (c as f64 - exp).powi(2) / exp).sum();
    // 자유도 11, p=0.001 임계값 31.3
    assert!(chi2 < 31.3, "chi2={chi2} {count:?}");
}

#[test]
fn fairness_with_gender_mixing() {
    // 남녀 짝을 켜도 특정 학생이 특정 자리에 몰리면 안 된다.
    let layout = pair_layout(2, 4, GenderMode::Mixed);
    let st = students(4, 4);
    let runs = 4000;
    let mut count = vec![0u32; 8];
    for seed in 0..runs {
        let arr = solve_with(&layout, &st, &[], &[], &[], seed).unwrap();
        count[arr.seat_of[0]] += 1;
    }
    let exp = runs as f64 / 8.0;
    let chi2: f64 = count.iter().map(|&c| (c as f64 - exp).powi(2) / exp).sum();
    assert!(chi2 < 24.3, "chi2={chi2} {count:?}"); // 자유도 7
}

fn same_gender_desks(layout: &Layout, st: &[Student], arr: &Arrangement) -> usize {
    let mut occ = vec![None; layout.seats.len()];
    for (i, &s) in arr.seat_of.iter().enumerate() {
        occ[s] = Some(i);
    }
    layout
        .units()
        .iter()
        .filter(|u| u.len() == 2)
        .filter(|u| matches!((occ[u[0]], occ[u[1]]), (Some(a), Some(b)) if st[a].gender == st[b].gender))
        .count()
}

#[test]
fn gender_pairs_mixed() {
    let layout = pair_layout(6, 6, GenderMode::Mixed);
    let st = students(18, 17);
    for seed in 0..20 {
        let arr = solve_with(&layout, &st, &[], &[], &[], seed).unwrap();
        assert_eq!(same_gender_desks(&layout, &st, &arr), 0);
    }
    // 남자가 많으면 남는 만큼만 같은 성별 짝
    let st = students(20, 14);
    for seed in 0..20 {
        let arr = solve_with(&layout, &st, &[], &[], &[], seed).unwrap();
        assert_eq!(same_gender_desks(&layout, &st, &arr), 3);
    }
}

#[test]
fn teacher_rule_yields_only_as_needed() {
    let layout = pair_layout(6, 6, GenderMode::Mixed);
    let st = students(18, 18);
    let teacher = vec![Rule::Distance { a: "남0".into(), b: "남1".into(), min: 0, max: Some(0) }];
    for seed in 0..20 {
        let arr = solve_with(&layout, &st, &teacher, &[], &[], seed).unwrap();
        assert_eq!(layout.distance(arr.seat_of[0], arr.seat_of[1]), 0);
        // 남0·남1 한 쌍 때문에 여자 한 쌍도 같은 성별이 되어야 함 → 정확히 2
        assert_eq!(same_gender_desks(&layout, &st, &arr), 2);
    }
}

#[test]
fn empty_seats_from_back() {
    let layout = pair_layout(3, 4, GenderMode::Any);
    let st = students(5, 4); // 12자리 중 3자리 빔
    for seed in 0..20 {
        let arr = solve_with(&layout, &st, &[], &[], &[], seed).unwrap();
        let used: Vec<_> = arr.seat_of.iter().map(|&s| (layout.seats[s].row, layout.seats[s].col)).collect();
        for (r, c) in [(3, 4), (3, 3), (3, 2)] {
            assert!(!used.contains(&(r, c)));
        }
    }
}

#[test]
fn infeasible_and_not_enough() {
    let layout = pair_layout(2, 2, GenderMode::Any);
    let st = students(2, 2);
    let teacher = vec![
        Rule::Allowed { student: "남0".into(), place: Place::Seat(1, 1) },
        Rule::Allowed { student: "남1".into(), place: Place::Seat(1, 1) },
    ];
    assert_eq!(solve_with(&layout, &st, &teacher, &[], &[], 1), Err(SolveError::Infeasible));
    let teacher = vec![Rule::Allowed { student: "남0".into(), place: Place::Seat(9, 9) }];
    assert_eq!(solve_with(&layout, &st, &teacher, &[], &[], 1), Err(SolveError::Infeasible));
    let st = students(3, 3);
    assert_eq!(solve_with(&layout, &st, &[], &[], &[], 1), Err(SolveError::NotEnoughSeats(2)));
}

#[test]
fn requests_and_previous_pairs() {
    let layout = pair_layout(5, 6, GenderMode::Any);
    let st = students(15, 15);
    let req = vec![
        ("남0".to_string(), vec![Rule::Allowed { student: "남0".into(), place: Place::Zone(Zone::Window) }]),
        ("여0".to_string(), vec![Rule::Distance { a: "여0".into(), b: "여1".into(), min: 0, max: Some(0) }]),
    ];
    let prev = vec![("남2".to_string(), "여2".to_string()), ("남3".to_string(), "여3".to_string())];
    for seed in 0..20 {
        let arr = solve_with(&layout, &st, &[], &req, &prev, seed).unwrap();
        assert_eq!(layout.seats[arr.seat_of[0]].col, 1);
        assert_eq!(layout.distance(arr.seat_of[15], arr.seat_of[16]), 0);
        assert_ne!(layout.distance(arr.seat_of[2], arr.seat_of[17]), 0);
        assert_ne!(layout.distance(arr.seat_of[3], arr.seat_of[18]), 0);
    }
}

#[test]
fn student_request_never_beats_teacher() {
    let layout = pair_layout(5, 6, GenderMode::Any);
    let st = students(15, 15);
    let teacher = vec![Rule::Distance { a: "남0".into(), b: "여0".into(), min: 2, max: None }];
    let req = vec![("남0".to_string(), vec![Rule::Distance { a: "남0".into(), b: "여0".into(), min: 0, max: Some(0) }])];
    for seed in 0..20 {
        let arr = solve_with(&layout, &st, &teacher, &req, &[], seed).unwrap();
        assert!(layout.distance(arr.seat_of[0], arr.seat_of[15]) >= 2);
    }
}

#[test]
fn group_mixing() {
    let layout = ClassroomConfig { preset: Preset::Group, rows: 2, cols: 3, group_size: 4, gender: GenderMode::Mixed, ..Default::default() }.build();
    let st = students(12, 12);
    for seed in 0..10 {
        let arr = solve_with(&layout, &st, &[], &[], &[], seed).unwrap();
        let mut males = std::collections::HashMap::new();
        for (i, &s) in arr.seat_of.iter().enumerate() {
            if st[i].gender == Gender::Male {
                *males.entry(layout.seats[s].unit).or_insert(0) += 1;
            }
        }
        assert!(males.values().all(|&m| m == 2), "{males:?}");
    }
}

#[test]
fn speed_35_students() {
    let layout = pair_layout(6, 6, GenderMode::Mixed);
    let st = students(18, 17);
    let mut rng = StdRng::seed_from_u64(42);
    let teacher = random_rules(&mut rng, &st, 5);
    let req: Vec<(String, Vec<Rule>)> = st.iter().map(|s| (s.name.clone(), random_rules(&mut rng, &st, 2))).collect();
    let t = std::time::Instant::now();
    let _ = solve_with(&layout, &st, &teacher, &req, &[], 1);
    let limit = if cfg!(debug_assertions) { 30.0 } else { 1.5 };
    assert!(t.elapsed().as_secs_f64() < limit, "{:?}", t.elapsed());
}
