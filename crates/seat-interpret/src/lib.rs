//! 자연어 조건 → 규칙.
//!
//! 1. 문장에서 명단 이름을 찾아 명단 표기로 바꾼다(조사·성 생략·동명이인 처리). 모호하면 건너뛴다.
//! 2. 고정 프롬프트 + 예시 + 문장을 모델에 주고, 문법 제약으로 JSON만 나오게 한다.
//! 3. 결과를 검증한다. 틀리면 한 번 다시 생성하고, 그래도 틀리면 그 조건을 조용히 건너뛴다.

pub mod cues;
pub mod grammar;
pub mod names;
pub mod prompt;
pub mod schema;

use cues::Cues;
use names::{canonicalize, NameMatcher};
use seat_core::Rule;

/// 해석 방식이 바뀌면 올린다 → 저장된 해석 캐시가 자동으로 버려진다.
pub const INTERPRETER_VERSION: &str = "3";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn new(role: &str, content: &str) -> Self {
        Self { role: role.to_string(), content: content.to_string() }
    }
}

/// 모델 실행기. 내장 llama.cpp 외에 테스트용 가짜 엔진도 이 trait을 구현한다.
pub trait LlmEngine: Send + Sync {
    /// `messages`를 채팅 형식으로 넣고, `grammar`(GBNF)를 지키는 답을 만든다.
    fn complete(&self, messages: &[ChatMessage], grammar: &str, max_tokens: usize) -> Result<String, String>;
}

pub struct Interpreter {
    matcher: NameMatcher,
}

/// 규칙 하나는 약 30~40토큰. 이름이 많으면 두 사람 쌍마다 규칙이 생기므로 넉넉히 잡는다.
fn max_tokens(names: usize) -> usize {
    let pairs = names * names.saturating_sub(1) / 2;
    (60 + 45 * (pairs + names)).min(2000)
}

/// 모델 실행 자체가 실패함 (출력이 틀린 것과 다르다). 이때는 결과를 캐시하면 안 된다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineError(pub String);

impl Interpreter {
    pub fn new(roster: &[String]) -> Self {
        Self { matcher: NameMatcher::new(roster.iter().map(String::as_str)) }
    }

    /// 모델에 보낼 준비: (다시 쓴 문장, 쓸 수 있는 이름, 단서). 해석할 필요가 없으면 None.
    pub fn prepare(&self, sentence: &str, requester: Option<&str>) -> Option<(String, Vec<String>, Cues)> {
        let sentence = sentence.trim();
        if sentence.is_empty() {
            return None;
        }
        // 이름을 먼저 찾는다 (한글 수 바꾸기가 "한열" 같은 이름을 "1열"로 바꾸지 않도록)
        let mentions = self.matcher.find(sentence);
        let (_, mut names) = canonicalize(sentence, &mentions)?;
        // 이름 사이의 글만 한글 수를 숫자로 바꾼다
        let mut canonical = String::new();
        let mut stripped = String::new();
        let mut last = 0;
        for m in &mentions {
            let gap = cues::normalize_numbers(&sentence[last..m.start]);
            canonical.push_str(&gap);
            canonical.push_str(&m.names[0]);
            stripped.push_str(&gap);
            stripped.push(' ');
            last = m.end;
        }
        let tail = cues::normalize_numbers(&sentence[last..]);
        canonical.push_str(&tail);
        stripped.push_str(&tail);
        let cues = Cues::find(&stripped);
        if let Some(r) = requester {
            names.retain(|n| n != r);
            names.insert(0, r.to_string());
        }
        let a = grammar::allowed(&names, &cues);
        if names.is_empty() || !(a.place || a.distance) {
            return None;
        }
        Some((canonical, names, cues))
    }

