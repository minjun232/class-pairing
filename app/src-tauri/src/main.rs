// 배포판에서는 콘솔 창을 띄우지 않는다.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod session;

use seat_core::ClassroomConfig;
use seat_storage::AppPaths;
use session::{ArrangeView, Boot, ClassView, Core, Engine, Status, Unsaved};
use std::collections::BTreeMap;
use std::sync::Arc;
use tauri::{Emitter, State};

type CoreState<'a> = State<'a, Arc<Core>>;

fn dates() -> (String, String) {
    let now = chrono::Local::now();
    (now.format("%Y. %m. %d.").to_string(), now.format("%Y-%m-%d").to_string())
}

#[tauri::command]
fn boot(core: CoreState) -> Result<Boot, String> {
    core.boot()
}

#[tauri::command]
fn add_class(core: CoreState, name: String) -> Result<Boot, String> {
    core.add_class(&name)
}

#[tauri::command]
fn open_class(core: CoreState, name: String) -> Result<ClassView, String> {
    core.leave_class();
    core.open_class(&name)
}

#[tauri::command]
fn refresh(core: CoreState) -> Result<ClassView, String> {
    core.refresh()
}

#[tauri::command]
fn save_roster(core: CoreState, males: Vec<String>, females: Vec<String>) -> Result<ClassView, String> {
    core.save_roster(males, females)
}

#[tauri::command]
fn set_classroom(core: CoreState, classroom: ClassroomConfig) -> Result<seat_core::Layout, String> {
    core.set_classroom(classroom)
}

#[tauri::command]
fn save_requests(core: CoreState, requests: BTreeMap<String, String>) -> Result<(), String> {
    core.save_requests(requests)
}

#[tauri::command]
fn status(core: CoreState) -> Status {
    core.status()
}

#[tauri::command]
async fn arrange(core: CoreState<'_>) -> Result<ArrangeView, String> {
    let core = Arc::clone(&core);
    tauri::async_runtime::spawn_blocking(move || core.arrange()).await.map_err(|e| e.to_string())?
}

#[tauri::command]
fn previous_view(core: CoreState) -> Option<session::PreviousView> {
    core.previous_view()
}

#[tauri::command]
fn save_previous_docx(core: CoreState) -> Result<String, String> {
    core.save_previous_docx()
}

#[tauri::command]
fn confirmed_view(core: CoreState) -> Option<ArrangeView> {
    core.confirmed_view()
}

#[tauri::command]
fn confirm(core: CoreState, names: Vec<Option<String>>) -> Result<String, String> {
    let (d, f) = dates();
    core.confirm(names, &d, &f)
}

#[tauri::command]
fn save_docx(core: CoreState) -> Result<String, String> {
    let (d, f) = dates();
    core.save_docx(&d, &f)
}

#[tauri::command]
fn open_folder(core: CoreState) {
    if let Some(dir) = core.class_dir() {
        #[cfg(windows)]
        let _ = std::process::Command::new("explorer").arg(dir).spawn();
        #[cfg(target_os = "macos")]
        let _ = std::process::Command::new("open").arg(dir).spawn();
        #[cfg(all(unix, not(target_os = "macos")))]
        let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
    }
}

#[tauri::command]
fn unsaved(core: CoreState) -> Unsaved {
    core.unsaved()
}

#[tauri::command]
fn quit(app: tauri::AppHandle) {
    app.exit(0);
}

fn load_engine(core: Arc<Core>) {
    std::thread::spawn(move || {
        #[cfg(feature = "llm")]
        {
            use seat_llm::{LlamaEngine, LoadError};
            let e = match LlamaEngine::load(&core.paths.model()) {
                Ok(engine) => Engine::Ready(Arc::new(engine)),
                Err(LoadError::MissingModel) => Engine::Missing,
                Err(LoadError::UnsupportedCpu) => Engine::Unsupported,
                Err(LoadError::Failed(_)) => Engine::Failed,
            };
            core.set_engine(e);
        }
        #[cfg(not(feature = "llm"))]
        core.set_engine(Engine::Missing);
    });
}

fn main() {
    let paths = AppPaths::from_exe().unwrap_or_else(|_| AppPaths::new("."));
    // 개발 중에는 JARIBAECHI_ROOT로 데이터 폴더를 바꿀 수 있다.
    let paths = std::env::var_os("JARIBAECHI_ROOT").map(AppPaths::new).unwrap_or(paths);

    // 오래된 PC 대비: system/webview2 폴더에 WebView2 고정 버전 런타임이 있으면 그것을 쓴다.
    let fixed = paths.system().join("webview2");
    if fixed.join("msedgewebview2.exe").exists() {
        std::env::set_var("WEBVIEW2_BROWSER_EXECUTABLE_FOLDER", &fixed);
    }

    let core = Core::new(paths);
    // 실행과 동시에 백그라운드에서 모델을 불러온다.
    load_engine(Arc::clone(&core));

    tauri::Builder::default()
        .manage(core)
        .invoke_handler(tauri::generate_handler![
            boot,
            add_class,
            open_class,
            refresh,
            save_roster,
            set_classroom,
            save_requests,
            status,
            arrange,
            confirmed_view,
            previous_view,
            save_previous_docx,
            confirm,
            save_docx,
            open_folder,
            unsaved,
            quit
        ])
        .on_window_event(|window, event| {
            // 창의 X 버튼도 화면의 [끝내기]와 같은 확인을 거친다
            // (화면에서 아직 저장하지 않은 명단·요청 입력은 화면만 안다).
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.emit("close-requested", ());
            }
        })
        .run(tauri::generate_context!())
        .expect("자리배치를 시작할 수 없어요");
}
