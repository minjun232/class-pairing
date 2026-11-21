//! 프로그램 상태와 동작. Tauri와 분리해 두어 테스트할 수 있다.
//!
//! 비밀 유지: 이 모듈이 화면으로 돌려보내는 값에는 교사 조건 문장이나 해석 결과가 절대 들어가지 않는다.
//! 오류 문구도 원인(어떤 조건인지)을 말하지 않는다.

use rand::rngs::StdRng;
use rand::SeedableRng;
use seat_core::{solve, Arrangement, ClassroomConfig, Layout, Problem, Rule, SolveError, Student};
use seat_interpret::{Interpreter, LlmEngine, INTERPRETER_VERSION};
use seat_storage::settings::TeacherSettings;
use seat_storage::state::{AppState, ClassState, ConfirmedSeating, PlacedName};
use seat_storage::{content_hash, docx, roster, settings, state, AppPaths};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

/// 모델 상태
#[derive(Clone)]
#[allow(dead_code)] // llm 기능을 끄고 빌드하면 일부 상태는 쓰이지 않는다
pub enum Engine {
    Loading,
    Ready(Arc<dyn LlmEngine>),
    Missing,
    Unsupported,
    Failed,
}

impl Engine {
    fn code(&self) -> &'static str {
        match self {
            Engine::Loading => "loading",
            Engine::Ready(_) => "ready",
            Engine::Missing => "missing",
            Engine::Unsupported => "unsupported",
            Engine::Failed => "failed",
        }
    }
}

/// 학생 요청 하나 (메모리에만)
#[derive(Clone, Default)]
struct Request {
    text: String,
    rules: Option<Vec<Rule>>,
}

struct ClassSession {
    name: String,
    students: Vec<Student>,
    roster_error: Option<String>,
    roster_hash: String,
    state: ClassState,
    teacher: TeacherSettings,
    /// 배치했지만 확정하지 않은 좌석 (자리 번호 → 이름)
    unconfirmed: Option<(ClassroomConfig, Layout, Vec<Option<String>>)>,
    /// 확정한 좌석과 저장한 Word 경로
    confirmed: Option<(Layout, Vec<Option<String>>, Option<PathBuf>)>,
}

#[derive(Default)]
struct Inner {
    class: Option<ClassSession>,
    /// 반 이름 → (학생 이름 → 요청). 프로그램이 켜져 있는 동안만 유지.
    requests: HashMap<String, BTreeMap<String, Request>>,
    worker_running: bool,
}

pub struct Core {
    pub paths: AppPaths,
    inner: Mutex<Inner>,
    engine: RwLock<Engine>,
    model_fingerprint: String,
}

#[derive(Serialize)]
pub struct Boot {
    pub classes: Vec<String>,
    pub last: Option<String>,
}

#[derive(Serialize)]
pub struct StudentView {
    pub name: String,
    pub gender: seat_core::Gender,
    pub request: String,
}

#[derive(Serialize)]
pub struct ClassView {
    pub name: String,
    pub students: Vec<StudentView>,
    pub roster_error: Option<String>,
    pub classroom: ClassroomConfig,
    pub layout: Layout,
    /// 직전에 확정한 배치의 날짜 (없으면 None)
    pub previous_date: Option<String>,
}

/// 직전에 확정한 배치 (현재 배치 보기)
#[derive(Debug, Serialize)]
pub struct PreviousView {
    pub date: String,
    pub layout: Layout,
    pub names: Vec<Option<String>>,
}

#[derive(Serialize)]
pub struct Status {
    pub model: &'static str,
    /// 해석이 남은 문장 수 (교사·학생 합계, 어떤 것인지는 알려주지 않는다)
    pub pending: usize,
}

#[derive(Debug, Serialize)]
pub struct ArrangeView {
    pub layout: Layout,
    /// 자리 번호 → 이름
    pub names: Vec<Option<String>>,
}

#[derive(Serialize)]
pub struct Unsaved {
    pub requests: bool,
    pub unconfirmed: bool,
}

pub const ERR_WAIT: &str = "wait";
const ERR_IMPOSSIBLE: &str = "배치할 수 없어요. 설정을 확인해 주세요.";

fn model_fingerprint(paths: &AppPaths) -> String {
    match std::fs::metadata(paths.model()) {
        Ok(m) => format!("model:{}", m.len()),
        Err(_) => "model:none".into(),
    }
}

