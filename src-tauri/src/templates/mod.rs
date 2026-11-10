use serde::Serialize;

use crate::rules::RuleDoc;

/// 预设角色的人设模板。
#[derive(Debug, Clone, Serialize)]
pub struct PersonaTemplate {
    pub role: String,
    pub persona: String,
}

const RULES: [(&str, &str); 4] = [
    ("协作规范.md", include_str!("rules/collaboration.md")),
    ("开发规范.md", include_str!("rules/development.md")),
    ("设计规范.md", include_str!("rules/design.md")),
    ("写作规范.md", include_str!("rules/writing.md")),
];

const PERSONAS: [(&str, &str); 6] = [
    ("组长", include_str!("personas/leader.md")),
    ("产品", include_str!("personas/product.md")),
    ("开发", include_str!("personas/developer.md")),
    ("测试", include_str!("personas/tester.md")),
    ("撰稿", include_str!("personas/writer.md")),
    ("审校", include_str!("personas/reviewer.md")),
];

pub fn rule_templates() -> Vec<RuleDoc> {
    RULES
        .iter()
        .map(|(filename, content)| RuleDoc {
            filename: filename.to_string(),
            content: content.to_string(),
        })
        .collect()
}

pub fn persona_templates() -> Vec<PersonaTemplate> {
    PERSONAS
        .iter()
        .map(|(role, persona)| PersonaTemplate {
            role: role.to_string(),
            persona: persona.to_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{persona, rules};

    #[test]
    fn rule_templates_have_expected_valid_filenames() {
        let templates = rule_templates();
        let names: Vec<&str> = templates.iter().map(|t| t.filename.as_str()).collect();
        assert_eq!(
            names,
            ["协作规范.md", "开发规范.md", "设计规范.md", "写作规范.md"]
        );
        for template in &templates {
            rules::validate_filename(&template.filename).unwrap();
            assert!(template.content.trim().starts_with("# "));
        }
    }

    #[test]
    fn persona_templates_cover_six_roles_and_are_valid() {
        let templates = persona_templates();
        let roles: Vec<&str> = templates.iter().map(|t| t.role.as_str()).collect();
        assert_eq!(roles, ["组长", "产品", "开发", "测试", "撰稿", "审校"]);
        for template in &templates {
            persona::validate(&template.persona)
                .unwrap_or_else(|e| panic!("{} 的模板不合法：{e}", template.role));
        }
    }
}
