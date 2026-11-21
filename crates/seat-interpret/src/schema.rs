//! LLM이 내놓는 JSON 형식과, 그것을 핵심 규칙([`seat_core::Rule`])으로 바꾸는 부분.
//!
//! 작은 모델이 틀리기 어렵도록 단순한 낱말 위주로 정했다.
//!
//! ```json
//! {"rules":[
//!   {"type":"distance","a":"김철수","b":"이영희","rel":"멀리"},
//!   {"type":"place","who":"박민수","where":"앞쪽"},
//!   {"type":"place","who":"한도윤","where":{"not":"뒷문근처"}},
//!   {"type":"place","who":"최서준","where":{"seat":[1,3]}}
//! ]}
//! ```

use seat_core::{Facility, Place, Rule, Zone};
use serde::Deserialize;
use serde_json::Value;

/// 장소 낱말 → 자리 집합
pub const ZONE_WORDS: &[&str] =
    &["앞쪽", "뒤쪽", "맨앞", "맨뒤", "창가", "복도", "가운데", "교탁근처", "칠판근처", "앞문근처", "뒷문근처"];

/// 거리 낱말 → (최소, 최대)
pub const REL_WORDS: &[&str] = &["짝", "근처", "멀리", "짝아님"];

fn zone_word(w: &str) -> Option<Place> {
    Some(match w {
        "앞쪽" => Place::Zone(Zone::Front),
        "뒤쪽" => Place::Zone(Zone::Back),
        "맨앞" => Place::Zone(Zone::FrontMost),
        "맨뒤" => Place::Zone(Zone::BackMost),
        "창가" => Place::Zone(Zone::Window),
        "복도" => Place::Zone(Zone::Hallway),
        "가운데" => Place::Zone(Zone::Center),
        "교탁근처" => Place::Near(Facility::Podium),
        "칠판근처" => Place::Near(Facility::Board),
        "앞문근처" => Place::Near(Facility::FrontDoor),
        "뒷문근처" => Place::Near(Facility::BackDoor),
        _ => return None,
    })
}

pub fn rel_range(rel: &str) -> Option<(u32, Option<u32>)> {
    Some(match rel {
        "짝" => (0, Some(0)),
        "근처" => (0, Some(1)),
        "멀리" => (2, None),
        "짝아님" => (1, None),
        _ => return None,
    })
}

fn pair(v: &Value) -> Option<(u32, u32)> {
    let a = v.as_array()?;
    if a.len() != 2 {
        return None;
    }
    let x = u32::try_from(a[0].as_u64()?).ok()?;
    let y = u32::try_from(a[1].as_u64()?).ok()?;
    (x >= 1 && y >= 1 && x <= 99 && y <= 99).then_some((x, y))
}

/// `all` 안에 서로 반대인 구역(앞쪽·맨앞 ↔ 뒤쪽·맨뒤, 창가 ↔ 복도)이 함께 있는지
fn contradicts(items: &[Place]) -> bool {
    let has = |zs: &[Zone]| items.iter().any(|p| matches!(p, Place::Zone(z) if zs.contains(z)));
    (has(&[Zone::Front, Zone::FrontMost]) && has(&[Zone::Back, Zone::BackMost]))
        || (has(&[Zone::Window]) && has(&[Zone::Hallway]))
}

fn place(v: &Value, depth: u32) -> Option<Place> {
    if depth > 6 {
        return None;
    }
    if let Some(s) = v.as_str() {
        return zone_word(s);
    }
    let o = v.as_object()?;
    if o.len() != 1 {
        return None;
    }
    let (k, v) = o.iter().next()?;
    Some(match k.as_str() {
        "rows" => {
            let (a, b) = pair(v)?;
            Place::Rows(a, b)
        }
        "cols" => {
            let (a, b) = pair(v)?;
            Place::Cols(a, b)
        }
        "seat" => {
            let (a, b) = pair(v)?;
            Place::Seat(a, b)
        }
        "not" => Place::Not(Box::new(place(v, depth + 1)?)),
        "any" | "all" => {
            let items: Option<Vec<Place>> = v.as_array()?.iter().map(|x| place(x, depth + 1)).collect();
            let items = items?;
            if items.is_empty() {
                return None;
            }
            if items.len() == 1 {
                items.into_iter().next().unwrap()
            } else if k == "any" {
                Place::Any(items)
            } else if contradicts(&items) {
                // "앞이면서 뒤" 같은 조합은 모델의 오해다 → 잘못된 출력으로 보고 다시 생성
                return None;
            } else {
                Place::All(items)
            }
        }
        _ => return None,
    })
}