impl Core {
    pub fn new(paths: AppPaths) -> Arc<Self> {
        let fp = model_fingerprint(&paths);
        let _ = std::fs::create_dir_all(paths.classes());
        let _ = std::fs::create_dir_all(paths.system());
        if !paths.template().exists() {
            let _ = roster::write_template(&paths.template());
        }
        Arc::new(Self { paths, inner: Mutex::default(), engine: RwLock::new(Engine::Loading), model_fingerprint: fp })
    }

    pub fn set_engine(self: &Arc<Self>, e: Engine) {
        *self.engine.write().unwrap() = e;
        self.kick();
    }

    fn engine(&self) -> Engine {
        self.engine.read().unwrap().clone()
    }

    pub fn boot(&self) -> Result<Boot, String> {
        let classes = self.paths.list_classes().map_err(|e| e.to_string())?;
        let app: AppState = state::load(&self.paths.app_dat());
        let last = app.last_class.filter(|c| classes.contains(c)).or_else(|| classes.first().cloned());
        Ok(Boot { classes, last })
    }

    pub fn add_class(&self, name: &str) -> Result<Boot, String> {
        self.paths.add_class(name).map_err(|e| e.to_string())?;
        let mut b = self.boot()?;
        b.last = Some(name.trim().to_string());
        Ok(b)
    }

    fn cache_key(&self, roster_hash: &str) -> String {
        format!("{INTERPRETER_VERSION}|{roster_hash}|{}", self.model_fingerprint)
    }

    /// 명단·Settings.ini를 다시 읽는다(교사가 프로그램을 켠 채로 고쳤을 수 있다).
    fn reload(&self, s: &mut ClassSession) {
        match std::fs::read(self.paths.roster(&s.name))
            .map_err(|_| ())
            .and_then(|bytes| roster::read_roster(&self.paths.roster(&s.name)).map(|r| (bytes, r)).map_err(|_| ()))
        {
            Ok((bytes, students)) => {
                s.roster_hash = content_hash(&[&bytes]);
                s.students = students;
                s.roster_error = None;
            }
            Err(()) => {
                // 다시 읽어서 정확한 이유를 얻는다 (중복 이름 등)
                s.roster_error = Some(match roster::read_roster(&self.paths.roster(&s.name)) {
                    Err(seat_storage::StorageError::Invalid(m)) => m,
                    _ => "명단.xlsx를 읽을 수 없어요. 엑셀에서 열려 있다면 닫고 다시 시도해 주세요.".into(),
                });
                s.students.clear();
                s.roster_hash = "error".into();
            }
        }
        if let Ok((t, _)) = settings::read(&self.paths.settings(&s.name)) {
            s.teacher = t;
        }
        // 명단을 잠깐 못 읽었을 때(엑셀에서 열림 등)는 캐시를 건드리지 않는다.
        // 지우면 모델을 못 쓰는 PC에서 교사 조건을 영영 못 쓰게 된다.
        if s.roster_error.is_some() {
            return;
        }
        let key = self.cache_key(&s.roster_hash);
        if s.state.teacher_cache.key != key {
            s.state.teacher_cache.key = key;
            s.state.teacher_cache.entries.clear();
        }
        // 지금 Settings.ini에 없는 문장의 캐시는 버린다
        let lines: std::collections::HashSet<&String> = s.teacher.conditions.iter().collect();
        s.state.teacher_cache.entries.retain(|k, _| lines.contains(k));
    }

    fn save_state(&self, s: &ClassSession) {
        let _ = state::save(&self.paths.state_file(&s.name), &s.state);
    }

    pub fn open_class(self: &Arc<Self>, name: &str) -> Result<ClassView, String> {
        if !self.paths.class_dir(name).is_dir() {
            return Err("반 폴더를 찾을 수 없어요".into());
        }
        let _ = state::save(&self.paths.app_dat(), &AppState { last_class: Some(name.to_string()) });
        let mut s = ClassSession {
            name: name.to_string(),
            students: vec![],
            roster_error: None,
            roster_hash: String::new(),
            state: state::load(&self.paths.state_file(name)),
            teacher: TeacherSettings::default(),
            unconfirmed: None,
            confirmed: None,
        };
        self.reload(&mut s);
        self.save_state(&s);
        {
            let mut inner = self.inner.lock().unwrap();
            inner.class = Some(s);
        }
        self.kick();
        self.class_view()
    }

    pub fn class_view(&self) -> Result<ClassView, String> {
        let inner = self.inner.lock().unwrap();
        let s = inner.class.as_ref().ok_or("반을 먼저 골라 주세요")?;
        let reqs = inner.requests.get(&s.name);
        Ok(ClassView {
            name: s.name.clone(),
            students: s
                .students
                .iter()
                .map(|st| StudentView {
                    name: st.name.clone(),
                    gender: st.gender,
                    request: reqs.and_then(|r| r.get(&st.name)).map(|r| r.text.clone()).unwrap_or_default(),
                })
                .collect(),
            roster_error: s.roster_error.clone(),
            classroom: s.state.classroom.clone(),
            layout: s.state.classroom.build(),
            previous_date: s.state.previous.as_ref().map(|p| p.date.clone()),
        })
    }

