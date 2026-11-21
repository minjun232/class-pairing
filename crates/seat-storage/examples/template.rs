//! 배포용 system/template.xlsx 만들기: cargo run -p seat-storage --example template -- <경로>
fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| "template.xlsx".into());
    seat_storage::roster::write_template(std::path::Path::new(&path)).expect("템플릿 생성 실패");
    println!("{path}");
}
