//! 문장 속 학생 이름 찾기.
//!
//! - 조사 변형("민수랑", "민수는", "지훈이랑")은 부분 문자열로 찾으므로 그대로 처리된다.
//! - 성을 빼고 이름만 쓴 경우("민수")도 찾는다.
//! - 동명이인 표기("김민수 A", "김민수A", "김민수(A)", "민수 A", "김민수1")를 처리한다.
//! - 가장 긴 표기부터 찾고, 이미 찾은 부분과 겹치면 건너뛴다("박지훈" 안의 "지훈").
//! - 한 표기가 여러 학생에 해당하면(동명이인인데 A/B를 안 씀) **모호함**으로 표시한다.

use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mention {
    /// 문장 안 바이트 위치
    pub start: usize,
    pub end: usize,
    /// 해당하는 명단 이름 (2개 이상이면 모호)
    pub names: Vec<String>,
}

pub struct NameMatcher {
    /// 표기(정규화) → 명단 이름들
    aliases: BTreeMap<String, BTreeSet<String>>,
}

/// 영문·숫자 비교를 위한 정규화 (대문자 → 소문자)
fn norm(s: &str) -> String {
    s.to_lowercase()
}

/// "김민수 A" → ("김민수", Some("A")), "김민수1" → ("김민수", Some("1")), "김민수(A)" → ("김민수", Some("A"))
pub fn split_suffix(name: &str) -> (String, Option<String>) {
    let n = name.trim();
    if let Some(open) = n.rfind('(') {
        if n.ends_with(')') {
            let inner = n[open + 1..n.len() - 1].trim();
            if !inner.is_empty() {
                return (n[..open].trim().to_string(), Some(inner.to_string()));
            }
        }
    }
    let suffix_len: usize = n.chars().rev().take_while(|c| c.is_ascii_alphanumeric()).map(|c| c.len_utf8()).sum();
    if suffix_len > 0 && suffix_len < n.len() {
        let core = n[..n.len() - suffix_len].trim_end();
        if core.chars().any(|c| !c.is_ascii()) {
            return (core.to_string(), Some(n[n.len() - suffix_len..].to_string()));
        }
    }
    (n.to_string(), None)
}

/// 성을 뺀 이름. 3글자 → 뒤 2글자, 4글자(두 글자 성) → 뒤 2글자. 2글자 이하는 없음.
fn given_name(core: &str) -> Option<String> {
    let chars: Vec<char> = core.chars().collect();
    let all_hangul = chars.iter().all(|c| ('가'..='힣').contains(c));
    if !all_hangul {
        return None;
    }
    match chars.len() {
        3 => Some(chars[1..].iter().collect()),
        4 => Some(chars[2..].iter().collect()),
        _ => None,
    }
}

impl NameMatcher {
    pub fn new<'a>(roster: impl IntoIterator<Item = &'a str>) -> Self {
        let mut aliases: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut add = |alias: String, name: &str| {
            if alias.chars().count() >= 2 {
                aliases.entry(norm(&alias)).or_default().insert(name.to_string());
            }
        };
        for name in roster {
            add(name.to_string(), name);
            let (core, suffix) = split_suffix(name);
            let given = given_name(&core);
            let mut bases = vec![core.clone()];
            bases.extend(given);
            for base in &bases {
                match &suffix {
                    Some(sfx) => {
                        add(format!("{base} {sfx}"), name);
                        add(format!("{base}{sfx}"), name);
                        add(format!("{base}({sfx})"), name);
                        add(format!("{base} ({sfx})"), name);
                        // 접미사 없이 쓴 경우 → 같은 core의 다른 학생과 함께 모호
                        add(base.clone(), name);
                    }
                    None => add(base.clone(), name),
                }
            }
        }
        Self { aliases }
    }

    /// 문장에서 이름을 찾는다.
    pub fn find(&self, sentence: &str) -> Vec<Mention> {
        let lower = norm(sentence);
        // to_lowercase는 한글·숫자 길이를 바꾸지 않지만, 안전하게 길이가 같을 때만 위치를 쓴다.
        let text = if lower.len() == sentence.len() { lower } else { sentence.to_string() };
        let mut keys: Vec<&String> = self.aliases.keys().collect();
        keys.sort_by(|a, b| b.chars().count().cmp(&a.chars().count()).then(a.cmp(b)));
        let mut taken = vec![false; text.len()];
        let mut out = Vec::new();
        for k in keys {
            let mut from = 0;
            while let Some(i) = text[from..].find(k.as_str()) {
                let (s, e) = (from + i, from + i + k.len());
                from = e;
                if taken[s..e].iter().any(|&t| t) {
                    continue;
                }
                // 영문 접미사 뒤에 영문이 바로 이어지면 다른 단어 (예: "민수 Ab")
                if k.ends_with(|c: char| c.is_ascii_alphanumeric())
                    && text[e..].starts_with(|c: char| c.is_ascii_alphanumeric())
                {
                    continue;
                }
                taken[s..e].iter_mut().for_each(|t| *t = true);
                out.push(Mention { start: s, end: e, names: self.aliases[k].iter().cloned().collect() });
            }
        }
        out.sort_by_key(|m| m.start);
        out
    }
}