#[derive(Deserialize)]
struct Root {
    rules: Vec<Value>,
}

/// LLM 출력 → 규칙. 형식이 틀리거나 허용되지 않은 이름이 있으면 Err.
pub fn parse_output(text: &str, allowed_names: &[String]) -> Result<Vec<Rule>, String> {
    let root: Root = serde_json::from_str(text.trim()).map_err(|e| format!("json: {e}"))?;
    let name = |v: Option<&Value>| -> Result<String, String> {
        let s = v.and_then(Value::as_str).ok_or("이름 없음")?;
        if allowed_names.iter().any(|n| n == s) {
            Ok(s.to_string())
        } else {
            Err(format!("명단에 없는 이름: {s}"))
        }
    };
    let mut out = Vec::new();
    for r in root.rules {
        match r.get("type").and_then(Value::as_str) {
            Some("place") => {
                let who = name(r.get("who"))?;
                let p = r.get("where").and_then(|w| place(w, 0)).ok_or("장소 형식 오류")?;
                out.push(Rule::Allowed { student: who, place: p });
            }
            Some("distance") => {
                let a = name(r.get("a"))?;
                let b = name(r.get("b"))?;
                if a == b {
                    continue;
                }
                let (min, max) =
                    r.get("rel").and_then(Value::as_str).and_then(rel_range).ok_or("거리 형식 오류")?;
                out.push(Rule::Distance { a, b, min, max });
            }
            _ => return Err("규칙 종류 오류".into()),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_examples() {
        let names = vec!["김철수".to_string(), "이영희".to_string()];
        let r = parse_output(
            r#"{"rules":[{"type":"distance","a":"김철수","b":"이영희","rel":"멀리"},{"type":"place","who":"김철수","where":{"not":{"any":["뒷문근처",{"seat":[1,3]}]}}}]}"#,
            &names,
        )
        .unwrap();
        assert_eq!(r[0], Rule::Distance { a: "김철수".into(), b: "이영희".into(), min: 2, max: None });
        assert_eq!(
            r[1],
            Rule::Allowed {
                student: "김철수".into(),
                place: Place::Not(Box::new(Place::Any(vec![Place::Near(Facility::BackDoor), Place::Seat(1, 3)])))
            }
        );
        assert!(parse_output(r#"{"rules":[{"type":"place","who":"홍길동","where":"앞쪽"}]}"#, &names).is_err());
        assert!(parse_output(r#"{"rules":[{"type":"place","who":"김철수","where":"하늘"}]}"#, &names).is_err());
        assert!(parse_output("garbage", &names).is_err());
        assert_eq!(parse_output(r#"{"rules":[]}"#, &names).unwrap(), vec![]);
    }

    #[test]
    fn contradictory_all_rejected() {
        let names = vec!["김철수".to_string()];
        let out = |w: &str| format!(r#"{{"rules":[{{"type":"place","who":"김철수","where":{w}}}]}}"#);
        assert!(parse_output(&out(r#"{"all":["맨뒤","맨앞"]}"#), &names).is_err());
        assert!(parse_output(&out(r#"{"all":["창가","복도"]}"#), &names).is_err());
        assert!(parse_output(&out(r#"{"all":["창가","맨뒤"]}"#), &names).is_ok());
        assert!(parse_output(&out(r#"{"any":["맨앞","맨뒤"]}"#), &names).is_ok());
    }
}
