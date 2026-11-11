use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

/// 一份规章：文件名即标题，正文是 Markdown。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuleDoc {
    pub filename: String,
    pub content: String,
}

const FORBIDDEN: [char; 9] = ['\\', '/', ':', '*', '?', '"', '<', '>', '|'];

/// 规章目录：`<项目文件夹>/.imates/rules`。
pub fn rules_dir(folder: &Path) -> PathBuf {
    folder.join(".imates").join("rules")
}

/// 文件名必须以 `.md` 结尾、主名非空且不以点开头、不含路径分隔符等字符。
/// 这同时保证了文件不会写到规章目录之外。
pub fn validate_filename(filename: &str) -> AppResult<()> {
    let stem_ok = filename
        .strip_suffix(".md")
        .is_some_and(|stem| !stem.trim().is_empty() && !stem.starts_with('.'));
    if stem_ok && !filename.contains(FORBIDDEN) {
        Ok(())
    } else {
        Err(AppError::Invalid(format!(
            "规章文件名不合法：{filename}（需以 .md 结尾，且不含 \\ / : * ? \" < > |）"
        )))
    }
}

pub fn list(folder: &Path) -> AppResult<Vec<RuleDoc>> {
    let dir = rules_dir(folder);
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut docs = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let path = entry?.path();
        if path.is_file() && path.extension().is_some_and(|ext| ext == "md") {
            let filename = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            docs.push(RuleDoc {
                filename,
                content: fs::read_to_string(&path)?,
            });
        }
    }
    docs.sort_by(|a, b| a.filename.cmp(&b.filename));
    Ok(docs)
}

pub fn save(folder: &Path, filename: &str, content: &str) -> AppResult<()> {
    validate_filename(filename)?;
    let dir = rules_dir(folder);
    fs::create_dir_all(&dir)?;
    fs::write(dir.join(filename), content)?;
    Ok(())
}

pub fn delete(folder: &Path, filename: &str) -> AppResult<()> {
    validate_filename(filename)?;
    let path = rules_dir(folder).join(filename);
    if !path.is_file() {
        return Err(AppError::NotFound(format!("规章 {filename}")));
    }
    fs::remove_file(path)?;
    Ok(())
}

/// 规章变更后给现有会话看的通知。`current` 必须是变更后的 `list` 结果。
/// 只生成文本，不改写已经发给 CLI 的提示词。
pub fn change_notice(operation: &str, filename: &str, content: Option<&str>, current: &[RuleDoc]) -> String {
    let version = if current.is_empty() {
        "none".to_string()
    } else {
        current.iter().map(|rule| format!("{}:{}", rule.filename, rule.content.len())).collect::<Vec<_>>().join("|")
    };
    let files = if current.is_empty() {
        "无".to_string()
    } else {
        current.iter().map(|rule| rule.filename.as_str()).collect::<Vec<_>>().join("、")
    };
    let mut notice = format!("规章版本：{version}\n操作：{operation}\n对象：{filename}\n有效文件：{files}\n");
    if current.is_empty() {
        notice.push_str("旧规章全部失效，当前暂无规章");
    } else {
        notice.push_str("此前的规章版本已失效。");
    }
    if let Some(content) = content {
        notice.push_str(&format!("\n{filename} 全文：\n{content}"));
    }
    notice
}

