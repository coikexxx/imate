use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex, MutexGuard};
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use crate::error::{AppError, AppResult};

/// 一段安静期之内合并上报。
const DEBOUNCE: Duration = Duration::from_millis(300);
/// 单批最多合并的原始事件数，持续写入时也能及时上报。
const MAX_RAW: usize = 4096;

/// 一次上报：改动的文件（相对路径）和规章目录是否有变化。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WatchBatch {
    pub paths: Vec<String>,
    pub rules_touched: bool,
}

/// 把原始路径归成相对路径，去重排序，并丢掉忽略目录。监听不判断是谁改的。
pub fn classify(folder: &Path, raw: &[PathBuf]) -> WatchBatch {
    let mut paths = BTreeSet::new();
    let mut rules_touched = false;
    for path in raw {
        let Some(key) = crate::files::relative_key(folder, path) else { continue };
        if key == ".imates/rules" || key.starts_with(".imates/rules/") {
            rules_touched = true;
            continue;
        }
        if key.split('/').any(crate::files::is_ignored) {
            continue;
        }
        paths.insert(key);
    }
    WatchBatch { paths: paths.into_iter().collect(), rules_touched }
}

struct Entry {
    folder: PathBuf,
    _watcher: RecommendedWatcher,
    changed: BTreeSet<String>,
}

/// 每个项目最多一个监听器。高亮集合只存在内存里。
#[derive(Default)]
pub struct WatchHub {
    entries: Arc<Mutex<HashMap<String, Entry>>>,
}

impl WatchHub {
    fn lock(&self) -> AppResult<MutexGuard<'_, HashMap<String, Entry>>> {
        self.entries.lock().map_err(|_| AppError::Invalid("文件监听状态不可用，请重启应用".into()))
    }

    pub fn ensure(&self, project_id: &str, folder: &Path, on_batch: impl Fn(WatchBatch) + Send + 'static) -> AppResult<bool> {
        // 用规范化后的路径监听，事件里的路径才能和它对上（macOS 的 /private、Windows 的 \\?\）。
        let folder = folder.canonicalize().map_err(|_| AppError::Invalid("项目目录不存在或无法读取".into()))?;
        let mut entries = self.lock()?;
        if entries.get(project_id).is_some_and(|entry| entry.folder == folder) {
            return Ok(false);
        }
        let (tx, rx) = mpsc::channel::<PathBuf>();
        let mut watcher = notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
            let Ok(event) = result else { return };
            if matches!(event.kind, notify::EventKind::Access(_)) {
                return;
            }
            for path in event.paths {
                let _ = tx.send(path);
            }
        })
        .map_err(|error| AppError::Invalid(format!("无法监听项目文件夹：{error}")))?;
        watcher
            .watch(&folder, RecursiveMode::Recursive)
            .map_err(|error| AppError::Invalid(format!("无法监听项目文件夹：{error}")))?;
        let shared = self.entries.clone();
        let id = project_id.to_string();
        let root = folder.clone();
        std::thread::Builder::new()
            .name("imates-fs-watch".into())
            .spawn(move || {
                // 监听器被移除后发送端关闭，循环自然结束。
                while let Ok(first) = rx.recv() {
                    let mut raw = vec![first];
                    while raw.len() < MAX_RAW {
                        match rx.recv_timeout(DEBOUNCE) {
                            Ok(next) => raw.push(next),
                            Err(_) => break,
                        }
                    }
                    let batch = classify(&root, &raw);
                    if batch.paths.is_empty() && !batch.rules_touched {
                        continue;
                    }
                    {
                        let Ok(mut entries) = shared.lock() else { break };
                        match entries.get_mut(&id) {
                            Some(entry) if entry.folder == root => entry.changed.extend(batch.paths.iter().cloned()),
                            _ => break,
                        }
                    }
                    on_batch(batch);
                }
            })
            .map_err(|error| AppError::Invalid(format!("无法启动文件监听线程：{error}")))?;
        // 线程要拿到同一把锁才能记录，所以一定晚于这次插入。
        entries.insert(project_id.to_string(), Entry { folder, _watcher: watcher, changed: BTreeSet::new() });
        Ok(true)
    }

    pub fn remove(&self, project_id: &str) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.remove(project_id);
        }
    }

    pub fn changed(&self, project_id: &str) -> Vec<String> {
        self.entries
            .lock()
            .ok()
            .and_then(|entries| entries.get(project_id).map(|entry| entry.changed.iter().cloned().collect()))
            .unwrap_or_default()
    }

    pub fn reset_changed(&self, project_id: &str) {
        if let Ok(mut entries) = self.entries.lock() {
            if let Some(entry) = entries.get_mut(project_id) {
                entry.changed.clear();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    #[test]
    fn classify_drops_ignored_directories_and_flags_rule_changes() {
        let root = std::path::Path::new("/项目");
        let raw = vec![
            root.join("src").join("main.rs"),
            root.join("src").join("main.rs"),
            root.join("node_modules").join("x.js"),
            root.join(".git").join("index"),
            root.join(".imates").join("rules").join("协作规范.md"),
            root.join("README.md"),
            std::path::PathBuf::from("/别处/a.txt"),
        ];
        let batch = classify(root, &raw);
        assert_eq!(batch.paths, ["README.md", "src/main.rs"]);
        assert!(batch.rules_touched);
        let quiet = classify(root, &[root.join(".imates").join("其他.json"), root.join("target").join("debug")]);
        assert!(quiet.paths.is_empty());
        assert!(!quiet.rules_touched);
    }

    fn wait_for(rx: &mpsc::Receiver<WatchBatch>, mut done: impl FnMut(&WatchBatch) -> bool) -> WatchBatch {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut seen = WatchBatch::default();
        while Instant::now() < deadline && !done(&seen) {
            if let Ok(batch) = rx.recv_timeout(Duration::from_millis(500)) {
                seen.paths.extend(batch.paths);
                seen.rules_touched |= batch.rules_touched;
            }
        }
        seen
    }

    #[test]
    fn hub_reports_changes_once_per_project_and_tracks_the_changed_set() {
        let dir = tempfile::tempdir().unwrap();
        let hub = WatchHub::default();
        let (tx, rx) = mpsc::channel();
        assert!(hub.ensure("甲", dir.path(), move |batch| { let _ = tx.send(batch); }).unwrap());
        assert!(!hub.ensure("甲", dir.path(), |_| {}).unwrap(), "同一目录不应重复监听");
        std::fs::write(dir.path().join("a.txt"), "1").unwrap();
        let seen = wait_for(&rx, |batch| batch.paths.iter().any(|path| path == "a.txt"));
        assert!(seen.paths.iter().any(|path| path == "a.txt"), "没有收到 a.txt 的变化");
        assert_eq!(hub.changed("甲"), ["a.txt"]);
        hub.reset_changed("甲");
        assert!(hub.changed("甲").is_empty());

        std::fs::create_dir_all(dir.path().join(".imates").join("rules")).unwrap();
        std::fs::write(dir.path().join(".imates").join("rules").join("协作规范.md"), "x").unwrap();
        let seen = wait_for(&rx, |batch| batch.rules_touched);
        assert!(seen.rules_touched, "规章目录的变化没有上报");
        assert!(hub.changed("甲").is_empty(), "规章文件不进入文件高亮");

        hub.remove("甲");
        assert!(hub.changed("甲").is_empty());
        assert!(hub.ensure("乙", &dir.path().join("不存在"), |_| {}).is_err());
    }
}