/// 이름 인식 결과로 문장을 다시 쓴다: 찾은 표기를 명단 이름으로 바꾼다.
/// 모호한 이름이 있으면 None.
pub fn canonicalize(sentence: &str, mentions: &[Mention]) -> Option<(String, Vec<String>)> {
    let mut out = String::new();
    let mut names = Vec::new();
    let mut last = 0;
    for m in mentions {
        if m.names.len() != 1 {
            return None;
        }
        out.push_str(&sentence[last..m.start]);
        out.push_str(&m.names[0]);
        last = m.end;
        if !names.contains(&m.names[0]) {
            names.push(m.names[0].clone());
        }
    }
    out.push_str(&sentence[last..]);
    Some((out, names))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roster() -> Vec<&'static str> {
        vec!["김민수 A", "김민수 B", "박지훈", "이서연", "남궁하늘", "최유나1", "최유나2", "이안"]
    }

    fn found(s: &str) -> Vec<Vec<String>> {
        NameMatcher::new(roster()).find(s).into_iter().map(|m| m.names).collect()
    }

    #[test]
    fn josa_and_given_names() {
        assert_eq!(found("지훈이랑 서연이 짝"), vec![vec!["박지훈".to_string()], vec!["이서연".to_string()]]);
        assert_eq!(found("박지훈은 앞자리"), vec![vec!["박지훈".to_string()]]);
        assert_eq!(found("하늘이 창가"), vec![vec!["남궁하늘".to_string()]]);
        assert_eq!(found("이안 뒷문 근처"), vec![vec!["이안".to_string()]]);
    }

    #[test]
    fn homonyms() {
        assert_eq!(found("민수 A랑 지훈 멀리")[0], vec!["김민수 A".to_string()]);
        assert_eq!(found("김민수B는 앞")[0], vec!["김민수 B".to_string()]);
        assert_eq!(found("김민수(a) 창가")[0], vec!["김민수 A".to_string()]);
        assert_eq!(found("유나2랑 유나1 멀리"), vec![vec!["최유나2".to_string()], vec!["최유나1".to_string()]]);
        // 접미사 없음 → 모호
        assert_eq!(found("민수는 앞")[0].len(), 2);
    }

    #[test]
    fn canonical() {
        let m = NameMatcher::new(roster());
        let s = "지훈이랑 민수 A 멀리";
        let (c, names) = canonicalize(s, &m.find(s)).unwrap();
        assert_eq!(c, "박지훈이랑 김민수 A 멀리");
        assert_eq!(names, ["박지훈", "김민수 A"]);
        assert!(canonicalize("민수 앞", &m.find("민수 앞")).is_none());
    }

    #[test]
    fn split() {
        assert_eq!(split_suffix("김민수 A"), ("김민수".into(), Some("A".into())));
        assert_eq!(split_suffix("김민수1"), ("김민수".into(), Some("1".into())));
        assert_eq!(split_suffix("김민수(B)"), ("김민수".into(), Some("B".into())));
        assert_eq!(split_suffix("박지훈"), ("박지훈".into(), None));
        assert_eq!(split_suffix("Tom"), ("Tom".into(), None));
    }
}
