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
}