    /// 문장 하나를 해석한다. 모델 출력이 두 번 다 틀리면 빈 목록(조용히 건너뜀).
    /// 모델 실행 자체가 두 번 다 실패하면 `EngineError` (결과를 캐시하지 말 것).
    ///
    /// `requester`가 있으면(학생 요청) 그 학생과 관계된 규칙만 남긴다.
    pub fn interpret(
        &self,
        engine: &dyn LlmEngine,
        sentence: &str,
        requester: Option<&str>,
    ) -> Result<Vec<Rule>, EngineError> {
        let Some((canonical, names, cues)) = self.prepare(sentence, requester) else {
            return Ok(vec![]);
        };
        let messages = prompt::messages(&canonical, requester);
        let grammar = grammar::build(&names, &cues);
        let mut engine_error = None;
        let mut got_output = false;
        for _attempt in 0..2 {
            let text = match engine.complete(&messages, &grammar, max_tokens(names.len())) {
                Ok(t) => t,
                Err(e) => {
                    engine_error = Some(e);
                    continue;
                }
            };
            got_output = true;
            if let Ok(rules) = schema::parse_output(&text, &names) {
                let mut out: Vec<Rule> = Vec::new();
                for rule in rules {
                    if requester.is_some_and(|r| !rule.names().contains(&r)) {
                        continue;
                    }
                    if !out.contains(&rule) {
                        out.push(rule);
                    }
                }
                return Ok(out);
            }
        }
        match engine_error {
            Some(e) if !got_output => Err(EngineError(e)),
            _ => Ok(vec![]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct Fake {
        replies: Mutex<Vec<String>>,
        seen: Mutex<Vec<String>>,
    }

    impl LlmEngine for Fake {
        fn complete(&self, m: &[ChatMessage], _g: &str, _n: usize) -> Result<String, String> {
            self.seen.lock().unwrap().push(m.last().unwrap().content.clone());
            self.replies.lock().unwrap().pop().ok_or("none".into())
        }
    }

    fn fake(replies: &[&str]) -> Fake {
        Fake { replies: Mutex::new(replies.iter().rev().map(|s| s.to_string()).collect()), seen: Mutex::new(vec![]) }
    }

    fn roster() -> Vec<String> {
        ["김철수", "이영희", "김민수 A", "김민수 B"].iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn canonical_sentence_sent_and_parsed() {
        let i = Interpreter::new(&roster());
        let f = fake(&[r#"{"rules":[{"type":"distance","a":"김철수","b":"이영희","rel":"멀리"}]}"#]);
        let r = i.interpret(&f, "철수랑 영희 멀리", None).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(f.seen.lock().unwrap()[0], "문장: 김철수랑 이영희 멀리");
    }

    #[test]
    fn retry_then_skip() {
        let i = Interpreter::new(&roster());
        let ok = r#"{"rules":[{"type":"place","who":"김철수","where":"앞쪽"}]}"#;
        assert_eq!(i.interpret(&fake(&["bad", ok]), "철수 앞", None).unwrap().len(), 1);
        assert!(i.interpret(&fake(&["bad", "bad", ok]), "철수 앞", None).unwrap().is_empty());
    }

    #[test]
    fn ambiguous_or_nameless_skipped_without_model() {
        let i = Interpreter::new(&roster());
        let f = fake(&[]);
        assert!(i.interpret(&f, "민수 앞", None).unwrap().is_empty());
        assert!(i.interpret(&f, "앞쪽에 앉히기", None).unwrap().is_empty());
        assert!(f.seen.lock().unwrap().is_empty());
    }

    #[test]
    fn engine_failure_is_not_cached_as_empty() {
        let i = Interpreter::new(&roster());
        assert!(i.interpret(&fake(&[]), "철수 앞", None).is_err());
    }

    #[test]
    fn names_are_not_turned_into_numbers() {
        // "한열"이 "1열"로 바뀌면 이름을 못 찾는다
        let r: Vec<String> = ["이한열", "김민수"].iter().map(|s| s.to_string()).collect();
        let i = Interpreter::new(&r);
        let (c, names, cues) = i.prepare("한열이랑 민수 멀리", None).unwrap();
        assert_eq!(c, "이한열이랑 김민수 멀리");
        assert_eq!(names.len(), 2);
        assert!(!cues.digits);
        let (c, _, _) = i.prepare("한열이는 앞에서 두 번째 줄", None).unwrap();
        assert_eq!(c, "이한열이는 앞에서 2 번째 줄");
    }

    #[test]
    fn token_budget_grows_with_names() {
        assert!(max_tokens(5) >= 10 * 30, "다섯 명 서로 떨어지게 = 규칙 10개");
    }

    #[test]
    fn student_request_only_about_requester() {
        let i = Interpreter::new(&roster());
        let f = fake(&[
            r#"{"rules":[{"type":"place","who":"이영희","where":"창가"},{"type":"distance","a":"김철수","b":"김민수 A","rel":"멀리"}]}"#,
        ]);
        let r = i.interpret(&f, "창가 자리, 그리고 철수랑 민수 A 멀리", Some("이영희")).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(f.seen.lock().unwrap()[0], "요청한 학생: 이영희\n문장: 창가 자리, 그리고 김철수랑 김민수 A 멀리");
    }
}