/// 64 位 FNV-1a。只用来判断规章内容是否变化，不用于安全目的。
pub fn fnv1a(text: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// 用有序的文件名和全文算出的规章版本。没有规章时为 `none`。
pub fn fingerprint(rules: &[RuleDoc]) -> String {
    if rules.is_empty() {
        return "none".to_string();
    }
    let mut text = String::new();
    for rule in rules {
        text.push_str(&rule.filename);
        text.push('\0');
        text.push_str(&rule.content);
        text.push('\0');
    }
    fnv1a(&text)
}

/// 文件名到正文摘要。用来比较两次读取之间哪些规章变了。
pub fn digest_map(rules: &[RuleDoc]) -> std::collections::BTreeMap<String, String> {
    rules.iter().map(|rule| (rule.filename.clone(), fnv1a(&rule.content))).collect()
}

/// 应用之外修改规章后给现有会话看的通知。没有变化时返回 None。
/// 监听只能证明文件变了，这里不写是谁改的。
pub fn external_change_notice(previous: &std::collections::BTreeMap<String, String>, current: &[RuleDoc]) -> Option<String> {
    let now = digest_map(current);
    let added: Vec<&RuleDoc> = current.iter().filter(|rule| !previous.contains_key(&rule.filename)).collect();
    let replaced: Vec<&RuleDoc> = current
        .iter()
        .filter(|rule| previous.get(&rule.filename).is_some_and(|old| Some(old) != now.get(&rule.filename)))
        .collect();
    let deleted: Vec<&String> = previous.keys().filter(|name| !now.contains_key(*name)).collect();
    if added.is_empty() && replaced.is_empty() && deleted.is_empty() {
        return None;
    }
    let names = |items: Vec<&str>| if items.is_empty() { "无".to_string() } else { items.join("、") };
    let mut notice = format!(
        "规章版本：{}\n操作：外部修改\n新增：{}\n替换：{}\n删除：{}\n有效文件：{}\n",
        fingerprint(current),
        names(added.iter().map(|rule| rule.filename.as_str()).collect()),
        names(replaced.iter().map(|rule| rule.filename.as_str()).collect()),
        names(deleted.iter().map(|name| name.as_str()).collect()),
        names(current.iter().map(|rule| rule.filename.as_str()).collect()),
    );
    if current.is_empty() {
        notice.push_str("旧规章全部失效，当前暂无规章");
    } else {
        notice.push_str("此前的规章版本已失效。");
    }
    for rule in added.iter().chain(replaced.iter()) {
        notice.push_str(&format!("\n{} 全文：\n{}", rule.filename, rule.content));
    }
    Some(notice)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_filename_accepts_normal_names() {
        assert!(validate_filename("协作规范.md").is_ok());
        assert!(validate_filename("API design v2.md").is_ok());
    }

    #[test]
    fn validate_filename_rejects_bad_names() {
        for bad in [
            "协作规范.txt",
            ".md",
            "  .md",
            ".hidden.md",
            "a/b.md",
            "a\\b.md",
            "../逃逸.md",
            "C:x.md",
            "问?.md",
        ] {
            assert!(validate_filename(bad).is_err(), "应当拒绝：{bad}");
        }
    }

    #[test]
    fn list_is_empty_when_directory_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(list(dir.path()).unwrap(), Vec::<RuleDoc>::new());
    }

    #[test]
    fn save_creates_directory_and_list_returns_sorted_docs() {
        let dir = tempfile::tempdir().unwrap();
        save(dir.path(), "开发规范.md", "# 开发\n").unwrap();
        save(dir.path(), "协作规范.md", "# 协作\n").unwrap();
        std::fs::write(rules_dir(dir.path()).join("笔记.txt"), "不是规章").unwrap();

        let docs = list(dir.path()).unwrap();
        let names: Vec<&str> = docs.iter().map(|d| d.filename.as_str()).collect();
        assert_eq!(names, ["协作规范.md", "开发规范.md"]);
        assert_eq!(docs[0].content, "# 协作\n");
        assert!(dir.path().join(".imates").join("rules").is_dir());
    }

    #[test]
    fn save_overwrites_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        save(dir.path(), "协作规范.md", "旧").unwrap();
        save(dir.path(), "协作规范.md", "新").unwrap();
        assert_eq!(list(dir.path()).unwrap()[0].content, "新");
    }

    #[test]
    fn save_and_delete_reject_invalid_filename() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            save(dir.path(), "../逃逸.md", "x"),
            Err(AppError::Invalid(_))
        ));
        assert!(matches!(
            delete(dir.path(), "../逃逸.md"),
            Err(AppError::Invalid(_))
        ));
    }

    #[test]
    fn delete_removes_file_and_missing_is_not_found() {
        let dir = tempfile::tempdir().unwrap();
        save(dir.path(), "协作规范.md", "x").unwrap();
        delete(dir.path(), "协作规范.md").unwrap();
        assert!(list(dir.path()).unwrap().is_empty());
        assert!(matches!(
            delete(dir.path(), "协作规范.md"),
            Err(AppError::NotFound(_))
        ));
    }

    #[test]
    fn change_notice_records_version_operation_full_text_and_invalidation() {
        let dir = tempfile::tempdir().unwrap();
        let added = "# 协作\n不得改已发出的提示词";
        save(dir.path(), "协作规范.md", added).unwrap();
        save(dir.path(), "开发规范.md", "开发正文").unwrap();
        let current = list(dir.path()).unwrap();
        let notice = change_notice("新增", "协作规范.md", Some(added), &current);
        let version = format!(
            "规章版本：{}:{}|{}:{}",
            current[0].filename,
            current[0].content.len(),
            current[1].filename,
            current[1].content.len()
        );
        assert!(notice.contains(&version), "{notice}");
        assert!(notice.contains("操作：新增"), "{notice}");
        assert!(notice.contains("协作规范.md"), "{notice}");
        assert!(notice.contains("开发规范.md"), "{notice}");
        assert!(notice.contains(added), "{notice}");
        assert!(notice.contains("失效"), "{notice}");
    }

    #[test]
    fn change_notice_for_clearing_the_last_rule_says_none_remain() {
        let notice = change_notice("删除", "协作规范.md", None, &[]);
        assert!(notice.contains("规章版本：none"), "{notice}");
        assert!(notice.contains("操作：删除"), "{notice}");
        assert!(notice.contains("旧规章全部失效，当前暂无规章"), "{notice}");
    }

    #[test]
    fn fingerprint_changes_with_content_even_when_length_is_equal() {
        let doc = |name: &str, text: &str| RuleDoc { filename: name.into(), content: text.into() };
        assert_eq!(fingerprint(&[]), "none");
        let first = fingerprint(&[doc("协作规范.md", "甲")]);
        assert_eq!(first.len(), 16);
        assert_eq!(first, fingerprint(&[doc("协作规范.md", "甲")]));
        assert_ne!(first, fingerprint(&[doc("协作规范.md", "乙")]));
        assert_ne!(first, fingerprint(&[doc("开发规范.md", "甲")]));
        assert_eq!(fnv1a(""), "cbf29ce484222325");
    }

    #[test]
    fn external_notice_lists_added_replaced_and_deleted_files() {
        let doc = |name: &str, text: &str| RuleDoc { filename: name.into(), content: text.into() };
        let previous = digest_map(&[doc("协作规范.md", "旧协作"), doc("设计规范.md", "设计")]);
        let current = vec![doc("协作规范.md", "新协作"), doc("开发规范.md", "开发正文")];
        assert!(external_change_notice(&previous, &[doc("协作规范.md", "旧协作"), doc("设计规范.md", "设计")]).is_none());
        let notice = external_change_notice(&previous, &current).unwrap();
        for text in [
            "操作：外部修改", "新增：开发规范.md", "替换：协作规范.md", "删除：设计规范.md",
            "有效文件：协作规范.md、开发规范.md", "此前的规章版本已失效",
            "协作规范.md 全文：\n新协作", "开发规范.md 全文：\n开发正文",
        ] {
            assert!(notice.contains(text), "通知缺少：{text}\n{notice}");
        }
        assert!(notice.contains(&format!("规章版本：{}", fingerprint(&current))));
        assert!(!notice.contains("设计\n"), "删除的规章不应再附全文");
        let cleared = external_change_notice(&previous, &[]).unwrap();
        assert!(cleared.contains("规章版本：none"));
        assert!(cleared.contains("旧规章全部失效，当前暂无规章"));
        assert!(cleared.contains("删除：协作规范.md、设计规范.md"));
    }
}