    /// 화면에 들어올 때마다 파일을 다시 읽는다.
    pub fn refresh(self: &Arc<Self>) -> Result<ClassView, String> {
        {
            let mut inner = self.inner.lock().unwrap();
            let s = inner.class.as_mut().ok_or("반을 먼저 골라 주세요")?;
            let mut s2 = std::mem::replace(s, empty_session());
            self.reload(&mut s2);
            self.save_state(&s2);
            *inner.class.as_mut().unwrap() = s2;
        }
        self.kick();
        self.class_view()
    }

    /// 프로그램에서 입력한 명단을 명단.xlsx에 저장한다.
    pub fn save_roster(self: &Arc<Self>, males: Vec<String>, females: Vec<String>) -> Result<ClassView, String> {
        let clean = |v: Vec<String>| -> Vec<String> {
            v.into_iter().map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).collect()
        };
        let (males, females) = (clean(males), clean(females));
        let mut seen = std::collections::HashSet::new();
        for n in males.iter().chain(females.iter()) {
            if !seen.insert(n.as_str()) {
                return Err(format!(
                    "'{n}' 이름이 두 번 있어요. 반에서 쓰는 표기(예: {n} A, {n} B)로 구분해 주세요."
                ));
            }
        }
        let name = {
            let inner = self.inner.lock().unwrap();
            inner.class.as_ref().ok_or("반을 먼저 골라 주세요")?.name.clone()
        };
        roster::save_roster(&self.paths.roster(&name), &males, &females).map_err(|_| {
            "명단을 저장할 수 없어요. 명단.xlsx가 엑셀에서 열려 있다면 닫고 다시 시도해 주세요.".to_string()
        })?;
        self.refresh()
    }

    pub fn set_classroom(&self, cfg: ClassroomConfig) -> Result<Layout, String> {
        let mut inner = self.inner.lock().unwrap();
        let s = inner.class.as_mut().ok_or("반을 먼저 골라 주세요")?;
        s.state.classroom = cfg.normalized();
        let layout = s.state.classroom.build();
        let _ = state::save(&self.paths.state_file(&s.name), &s.state);
        Ok(layout)
    }

    /// 학생 요청 저장(메모리). 바뀐 요청만 다시 해석한다.
    pub fn save_requests(self: &Arc<Self>, map: BTreeMap<String, String>) -> Result<(), String> {
        {
            let mut inner = self.inner.lock().unwrap();
            let class = inner.class.as_ref().ok_or("반을 먼저 골라 주세요")?.name.clone();
            let reqs = inner.requests.entry(class).or_default();
            for (name, text) in map {
                let text = text.trim().to_string();
                let e = reqs.entry(name).or_default();
                if e.text != text {
                    e.text = text;
                    e.rules = None;
                }
            }
            reqs.retain(|_, r| !r.text.is_empty());
        }
        self.kick();
        Ok(())
    }

    fn pending_count(&self, inner: &Inner) -> usize {
        let Some(s) = inner.class.as_ref() else { return 0 };
        let t = s.teacher.conditions.iter().filter(|l| !s.state.teacher_cache.entries.contains_key(*l)).count();
        let r = inner
            .requests
            .get(&s.name)
            .map(|m| m.iter().filter(|(n, r)| r.rules.is_none() && s.students.iter().any(|st| &st.name == *n)).count())
            .unwrap_or(0);
        t + r
    }

    pub fn status(&self) -> Status {
        let inner = self.inner.lock().unwrap();
        let engine = self.engine();
        let pending = match engine {
            Engine::Loading | Engine::Ready(_) => self.pending_count(&inner),
            _ => 0,
        };
        Status { model: engine.code(), pending }
    }

    /// 해석이 남아 있으면 백그라운드 작업을 시작한다.
    pub fn kick(self: &Arc<Self>) {
        let Engine::Ready(engine) = self.engine() else { return };
        {
            let mut inner = self.inner.lock().unwrap();
            if inner.worker_running || self.pending_count(&inner) == 0 {
                return;
            }
            inner.worker_running = true;
        }
        let me = Arc::clone(self);
        std::thread::spawn(move || me.work(engine));
    }

    fn work(self: Arc<Self>, engine: Arc<dyn LlmEngine>) {
        enum Job {
            Teacher(String),
            Request(String, String),
        }
        let mut interp_cache: Option<(String, Interpreter)> = None;
        loop {
            // 다음 할 일 고르기 (교사 조건 먼저)
            let (class, roster_names, roster_hash, job) = {
                let mut inner = self.inner.lock().unwrap();
                let next = inner.class.as_ref().and_then(|s| {
                    let names: Vec<String> = s.students.iter().map(|x| x.name.clone()).collect();
                    if let Some(l) = s.teacher.conditions.iter().find(|l| !s.state.teacher_cache.entries.contains_key(*l)) {
                        return Some((s.name.clone(), names, s.roster_hash.clone(), Job::Teacher(l.clone())));
                    }
                    let reqs = inner.requests.get(&s.name)?;
                    let (who, r) = reqs
                        .iter()
                        .find(|(n, r)| r.rules.is_none() && s.students.iter().any(|st| &st.name == *n))?;
                    Some((s.name.clone(), names, s.roster_hash.clone(), Job::Request(who.clone(), r.text.clone())))
                });
                match next {
                    Some(n) => n,
                    None => {
                        inner.worker_running = false;
                        return;
                    }
                }
            };
            let cache_id = format!("{class}|{roster_hash}");
            if interp_cache.as_ref().map(|(k, _)| k != &cache_id).unwrap_or(true) {
                interp_cache = Some((cache_id.clone(), Interpreter::new(&roster_names)));
            }
            let interp = &interp_cache.as_ref().unwrap().1;
            let result = match &job {
                Job::Teacher(line) => interp.interpret(engine.as_ref(), line, None),
                Job::Request(who, text) => interp.interpret(engine.as_ref(), text, Some(who)),
            };
            let rules = match result {
                Ok(r) => r,
                Err(_) => {
                    // 모델 실행이 실패함: "조건 없음"으로 저장하지 않고 모델을 쓸 수 없는 상태로 둔다
                    *self.engine.write().unwrap() = Engine::Failed;
                    self.inner.lock().unwrap().worker_running = false;
                    return;
                }
            };
            // 결과 저장 (그동안 반이나 문장이 바뀌었으면 버린다)
            let mut inner = self.inner.lock().unwrap();
            let Some(s) = inner.class.as_mut() else { continue };
            if s.name != class || s.roster_hash != roster_hash {
                continue;
            }
            match job {
                Job::Teacher(line) => {
                    if s.teacher.conditions.contains(&line) {
                        s.state.teacher_cache.entries.insert(line, rules);
                        let _ = state::save(&self.paths.state_file(&s.name), &s.state);
                    }
                }
                Job::Request(who, text) => {
                    if let Some(r) = inner.requests.get_mut(&class).and_then(|m| m.get_mut(&who)) {
                        if r.text == text {
                            r.rules = Some(rules);
                        }
                    }
                }
            }
        }
    }

    /// 자리 배치. 해석이 남아 있으면 `ERR_WAIT`.
    pub fn arrange(self: &Arc<Self>) -> Result<ArrangeView, String> {
        let _ = self.refresh()?;
        let engine = self.engine();
        let (class_name, classroom, layout, students, teacher_rules, student_rules, prev, zones) = {
            let inner = self.inner.lock().unwrap();
            let s = inner.class.as_ref().ok_or("반을 먼저 골라 주세요")?;
            if let Some(e) = &s.roster_error {
                return Err(e.clone());
            }
            if s.students.is_empty() {
                return Err("명단.xlsx에 학생 이름을 적어 주세요".into());
            }
            if s.confirmed.is_some() {
                return Err("이미 확정된 자리예요".into());
            }
            let ready = matches!(engine, Engine::Loading | Engine::Ready(_));
            let mut teacher_rules = Vec::new();
            for line in &s.teacher.conditions {
                match s.state.teacher_cache.entries.get(line) {
                    Some(r) => teacher_rules.extend(r.iter().cloned()),
                    None if ready => return Err(ERR_WAIT.into()),
                    // 모델을 쓸 수 없어 교사 조건을 읽지 못함 → 조건을 어긴 배치를 내지 않는다
                    None => return Err(model_error(&engine)),
                }
            }
            let mut student_rules = Vec::new();
            if let Some(reqs) = inner.requests.get(&s.name) {
                for (who, r) in reqs {
                    match &r.rules {
                        Some(rules) => student_rules.push((who.clone(), rules.clone())),
                        None if ready && s.students.iter().any(|st| &st.name == who) => return Err(ERR_WAIT.into()),
                        None => {}
                    }
                }
            }
            let prev = s.state.previous.as_ref().map(|p| p.pairs.clone()).unwrap_or_default();
            (
                s.name.clone(),
                s.state.classroom.clone(),
                s.state.classroom.build(),
                s.students.clone(),
                teacher_rules,
                student_rules,
                prev,
                s.teacher.zones,
            )
        };
        let mut problem = Problem {
            layout: Some(&layout),
            students: &students,
            teacher_rules: &teacher_rules,
            student_rules: &student_rules,
            previous_pairs: &prev,
            zones,
            relax_teacher: false,
        };
        let mut rng = StdRng::from_entropy();
        // 교사 조건을 모두 지킬 수 없으면 거부하지 않고, 지킬 수 없는 조건을 무시해 배치한다 (사용자 결정)
        let result = match solve(&problem, &mut rng) {
            Err(SolveError::Infeasible) => {
                problem.relax_teacher = true;
                solve(&problem, &mut rng)
            }
            r => r,
        };
        let arr = result.map_err(|e| match e {
            SolveError::NotEnoughSeats(n) => format!("자리가 {n}개 부족해요"),
            SolveError::Infeasible => ERR_IMPOSSIBLE.into(),
        })?;
        let names = names_by_seat(&layout, &students, &arr);
        let mut inner = self.inner.lock().unwrap();
        // 배치하는 동안 다른 반으로 바뀌었으면 결과를 버린다
        match inner.class.as_mut() {
            Some(s) if s.name == class_name => {
                s.unconfirmed = Some((classroom, layout.clone(), names.clone()));
                Ok(ArrangeView { layout, names })
            }
            _ => Err("반이 바뀌었어요. 다시 배치해 주세요.".into()),
        }
    }

    /// 확정 + Word 저장. `names`는 편집 모드에서 바꾼 최종 좌석.
    pub fn confirm(&self, names: Vec<Option<String>>, date: &str, file_date: &str) -> Result<String, String> {
        let mut inner = self.inner.lock().unwrap();
        let s = inner.class.as_mut().ok_or("반을 먼저 골라 주세요")?;
        let (classroom, layout, original) = s.unconfirmed.clone().ok_or("먼저 자리를 배치해 주세요")?;
        // 편집은 자리 맞바꾸기뿐이므로 이름 집합이 같아야 한다
        let mut a: Vec<_> = original.iter().flatten().collect();
        let mut b: Vec<_> = names.iter().flatten().collect();
        a.sort();
        b.sort();
        if a != b || names.len() != layout.seats.len() {
            return Err("좌석표가 올바르지 않아요".into());
        }
        let seat_of: Vec<usize> = s
            .students
            .iter()
            .filter_map(|st| names.iter().position(|n| n.as_deref() == Some(st.name.as_str())))
            .collect();
        let students: Vec<Student> =
            s.students.iter().filter(|st| names.iter().any(|n| n.as_deref() == Some(st.name.as_str()))).cloned().collect();
        let pairs = seat_core::pairs_of(&layout, &students, &Arrangement { seat_of });
        // Word를 먼저 저장한다. 실패하면 확정하지 않는다(다시 시도할 수 있게).
        let path = new_docx_path(&self.paths.class_dir(&s.name), file_date);
        docx::write_seating(&path, &s.name, date, &layout, &names).map_err(|_| ERR_DOCX.to_string())?;
        let file = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
        s.state.previous = Some(ConfirmedSeating {
            date: date.to_string(),
            file: Some(file.clone()),
            classroom,
            seats: names
                .iter()
                .enumerate()
                .filter_map(|(i, n)| {
                    n.as_ref().map(|n| PlacedName { name: n.clone(), row: layout.seats[i].row, col: layout.seats[i].col })
                })
                .collect(),
            pairs,
        });
        let _ = state::save(&self.paths.state_file(&s.name), &s.state);
        s.unconfirmed = None;
        s.confirmed = Some((layout, names, Some(path)));
        Ok(format!("{}/{}", s.name, file))
    }

    /// 확정한 좌석표를 Word로 (다시) 저장한다.
    pub fn save_docx(&self, date: &str, file_date: &str) -> Result<String, String> {
        let mut inner = self.inner.lock().unwrap();
        let s = inner.class.as_mut().ok_or("반을 먼저 골라 주세요")?;
        let dir = self.paths.class_dir(&s.name);
        let class_name = s.name.clone();
        let (layout, names, saved) = s.confirmed.as_mut().ok_or("먼저 자리를 확정해 주세요")?;
        let path = match saved {
            Some(p) => p.clone(),
            None => new_docx_path(&dir, file_date),
        };
        docx::write_seating(&path, &class_name, date, layout, names).map_err(|_| ERR_DOCX.to_string())?;
        *saved = Some(path.clone());
        Ok(format!("{}/{}", class_name, path.file_name().and_then(|n| n.to_str()).unwrap_or("")))
    }

    pub fn class_dir(&self) -> Option<PathBuf> {
        let inner = self.inner.lock().unwrap();
        inner.class.as_ref().map(|s| self.paths.class_dir(&s.name))
    }

    /// 확정된 좌석 (화면을 다시 열었을 때 보여 주기)
    /// 직전에 확정한 배치를 그때의 교실 모양 그대로 보여 준다.
    pub fn previous_view(&self) -> Option<PreviousView> {
        let inner = self.inner.lock().unwrap();
        let p = inner.class.as_ref()?.state.previous.as_ref()?;
        let layout = p.classroom.build();
        let mut names = vec![None; layout.seats.len()];
        for placed in &p.seats {
            if let Some(i) = layout.seat_index(placed.row, placed.col) {
                names[i] = Some(placed.name.clone());
            }
        }
        Some(PreviousView { date: p.date.clone(), layout, names })
    }

    /// 직전에 확정한 배치를 Word로 다시 저장한다 (파일 이름은 확정한 날짜).
    pub fn save_previous_docx(&self) -> Result<String, String> {
        let v = self.previous_view().ok_or("저장된 배치가 없어요")?;
        let (class_name, saved_file) = {
            let inner = self.inner.lock().unwrap();
            let s = inner.class.as_ref().ok_or("반을 먼저 골라 주세요")?;
            (s.name.clone(), s.state.previous.as_ref().and_then(|p| p.file.clone()))
        };
        let dir = self.paths.class_dir(&class_name);
        // 확정할 때 저장한 파일에 다시 쓴다 (같은 날 확정한 다른 배치를 덮어쓰지 않게)
        let path = match saved_file {
            Some(f) => dir.join(f),
            None => {
                let digits: Vec<String> = v
                    .date
                    .split(|c: char| !c.is_ascii_digit())
                    .filter(|x| !x.is_empty())
                    .map(|x| if x.len() == 1 { format!("0{x}") } else { x.to_string() })
                    .collect();
                new_docx_path(&dir, &digits.join("-"))
            }
        };
        docx::write_seating(&path, &class_name, &v.date, &v.layout, &v.names).map_err(|_| ERR_DOCX.to_string())?;
        Ok(format!("{}/{}", class_name, path.file_name().and_then(|n| n.to_str()).unwrap_or("")))
    }

    pub fn confirmed_view(&self) -> Option<ArrangeView> {
        let inner = self.inner.lock().unwrap();
        let (layout, names, _) = inner.class.as_ref()?.confirmed.as_ref()?;
        Some(ArrangeView { layout: layout.clone(), names: names.clone() })
    }

    /// 반을 다시 고르면 확정 잠금을 풀고 새로 시작한다.
    pub fn leave_class(&self) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(s) = inner.class.as_mut() {
            s.confirmed = None;
        }
    }

    pub fn unsaved(&self) -> Unsaved {
        let inner = self.inner.lock().unwrap();
        Unsaved {
            requests: inner.requests.values().any(|m| m.values().any(|r| !r.text.is_empty())),
            unconfirmed: inner.class.as_ref().map(|s| s.unconfirmed.is_some()).unwrap_or(false),
        }
    }
}

