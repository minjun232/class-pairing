//! llama.cpp GBNF 문법: 출력이 반드시 [`crate::schema`] 형식의 JSON이 되도록 강제한다.
//!
//! - 이름 자리에는 이 문장에 나온 명단 이름만 올 수 있다.
//! - 문장의 단서([`Cues`])에 맞는 형식만 허용한다(숫자 없으면 줄·열 없음, 부정어 없으면 not 없음 …).

use crate::cues::Cues;
use crate::schema::ZONE_WORDS;

fn lit(s: &str) -> String {
    // GBNF 문자열 리터럴: JSON 문자열 "..."을 그대로 출력하도록
    let json = serde_json::to_string(s).expect("string");
    let escaped = json.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

fn alt(items: impl IntoIterator<Item = String>) -> String {
    items.into_iter().collect::<Vec<_>>().join(" | ")
}

/// 이 문장에서 만들 수 있는 규칙 종류
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Allowed {
    pub place: bool,
    pub distance: bool,
}

pub fn allowed(names: &[String], cues: &Cues) -> Allowed {
    Allowed { place: cues.place(), distance: names.len() >= 2 }
}

pub fn build(names: &[String], cues: &Cues) -> String {
    let a = allowed(names, cues);
    let name = alt(names.iter().map(|n| lit(n)));
    let zone = alt(ZONE_WORDS.iter().filter(|z| cues.zone_ok(z)).map(|z| lit(z)));
    // 부정어가 있으면 "멀리/짝아님"만, 없으면 "짝/근처/멀리"만
    let rels: &[&str] = if cues.negation { &["멀리", "짝아님"] } else { &["짝", "근처", "멀리"] };
    let rel = alt(rels.iter().map(|r| lit(r)));

    let mut rule_alts = vec![];
    if a.place {
        rule_alts.push("place");
    }
    if a.distance {
        rule_alts.push("dist");
    }
    let rules = if rule_alts.is_empty() { String::new() } else { "( rule ( \",\" rule )* )?".to_string() };
    let rule = if rule_alts.is_empty() { "\"\"".to_string() } else { rule_alts.join(" | ") };

    let simple = if cues.digits { "zone | range" } else { "zone" };
    let mut where_alts = vec!["simple"];
    if cues.negation {
        where_alts.push("not");
    }
    if cues.place_groups + usize::from(cues.digits) >= 2 {
        where_alts.push(if cues.choice { "any" } else { "all" });
    }
    let where_ = where_alts.join(" | ");

    format!(
        r#"root ::= "{{\"rules\":[" {rules} "]}}"
rule ::= {rule}
place ::= "{{\"type\":\"place\",\"who\":" name ",\"where\":" where "}}"
dist ::= "{{\"type\":\"distance\",\"a\":" name ",\"b\":" name ",\"rel\":" rel "}}"
where ::= {where_}
simple ::= {simple}
range ::= "{{\"" ( "rows" | "cols" | "seat" ) "\":[" num "," num "]}}"
not ::= "{{\"not\":" simple "}}"
any ::= "{{\"any\":[" simple ( "," simple )+ "]}}"
all ::= "{{\"all\":[" simple ( "," simple )+ "]}}"
num ::= [1-9] [0-9]?
zone ::= {zone}
rel ::= {rel}
name ::= {name}
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_escaping() {
        assert_eq!(lit("김철수"), r#""\"김철수\"""#);
        let g = build(&["김민수 A".into()], &Cues::find(" 앞쪽"));
        assert!(g.contains(r#"name ::= "\"김민수 A\"""#));
    }

    #[test]
    fn cues_shape_grammar() {
        let one = vec!["김철수".to_string()];
        let g = build(&one, &Cues::find(" 시력 안 좋음"));
        assert!(g.contains("rule ::= place\n"));
        assert!(g.contains("where ::= simple\n"));
        assert!(g.contains("simple ::= zone\n"));
        let two = vec!["김철수".to_string(), "이영희".to_string()];
        let g = build(&two, &Cues::find("랑 멀리"));
        assert!(g.contains("rule ::= dist\n"));
        assert!(g.contains(r#"rel ::= "\"짝\"" | "\"근처\"" | "\"멀리\"""#));
        let g = build(&one, &Cues::find(" 3줄 5열 말고"));
        assert!(g.contains("where ::= simple | not | all"));
        assert!(g.contains("simple ::= zone | range"));
        let g = build(&one, &Cues::find("은 맨뒷줄"));
        assert!(!g.contains("뒷문근처") && g.contains("맨뒤"));
    }
}
