use std::path::{Component, Path, PathBuf};

use serde::Serialize;

use crate::error::{AppError, AppResult};

/// 文件树和文件监听都不展示的目录名。
pub const IGNORED_DIRS: [&str; 8] = [".git", "node_modules", ".imates", "target", "dist", ".venv", "__pycache__", ".idea"];
/// 单次列出的条目上限，避免超大目录卡住界面。
pub const MAX_ENTRIES: usize = 2000;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileListing {
    pub entries: Vec<FileEntry>,
    pub truncated: bool,
}

pub fn is_ignored(name: &str) -> bool {
    IGNORED_DIRS.contains(&name)
}

/// 相对项目文件夹的路径，统一用 `/` 分隔。不在文件夹内或就是文件夹本身时返回 None。
pub fn relative_key(folder: &Path, path: &Path) -> Option<String> {
    let rest = path.strip_prefix(folder).ok()?;
    let parts = rest
        .components()
        .map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    if parts.is_empty() { None } else { Some(parts.join("/")) }
}

pub fn list_files(folder: &Path) -> AppResult<FileListing> {
    if !folder.is_dir() {
        return Err(AppError::Invalid("项目文件夹不存在".into()));
    }
    let mut entries = Vec::new();
    let mut truncated = false;
    walk(folder, folder, &mut entries, &mut truncated)?;
    Ok(FileListing { entries, truncated })
}

fn walk(root: &Path, dir: &Path, entries: &mut Vec<FileEntry>, truncated: &mut bool) -> AppResult<()> {
    let mut children = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        // file_type 不跟随符号链接；链接一律不展示，避免走出项目文件夹或成环。
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if kind.is_dir() && is_ignored(&name) {
            continue;
        }
        children.push((name, entry.path(), kind.is_dir()));
    }
    children.sort_by(|a, b| {
        b.2.cmp(&a.2)
            .then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase()))
            .then_with(|| a.0.cmp(&b.0))
    });
    for (name, path, is_dir) in children {
        if entries.len() >= MAX_ENTRIES {
            *truncated = true;
            return Ok(());
        }
        let Some(key) = relative_key(root, &path) else { continue };
        entries.push(FileEntry { path: key, name, is_dir });
        if is_dir {
            // 读不了的子目录只是不展开，不让整棵树失败。
            let _ = walk(root, &path, entries, truncated);
            if *truncated {
                return Ok(());
            }
        }
    }
    Ok(())
}

/// 把前端传来的相对路径解析成项目文件夹内的真实路径。
pub fn resolve_inside(folder: &Path, relative: &str) -> AppResult<PathBuf> {
    let rel = Path::new(relative);
    let plain = !relative.trim().is_empty() && rel.components().all(|component| matches!(component, Component::Normal(_)));
    if !plain {
        return Err(AppError::Invalid("文件路径不合法".into()));
    }
    let hidden = rel.components().any(|component| matches!(component, Component::Normal(part) if is_ignored(&part.to_string_lossy())));
    if hidden {
        return Err(AppError::Invalid("该路径不在文件列表中".into()));
    }
    let root = folder.canonicalize().map_err(|_| AppError::Invalid("项目文件夹不存在".into()))?;
    let target = root.join(rel).canonicalize().map_err(|_| AppError::NotFound(format!("文件 {relative}")))?;
    if !target.starts_with(&root) {
        return Err(AppError::Invalid("文件不在项目文件夹内".into()));
    }
    Ok(target)
}

/// 交给系统打开时去掉 Windows 的 `\\?\` 前缀，UNC 形式保持原样。
pub fn display_path(path: &Path) -> String {
    let text = path.to_string_lossy().into_owned();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC\\") => rest.to_string(),
        _ => text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for sub in ["src", "node_modules", ".git", ".imates/rules", "docs/深层"] {
            std::fs::create_dir_all(root.join(sub)).unwrap();
        }
        for (file, text) in [
            ("b.txt", "b"), ("A.md", "a"), ("src/main.rs", "fn main() {}"),
            ("node_modules/x.js", "x"), (".git/config", "c"), (".imates/rules/协作规范.md", "r"),
            ("docs/深层/说明.md", "d"),
        ] {
            std::fs::write(root.join(file), text).unwrap();
        }
        dir
    }

    #[test]
    fn lists_directories_first_and_skips_ignored_directories() {
        let dir = project();
        let listing = list_files(dir.path()).unwrap();
        let paths: Vec<&str> = listing.entries.iter().map(|entry| entry.path.as_str()).collect();
        assert_eq!(paths, ["docs", "docs/深层", "docs/深层/说明.md", "src", "src/main.rs", "A.md", "b.txt"]);
        assert!(!listing.truncated);
        assert!(listing.entries[0].is_dir);
        assert_eq!(listing.entries[2].name, "说明.md");
        assert!(list_files(&dir.path().join("不存在")).is_err());
    }

    #[test]
    fn listing_stops_at_the_entry_limit() {
        let dir = tempfile::tempdir().unwrap();
        for index in 0..(MAX_ENTRIES + 5) {
            std::fs::write(dir.path().join(format!("f{index:05}.txt")), "").unwrap();
        }
        let listing = list_files(dir.path()).unwrap();
        assert_eq!(listing.entries.len(), MAX_ENTRIES);
        assert!(listing.truncated);
    }

    #[test]
    fn resolve_inside_rejects_escapes_and_hidden_areas() {
        let dir = project();
        let target = resolve_inside(dir.path(), "src/main.rs").unwrap();
        assert!(target.ends_with("main.rs"));
        let absolute = dir.path().join("b.txt").to_string_lossy().into_owned();
        for bad in ["", "  ", "../外面.txt", "src/../../外面.txt", "node_modules/x.js", ".imates/rules/协作规范.md", "不存在.txt", absolute.as_str()] {
            assert!(resolve_inside(dir.path(), bad).is_err(), "应拒绝：{bad}");
        }
    }

    #[test]
    fn relative_key_uses_forward_slashes_and_rejects_outside_paths() {
        let root = std::path::Path::new("/项目");
        assert_eq!(relative_key(root, &root.join("src").join("main.rs")).as_deref(), Some("src/main.rs"));
        assert_eq!(relative_key(root, root), None);
        assert_eq!(relative_key(root, std::path::Path::new("/别处/a.txt")), None);
    }

    #[test]
    fn display_path_drops_the_windows_verbatim_prefix() {
        assert_eq!(display_path(std::path::Path::new(r"\\?\C:\项目\a.txt")), r"C:\项目\a.txt");
        assert_eq!(display_path(std::path::Path::new(r"\\?\UNC\server\share\a.txt")), r"\\?\UNC\server\share\a.txt");
        assert_eq!(display_path(std::path::Path::new("/项目/a.txt")), "/项目/a.txt");
    }
}
