//! 문장의 단서(숫자, 부정어, 장소 낱말)를 프로그램이 먼저 찾는다.
//!
//! 단서에 맞는 출력 형식만 문법에서 허용한다. 작은 모델이 엉뚱한 형식을 고르는 일을 막기 위해서다.
//! 예를 들어 숫자가 없는 문장에서는 `{"seat":[1,1]}`을 낼 수 없고,
//! 부정어가 없는 문장에서는 `{"not":...}`을 낼 수 없다.

/// 부정 표현. "안 좋음", "안 보임"처럼 상태를 말하는 "안"은 넣지 않는다.
const NEGATION: &[&str] = &[
    "싫", "말고", "빼고", "제외", "금지", "피해", "피하", "아니", "아님", "안 됨", "안됨", "안 돼", "안돼", "안 되", "안되",
    "안 됐", "안됐", "않", "말 것", "말것", "마세요", "말아", "하지 마", "하지마", "없게", "없도록",
];

/// 자리(장소)를 말하는 낱말. 같은 무리는 한 번만 센다.
const PLACE: &[&[&str]] = &[
    &["앞"],
    &["뒤"],
    &["창가", "창문", "창 쪽", "창쪽"],
    &["복도"],
    &["가운데", "중앙"],
    &["교탁", "선생님", "집중", "떠들", "떠듦", "딴짓", "감시"],
    &["칠판"],
    &["앞문", "뒷문", "출입문", "문 근처", "문 옆", "문쪽", "문 쪽"],
    &["줄", "열", "번째"],
    &["시력", "눈이", "눈 ", "키가", "키 ", "청력", "귀가", "안 보", "잘 보"],
    &["맨", "구석", "끝"],
];

/// "이거나 저거" 같은 선택 표현
const CHOICE: &[&str] = &["이나 ", "나 ", "거나", "또는", "혹은", "아니면"];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Cues {
    pub digits: bool,
    pub negation: bool,
    /// 장소 낱말 무리 수
    pub place_groups: usize,
    pub choice: bool,
}

impl Cues {
    /// `text`는 학생 이름을 지운 문장이어야 한다("문채린"의 "문" 같은 오인 방지).
    pub fn find(text: &str) -> Self {
        Cues {
            digits: text.chars().any(|c| c.is_ascii_digit()),
            negation: NEGATION.iter().any(|n| text.contains(n)),
            place_groups: PLACE.iter().filter(|g| g.iter().any(|w| text.contains(w))).count(),
            choice: CHOICE.iter().any(|w| text.contains(w)),
        }
    }

    pub fn place(&self) -> bool {
        self.place_groups > 0 || self.digits
    }
}

/// "두 번째 줄", "앞 세 줄" 같은 한글 수를 숫자로 바꾼다.
pub fn normalize_numbers(s: &str) -> String {
    const NUMS: &[(&str, &str)] = &[
        ("첫", "1"),
        ("한", "1"),
        ("두", "2"),
        ("둘", "2"),
        ("세", "3"),
        ("셋", "3"),
        ("네", "4"),
        ("넷", "4"),
        ("다섯", "5"),
        ("여섯", "6"),
        ("일곱", "7"),
        ("여덟", "8"),
        ("아홉", "9"),
    ];
    const SUFFIX: &[&str] = &[" 번째", "번째", "째 ", "째줄", "째 줄", " 줄", "줄", " 열", "열"];
    let mut out = s.to_string();
    for (k, v) in NUMS {
        for suf in SUFFIX {
            let from = format!("{k}{suf}");
            if out.contains(&from) {
                // 앞 글자가 한글이면 다른 낱말의 일부("주세요" 등)이므로 바꾸지 않는다.
                let mut result = String::new();
                let mut rest = out.as_str();
                while let Some(i) = rest.find(&from) {
                    let prev = rest[..i].chars().last();
                    let ok = prev.map_or(true, |c| !('가'..='힣').contains(&c));
                    result.push_str(&rest[..i]);
                    result.push_str(&if ok { format!("{v}{suf}") } else { from.clone() });
                    rest = &rest[i + from.len()..];
                }
                result.push_str(rest);
                out = result;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negation_is_not_state() {
        assert!(!Cues::find(" 시력 안 좋음").negation);
        assert!(!Cues::find(" 칠판이 잘 안 보인대요").negation);
        assert!(Cues::find(" 뒷문 근처는 싫어요").negation);
        assert!(Cues::find(" 짝 안 됐으면").negation);
        assert!(Cues::find(" 가까이 안 되게").negation);
    }

    #[test]
    fn place_and_digits() {
        let c = Cues::find(" 창가 맨 뒤");
        assert_eq!(c.place_groups, 3);
        assert!(!c.digits);
        assert!(Cues::find(" 1줄 2열").digits);
        assert!(!Cues::find(" 오늘 결석").place());
        assert!(Cues::find(" 창가나 맨 뒤").choice);
    }

    #[test]
    fn numbers() {
        assert_eq!(normalize_numbers("앞에서 두 번째 줄"), "앞에서 2 번째 줄");
        assert_eq!(normalize_numbers("앞 두 줄 안에"), "앞 2 줄 안에");
        assert_eq!(normalize_numbers("세 줄"), "3 줄");
        assert_eq!(normalize_numbers("앉혀 주세요"), "앉혀 주세요");
        assert_eq!(normalize_numbers("첫째 줄"), "1째 줄");
    }
}