const ERR_DOCX: &str = "Word 파일을 저장할 수 없어요. 같은 이름의 파일이 Word에서 열려 있다면 닫고 다시 시도해 주세요.";

/// 반 폴더에 겹치지 않는 Word 파일 경로: 자리배치_날짜.docx, 자리배치_날짜_2.docx …
fn new_docx_path(dir: &std::path::Path, file_date: &str) -> PathBuf {
    let mut p = dir.join(format!("자리배치_{file_date}.docx"));
    let mut k = 2;
    while p.exists() {
        p = dir.join(format!("자리배치_{file_date}_{k}.docx"));
        k += 1;
    }
    p
}

fn model_error(engine: &Engine) -> String {
    match engine {
        Engine::Failed => "조건을 읽다가 문제가 생겼어요. 프로그램을 다시 켜 주세요.".into(),
        Engine::Unsupported => "이 컴퓨터에서는 조건을 해석할 수 없어요.".into(),
        Engine::Missing => "조건을 해석할 모델 파일이 없어요. 프로그램을 다시 내려받아 주세요.".into(),
        _ => ERR_IMPOSSIBLE.into(),
    }
}

fn empty_session() -> ClassSession {
    ClassSession {
        name: String::new(),
        students: vec![],
        roster_error: None,
        roster_hash: String::new(),
        state: ClassState::default(),
        teacher: TeacherSettings::default(),
        unconfirmed: None,
        confirmed: None,
    }
}

