use crate::error::{AppError, AppResult};

/// 人设固定的四段，顺序即书写顺序。
pub const SECTIONS: [&str; 4] = ["定位", "职责", "边界", "工作风格"];

/// 取出某一段的正文（不含标题，首尾去空白）。没有这一段时返回 None。
pub fn section_body(markdown: &str, section: &str) -> Option<String> {
    let heading = format!("## {section}");
    let mut body: Option<Vec<&str>> = None;
    for line in markdown.lines() {
        let trimmed = line.trim();
        if trimmed == heading {
            body = Some(Vec::new());
            continue;
        }
        if body.is_some() && SECTIONS.iter().any(|s| trimmed == format!("## {s}")) {
            break;
        }
        if let Some(lines) = body.as_mut() {
            lines.push(line);
        }
    }
    body.map(|lines| lines.join("\n").trim().to_string())
}

/// 四段标题齐全且每段都有内容才算合法。
pub fn validate(markdown: &str) -> AppResult<()> {
    for section in SECTIONS {
        match section_body(markdown, section) {
            None => {
                return Err(AppError::Invalid(format!(
                    "人设缺少「## {section}」一段"
                )));
            }
            Some(body) if body.is_empty() => {
                return Err(AppError::Invalid(format!(
                    "人设的「{section}」一段不能为空"
                )));
            }
            Some(_) => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: &str = "## 定位\n\n后端开发\n\n## 职责\n\n- 写接口\n- 写测试\n\n## 边界\n\n不做界面\n\n## 工作风格\n\n务实\n";

    #[test]
    fn validate_accepts_four_filled_sections() {
        assert!(validate(FULL).is_ok());
    }

    #[test]
    fn validate_rejects_missing_section() {
        let text = FULL.replace("## 边界\n\n不做界面\n\n", "");
        let err = validate(&text).unwrap_err().to_string();
        assert!(err.contains("边界"), "{err}");
    }

    #[test]
    fn validate_rejects_empty_section() {
        let text = FULL.replace("务实", "   ");
        let err = validate(&text).unwrap_err().to_string();
        assert!(err.contains("工作风格"), "{err}");
    }

    #[test]
    fn section_body_returns_text_between_headings() {
        assert_eq!(section_body(FULL, "定位").as_deref(), Some("后端开发"));
        assert_eq!(
            section_body(FULL, "职责").as_deref(),
            Some("- 写接口\n- 写测试")
        );
        assert_eq!(section_body(FULL, "工作风格").as_deref(), Some("务实"));
        assert_eq!(section_body("没有标题", "定位"), None);
    }
}
