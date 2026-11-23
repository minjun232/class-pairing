//! 모델에 주는 프롬프트. 기준값과 시설 위치는 매번 같은 글로 전달해 해석이 흔들리지 않게 한다.

use crate::ChatMessage;

pub const SYSTEM: &str = r#"너는 학교 자리 배치 조건 문장을 JSON 규칙으로 바꾼다. JSON만 출력한다.

교실 (학생이 칠판을 보는 방향 기준)
- 1줄이 맨 앞, 1열이 창문 쪽(왼쪽)이다.
- 칠판은 앞쪽 가운데, 교탁은 앞쪽 창문 쪽, 창문은 왼쪽 벽, 앞문과 뒷문은 오른쪽 벽에 있다.

규칙은 두 가지뿐이다.
1. 자리: {"type":"place","who":이름,"where":장소}
   장소 낱말: "앞쪽","뒤쪽","맨앞","맨뒤","창가","복도","가운데","교탁근처","칠판근처","앞문근처","뒷문근처"
   장소 식: {"rows":[시작줄,끝줄]}, {"cols":[시작열,끝열]}, {"seat":[줄,열]}, {"not":장소}, {"any":[장소,...]}, {"all":[장소,...]}
2. 거리: {"type":"distance","a":이름,"b":이름,"rel":관계}
   관계: "짝"(같이 붙어 앉음), "근처"(가까이), "멀리"(떨어뜨림), "짝아님"(짝만 아니면 됨)

지침
- 이름은 문장에 나온 표기 그대로 쓴다.
- "요청한 학생"이 있으면 '나', '저', 생략된 주어는 그 학생이다.
- 시력이 나쁨, 눈이 나쁨, 키가 작음, 칠판이 잘 안 보임 → "앞쪽"
- 키가 큼, 키가 커서 뒷사람이 안 보임 → "뒤쪽"
- 옆, 옆자리, 붙여서 → "짝" / 가까이, 근처 → "근처"
- 선생님 눈에 잘 띄는 자리, 선생님 가까이, 감시가 필요함 → "교탁근처"
- 싫다, 빼고, 말고, 아니면 좋겠다 → {"not":장소}
- 여러 명을 서로 떨어뜨리라고 하면 모든 두 사람 쌍마다 거리 규칙을 만든다.
- 자리 배치와 관계없는 문장이면 {"rules":[]}"#;

const EXAMPLES: &[(&str, &str)] = &[
    ("문장: 김철수랑 이영희 멀리 떨어뜨려 주세요", r#"{"rules":[{"type":"distance","a":"김철수","b":"이영희","rel":"멀리"}]}"#),
    ("문장: 박민수 시력 안 좋음", r#"{"rules":[{"type":"place","who":"박민수","where":"앞쪽"}]}"#),
    ("문장: 최서준은 1줄 3열", r#"{"rules":[{"type":"place","who":"최서준","where":{"seat":[1,3]}}]}"#),
    ("문장: 윤재민은 선생님 눈에 잘 띄는 자리로", r#"{"rules":[{"type":"place","who":"윤재민","where":"교탁근처"}]}"#),
    (
        "문장: 김철수, 이영희, 박민수 셋은 서로 떨어지게",
        r#"{"rules":[{"type":"distance","a":"김철수","b":"이영희","rel":"멀리"},{"type":"distance","a":"김철수","b":"박민수","rel":"멀리"},{"type":"distance","a":"이영희","b":"박민수","rel":"멀리"}]}"#,
    ),
    ("요청한 학생: 정수아\n문장: 강하윤이랑 짝 하고 싶어요", r#"{"rules":[{"type":"distance","a":"정수아","b":"강하윤","rel":"짝"}]}"#),
    ("요청한 학생: 한도윤\n문장: 뒷문 근처는 싫어요", r#"{"rules":[{"type":"place","who":"한도윤","where":{"not":"뒷문근처"}}]}"#),
    ("요청한 학생: 오지호\n문장: 창가 맨 뒤", r#"{"rules":[{"type":"place","who":"오지호","where":{"all":["창가","맨뒤"]}}]}"#),
    ("문장: 박민수는 복도 쪽 자리", r#"{"rules":[{"type":"place","who":"박민수","where":"복도"}]}"#),
    ("문장: 윤재민 맨 앞줄", r#"{"rules":[{"type":"place","who":"윤재민","where":"맨앞"}]}"#),
    ("문장: 최서준은 키가 커서 뒤로", r#"{"rules":[{"type":"place","who":"최서준","where":"뒤쪽"}]}"#),
    ("요청한 학생: 오지호\n문장: 키가 커요", r#"{"rules":[{"type":"place","who":"오지호","where":"뒤쪽"}]}"#),
    ("요청한 학생: 한도윤\n문장: 교탁 근처요", r#"{"rules":[{"type":"place","who":"한도윤","where":"교탁근처"}]}"#),
    ("문장: 윤재민은 맨뒷줄", r#"{"rules":[{"type":"place","who":"윤재민","where":"맨뒤"}]}"#),
    ("요청한 학생: 정수아\n문장: 가운데 자리요", r#"{"rules":[{"type":"place","who":"정수아","where":"가운데"}]}"#),
    ("문장: 이서연과 박하윤은 짝만 안 되게", r#"{"rules":[{"type":"distance","a":"이서연","b":"박하윤","rel":"짝아님"}]}"#),
    ("요청한 학생: 정수아\n문장: 아무 데나 괜찮아요", r#"{"rules":[]}"#),
    ("문장: 김철수는 1~2열", r#"{"rules":[{"type":"place","who":"김철수","where":{"cols":[1,2]}}]}"#),
    ("요청한 학생: 한도윤\n문장: 앞에서 2 번째 줄이요. 그리고 오지호 근처", r#"{"rules":[{"type":"place","who":"한도윤","where":{"rows":[2,2]}},{"type":"distance","a":"한도윤","b":"오지호","rel":"근처"}]}"#),
];

pub fn user_message(sentence: &str, requester: Option<&str>) -> String {
    match requester {
        Some(r) => format!("요청한 학생: {r}\n문장: {sentence}"),
        None => format!("문장: {sentence}"),
    }
}

/// 공통 앞부분(시스템 + 예시). 모든 문장에서 똑같으므로 엔진이 계산 결과를 재사용할 수 있다.
pub fn prefix() -> Vec<ChatMessage> {
    let mut v = vec![ChatMessage::new("system", SYSTEM)];
    for (u, a) in EXAMPLES {
        v.push(ChatMessage::new("user", u));
        v.push(ChatMessage::new("assistant", a));
    }
    v
}

pub fn messages(sentence: &str, requester: Option<&str>) -> Vec<ChatMessage> {
    let mut v = prefix();
    v.push(ChatMessage::new("user", &user_message(sentence, requester)));
    v
}

#[cfg(test)]
mod tests {
    #[test]
    fn examples_are_valid_outputs() {
        // 예시 답이 우리 문법·파서와 맞는지 확인
        for (_, a) in super::EXAMPLES {
            let v: serde_json::Value = serde_json::from_str(a).unwrap();
            let mut names = vec![];
            for r in v["rules"].as_array().unwrap() {
                for k in ["who", "a", "b"] {
                    if let Some(s) = r.get(k).and_then(|x| x.as_str()) {
                        names.push(s.to_string());
                    }
                }
            }
            crate::schema::parse_output(a, &names).unwrap();
        }
    }
}