fn names_by_seat(layout: &Layout, students: &[Student], arr: &Arrangement) -> Vec<Option<String>> {
    let mut names = vec![None; layout.seats.len()];
    for (i, &s) in arr.seat_of.iter().enumerate() {
        names[s] = Some(students[i].name.clone());
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;
    use seat_interpret::ChatMessage;

    /// 조건 문장을 보고 정해진 답을 주는 가짜 모델
    struct Fake;
    impl LlmEngine for Fake {
        fn complete(&self, m: &[ChatMessage], _g: &str, _n: usize) -> Result<String, String> {
            let last = &m.last().unwrap().content;
            Ok(if last.contains("멀리") {
                r#"{"rules":[{"type":"distance","a":"김철수","b":"이영희","rel":"멀리"}]}"#.into()
            } else if last.contains("1줄 1열") {
                r#"{"rules":[{"type":"place","who":"박민수","where":{"seat":[1,1]}}]}"#.into()
            } else if last.contains("창가") {
                r#"{"rules":[{"type":"place","who":"정수아","where":"창가"}]}"#.into()
            } else {
                r#"{"rules":[]}"#.into()
            })
        }
    }

    fn setup() -> (tempfile::TempDir, Arc<Core>) {
        let t = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(t.path().join("자리배치"));
        let core = Core::new(paths.clone());
        core.add_class("2-3").unwrap();
        roster::write_roster(&paths.roster("2-3"), &["김철수", "박민수", "최서준"], &["이영희", "정수아", "한지민"]).unwrap();
        std::fs::write(paths.settings("2-3"), "[General]\r\n철수랑 영희 멀리\r\n민수는 1줄 1열\r\n").unwrap();
        (t, core)
    }

    fn wait(core: &Arc<Core>) {
        for _ in 0..200 {
            if core.status().pending == 0 {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("해석이 끝나지 않음");
    }

    #[test]
    fn full_flow() {
        let (_t, core) = setup();
        let v = core.open_class("2-3").unwrap();
        assert_eq!(v.students.len(), 6);
        // 모델 로딩 중이면 기다리라고 한다
        assert_eq!(core.arrange().err().as_deref(), Some(ERR_WAIT));
        core.set_engine(Engine::Ready(Arc::new(Fake)));
        let mut req = BTreeMap::new();
        req.insert("정수아".to_string(), "창가 자리".to_string());
        core.save_requests(req).unwrap();
        wait(&core);
        core.set_classroom(ClassroomConfig { rows: 2, cols: 4, ..Default::default() }).unwrap();
        for _ in 0..30 {
            let a = core.arrange().unwrap();
            let pos = |n: &str| a.names.iter().position(|x| x.as_deref() == Some(n)).unwrap();
            assert!(a.layout.distance(pos("김철수"), pos("이영희")) >= 2);
            let m = a.layout.seats[pos("박민수")];
            assert_eq!((m.row, m.col), (1, 1));
        }
        assert!(core.unsaved().requests && core.unsaved().unconfirmed);
        let a = core.arrange().unwrap();
        let saved = core.confirm(a.names.clone(), "2026. 11. 20.", "2026-11-20").unwrap();
        assert_eq!(saved, "2-3/자리배치_2026-11-20.docx");
        assert!(!core.unsaved().unconfirmed);
        assert!(core.arrange().is_err(), "확정 후에는 다시 배치 불가");
        // 다시 저장해도 같은 파일
        assert_eq!(core.save_docx("2026. 11. 20.", "2026-11-20").unwrap(), saved);
        // 직전 짝이 저장됨 + 캐시가 파일에 남음
        let st: ClassState = state::load(&core.paths.state_file("2-3"));
        assert!(st.previous.is_some());
        // 현재 배치 보기: 확정한 좌석 그대로
        let prev = core.previous_view().unwrap();
        assert_eq!(prev.names, a.names);
        assert_eq!(core.class_view().unwrap().previous_date.as_deref(), Some("2026. 11. 20."));
        assert_eq!(core.save_previous_docx().unwrap(), "2-3/자리배치_2026-11-20.docx");
        assert_eq!(st.teacher_cache.entries.len(), 2);
    }

    struct Broken;
    impl LlmEngine for Broken {
        fn complete(&self, _m: &[ChatMessage], _g: &str, _n: usize) -> Result<String, String> {
            Err("decode failed".into())
        }
    }

    #[test]
    fn engine_failure_is_not_cached() {
        let (_t, core) = setup();
        core.open_class("2-3").unwrap();
        core.set_engine(Engine::Ready(Arc::new(Broken)));
        for _ in 0..200 {
            if core.status().model == "failed" {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(core.status().model, "failed");
        let st: ClassState = state::load(&core.paths.state_file("2-3"));
        assert!(st.teacher_cache.entries.is_empty(), "실패를 '조건 없음'으로 저장하면 안 됨");
        let e = core.arrange().unwrap_err();
        assert!(e.contains("다시 켜"), "{e}");
    }

    #[test]
    fn roster_error_keeps_teacher_cache() {
        let (_t, core) = setup();
        core.open_class("2-3").unwrap();
        core.set_engine(Engine::Ready(Arc::new(Fake)));
        wait(&core);
        // 명단을 못 읽는 상태(엑셀이 잠근 상황 흉내)
        std::fs::write(core.paths.roster("2-3"), b"not a zip").unwrap();
        let v = core.refresh().unwrap();
        assert!(v.roster_error.is_some());
        let st: ClassState = state::load(&core.paths.state_file("2-3"));
        assert_eq!(st.teacher_cache.entries.len(), 2);
    }

    #[test]
    fn confirm_keeps_arrange_time_classroom_and_file() {
        let (_t, core) = setup();
        core.open_class("2-3").unwrap();
        core.set_engine(Engine::Ready(Arc::new(Fake)));
        wait(&core);
        core.set_classroom(ClassroomConfig { rows: 2, cols: 4, ..Default::default() }).unwrap();
        let a = core.arrange().unwrap();
        // 배치한 뒤 교실 모양을 바꾸고 확정해도, 배치 당시 모양으로 저장되어야 한다
        core.set_classroom(ClassroomConfig { rows: 1, cols: 2, ..Default::default() }).unwrap();
        let f1 = core.confirm(a.names.clone(), "2026. 11. 20.", "2026-11-20").unwrap();
        let prev = core.previous_view().unwrap();
        assert_eq!(prev.names, a.names);
        // 같은 날 두 번째 확정 → _2 파일, 현재 배치 다시 저장도 _2에
        core.open_class("2-3").unwrap();
        core.set_classroom(ClassroomConfig { rows: 2, cols: 4, ..Default::default() }).unwrap();
        let b = core.arrange().unwrap();
        let f2 = core.confirm(b.names.clone(), "2026. 11. 20.", "2026-11-20").unwrap();
        assert_ne!(f1, f2);
        assert_eq!(core.save_previous_docx().unwrap(), f2);
    }

    #[test]
    fn roster_edit_in_app() {
        let (_t, core) = setup();
        core.open_class("2-3").unwrap();
        let v = core.save_roster(vec!["김철수".into(), " ".into(), "박민수".into()], vec!["이영희".into()]).unwrap();
        assert_eq!(v.students.len(), 3);
        assert!(core.save_roster(vec!["김철수".into()], vec!["김철수".into()]).is_err());
        assert_eq!(roster::read_roster(&core.paths.roster("2-3")).unwrap().len(), 3);
    }

    #[test]
    fn missing_model_blocks_only_when_teacher_rules_uncached() {
        let (_t, core) = setup();
        core.open_class("2-3").unwrap();
        core.set_engine(Engine::Missing);
        assert!(core.arrange().unwrap_err().contains("모델"));
        std::fs::write(core.paths.settings("2-3"), "[General]\r\n").unwrap();
        assert!(core.arrange().is_ok());
    }

    #[test]
    fn errors_never_reveal_conditions() {
        let (_t, core) = setup();
        std::fs::write(core.paths.settings("2-3"), "철수랑 영희 멀리\r\n민수는 1줄 1열\r\n민수는 1줄 1열 말고 창가\r\n").unwrap();
        core.open_class("2-3").unwrap();
        core.set_engine(Engine::Ready(Arc::new(Fake)));
        wait(&core);
        core.set_classroom(ClassroomConfig { rows: 1, cols: 6, ..Default::default() }).unwrap();
        // 김철수·이영희 멀리(2칸 이상)는 1줄 6자리에서도 가능, 지금 조건은 가능해야 함
        let _ = core.arrange();
        core.set_classroom(ClassroomConfig { rows: 1, cols: 2, ..Default::default() }).unwrap();
        let e = core.arrange().unwrap_err();
        assert!(!e.contains("철수") && !e.contains("민수") && !e.contains("영희"));
    }
}
