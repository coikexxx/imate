use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::{Child, Command};

/// 直接调用 node 的启动器：某一行正好是 `"<node.exe 绝对路径>" "<入口 .js 绝对路径>" %*`。
/// 2026-11-11 在 Windows 的 dsh 0.2.2-rc.1 上看到这种格式。两个文件都必须存在。
fn direct_node_shim(shim: &str) -> Option<(PathBuf, PathBuf)> {
    shim.lines().map(str::trim).find_map(|line| {
        let rest = line.strip_prefix('"')?;
        let (node, rest) = rest.split_once('"')?;
        let rest = rest.trim_start().strip_prefix('"')?;
        let (script, tail) = rest.split_once('"')?;
        if tail.trim() != "%*" { return None; }
        let (node, script) = (PathBuf::from(node), PathBuf::from(script));
        let node_ok = node.is_absolute() && node.is_file()
            && node.file_name().is_some_and(|name| name.eq_ignore_ascii_case("node.exe"));
        let script_ok = script.is_absolute() && script.is_file()
            && script.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("js"));
        (node_ok && script_ok).then_some((node, script))
    })
}

/// 只接受可核对的 npm 标准入口。`.js` 入口把参数作为 argv 传给 node；
/// 原生 `.exe` 入口直接启动。两种都不经过 cmd.exe。
pub fn resolve_program(program: &Path, args: &[String], path_env: Option<&std::ffi::OsStr>) -> Result<(PathBuf, Vec<String>), String> {
    if !program.extension().is_some_and(|e| e.eq_ignore_ascii_case("cmd")) {
        return Ok((program.to_path_buf(), args.to_vec()));
    }
    let parent = program.parent().ok_or("CLI 启动器没有父目录")?;
    if program.file_stem().and_then(|stem| stem.to_str()).is_some_and(|stem| stem.eq_ignore_ascii_case("dsh")) {
        let shim = std::fs::read_to_string(program).map_err(|_| "无法读取 CLI 启动器")?;
        let (node, script) = direct_node_shim(&shim)
            .ok_or("dsh 启动器不是已核对的格式（应为一行 \"node.exe\" \"入口.js\" %*），请检查安装方式")?;
        let mut node_args = vec![script.to_string_lossy().into_owned()];
        node_args.extend_from_slice(args);
        return Ok((node, node_args));
    }
    let candidates: &[&str] = match program.file_stem().and_then(|s| s.to_str()) {
        Some("claude") => &[
            "node_modules/@anthropic-ai/claude-code/bin/claude.exe",
            "node_modules/@anthropic-ai/claude-code/cli.js",
        ],
        Some("codex") => &["node_modules/@openai/codex/bin/codex.js"],
        _ => return Err("无法解析该 .cmd 启动器，请使用已接入 CLI 的标准 npm 安装".into()),
    };
    let shim = std::fs::read_to_string(program).map_err(|_| "无法读取 CLI 启动器")?;
    let normalized = shim.replace('\\', "/");
    let entry = candidates.iter().copied().find(|entry| normalized.contains(entry) && parent.join(entry).is_file())
        .ok_or("CLI 启动器不符合已验证的 npm 格式，请检查安装")?;
    let target = parent.join(entry);
    if entry.ends_with(".exe") {
        return Ok((target, args.to_vec()));
    }
    let local_node = parent.join("node.exe");
    let node = if local_node.is_file() { local_node } else {
        which::which_in("node", path_env, parent).map_err(|_| "未找到 CLI 所需的 node")?
    };
    let mut node_args = vec![target.to_string_lossy().into_owned()];
    node_args.extend_from_slice(args);
    Ok((node, node_args))
}

/// 把已启动的进程放进可整树终止的组，并在独立线程里记住后代。
/// 快照不占用 stdout/stderr 排空；父进程先退出后，仍按创建时间核对再清理。
pub struct ProcessTree {
    #[cfg(target_os = "macos")]
    macos: MacProcessTree,
    #[cfg(windows)]
    job: isize,
    #[cfg(windows)]
    root: u32,
    #[cfg(windows)]
    root_created: Option<u64>,
    #[cfg(windows)]
    seen: std::sync::Arc<std::sync::Mutex<Vec<(u32, u64)>>>,
    #[cfg(windows)]
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    #[cfg(windows)]
    watcher: Option<std::thread::JoinHandle<()>>,
}

impl Drop for ProcessTree {
    fn drop(&mut self) {
        #[cfg(windows)]
        {
            self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
            if let Some(handle) = self.watcher.take() { let _ = handle.join(); }
            if self.job != 0 {
                unsafe {
                    unsafe extern "system" {
                        fn TerminateJobObject(job: *mut std::ffi::c_void, code: u32) -> i32;
                        fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
                    }
                    let handle = self.job as *mut std::ffi::c_void;
                    TerminateJobObject(handle, 1);
                    CloseHandle(handle);
                }
                self.job = 0;
            }
            let recorded = self.seen.lock().map(|seen| seen.clone()).unwrap_or_default();
            if self.root_created.is_some_and(|created| process_created(self.root) == Some(created)) {
                taskkill_tree(self.root);
            }
            for (pid, created) in recorded {
                if process_created(pid) == Some(created) { taskkill_tree(pid); }
            }
        }
    }
}

pub fn track(pid: u32) -> ProcessTree {
    #[cfg(target_os = "macos")]
    { ProcessTree { macos: MacProcessTree::new(pid) } }
    #[cfg(windows)]
    {
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let seen_bg = seen.clone();
        let stop_bg = stop.clone();
        let root_created = process_created(pid);
        let watcher = std::thread::Builder::new().name("imates-proc-tree".into()).spawn(move || {
            while !stop_bg.load(std::sync::atomic::Ordering::Relaxed) {
                match (root_created, process_created(pid)) {
                    (Some(expected), Some(actual)) if expected == actual => {}
                    _ => break,
                }
                if let Ok(mut guard) = seen_bg.lock() {
                    for child in descendant_pids(pid) {
                        if guard.iter().any(|(existing, _)| *existing == child) { continue; }
                        if let Some(created) = process_created(child) { guard.push((child, created)); }
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        }).ok();
        ProcessTree { job: bind_job(pid), root: pid, root_created, seen, stop, watcher }
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    { let _ = pid; ProcessTree {} }
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MacProcessIdentity { pid: u32, started: (u64, u64) }

#[cfg(target_os = "macos")]
struct MacProcessInfo { identity: MacProcessIdentity, parent: u32, group: u32, zombie: bool }

#[cfg(target_os = "macos")]
fn mac_process_info_with_fallback(
    pid: u32,
    primary: impl FnOnce(u32) -> std::io::Result<Option<MacProcessInfo>>,
    fallback: impl FnOnce(u32) -> std::io::Result<Option<MacProcessInfo>>,
) -> std::io::Result<Option<MacProcessInfo>> {
    match primary(pid) {
        Err(error) if error.raw_os_error() == Some(libc::EPERM) => fallback(pid),
        result => result,
    }
}

#[cfg(target_os = "macos")]
fn mac_process_info(pid: u32) -> std::io::Result<Option<MacProcessInfo>> {
    mac_process_info_with_fallback(pid, mac_bsd_process_info, mac_sysctl_process_info)
}

#[cfg(target_os = "macos")]
fn mac_bsd_process_info(pid: u32) -> std::io::Result<Option<MacProcessInfo>> {
    let mut info = unsafe { std::mem::zeroed::<libc::proc_bsdinfo>() };
    let size = std::mem::size_of_val(&info) as i32;
    let read = unsafe { libc::proc_pidinfo(pid as i32, libc::PROC_PIDTBSDINFO, 0, &mut info as *mut _ as *mut _, size) };
    if read == size {
        return Ok(Some(MacProcessInfo {
            identity: MacProcessIdentity { pid: info.pbi_pid, started: (info.pbi_start_tvsec, info.pbi_start_tvusec) },
            parent: info.pbi_ppid, group: info.pbi_pgid, zombie: info.pbi_status == libc::SZOMB,
        }));
    }
    let error = std::io::Error::last_os_error();
    if read == 0 && error.raw_os_error() == Some(libc::ESRCH) { Ok(None) } else { Err(error) }
}

// 公开 SDK sys/proc.h、sys/sysctl.h 的 64 位 kinfo_proc ABI 视图。
// libc 未绑定此结构；未读取的字段保留原位，不把其他 BSD 的结构误用于 macOS。
#[cfg(all(target_os = "macos", target_pointer_width = "64"))]
#[repr(C)]
struct MacKinfoProc {
    started: libc::timeval,
    _before_status: [u8; 20],
    status: i8,
    pid: i32,
    _before_parent: [u8; 516],
    parent: i32,
    group: i32,
    _tail: [u8; 80],
}

#[cfg(all(target_os = "macos", target_pointer_width = "64"))]
const _: () = {
    assert!(std::mem::size_of::<MacKinfoProc>() == 648);
    assert!(std::mem::offset_of!(MacKinfoProc, started) == 0);
    assert!(std::mem::offset_of!(MacKinfoProc, status) == 36);
    assert!(std::mem::offset_of!(MacKinfoProc, pid) == 40);
    assert!(std::mem::offset_of!(MacKinfoProc, parent) == 560);
    assert!(std::mem::offset_of!(MacKinfoProc, group) == 564);
};

#[cfg(all(target_os = "macos", target_pointer_width = "64"))]
fn mac_kinfo_process_info(pid: u32, info: &MacKinfoProc, size: usize) -> std::io::Result<Option<MacProcessInfo>> {
    // 零长度只表示本次未列到目标，调用方还须确认 ESRCH；短读、错 PID 和无效字段失败关闭。
    if size == 0 { return Ok(None); }
    let invalid = || std::io::Error::new(std::io::ErrorKind::InvalidData, "系统进程信息不完整或身份字段无效");
    if size != std::mem::size_of::<MacKinfoProc>() || info.pid <= 0 || info.pid as u32 != pid
        || info.started.tv_sec <= 0 || !(0..1_000_000).contains(&info.started.tv_usec)
        || info.parent < 0 || info.group <= 0 || !(libc::SIDL..=libc::SZOMB).contains(&(info.status as u32)) {
        return Err(invalid());
    }
    Ok(Some(MacProcessInfo {
        identity: MacProcessIdentity { pid, started: (info.started.tv_sec as u64, info.started.tv_usec as u64) },
        parent: info.parent as u32, group: info.group as u32, zombie: info.status as u32 == libc::SZOMB,
    }))
}

#[cfg(target_os = "macos")]
fn mac_empty_process_snapshot(probe: std::io::Result<()>) -> std::io::Result<Option<MacProcessInfo>> {
    match probe {
        Err(error) if error.raw_os_error() == Some(libc::ESRCH) => Ok(None),
        Err(error) => Err(error),
        Ok(()) => Err(std::io::Error::new(std::io::ErrorKind::WouldBlock, "系统进程快照未覆盖仍存在的目标")),
    }
}

#[cfg(target_os = "macos")]
fn mac_sysctl_process_info(pid: u32) -> std::io::Result<Option<MacProcessInfo>> {
    #[cfg(target_pointer_width = "64")]
    {
        let pid_value = i32::try_from(pid).ok().filter(|value| *value > 0)
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "系统进程编号无效"))?;
        let mut mib = [libc::CTL_KERN, libc::KERN_PROC, libc::KERN_PROC_PID, pid_value];
        let mut info = unsafe { std::mem::zeroed::<MacKinfoProc>() };
        let mut size = std::mem::size_of_val(&info);
        let result = unsafe { libc::sysctl(mib.as_mut_ptr(), mib.len() as u32, &mut info as *mut _ as *mut _, &mut size,
            std::ptr::null_mut(), 0) };
        if result != 0 { return Err(std::io::Error::last_os_error()); }
        if size == 0 {
            // sysctl 会略过创建中的进程；零信号只探测存在性，不能把暂时漏列误当已退出。
            let exists = unsafe { libc::kill(pid_value, 0) };
            return mac_empty_process_snapshot(if exists == 0 { Ok(()) } else { Err(std::io::Error::last_os_error()) });
        }
        // 两种接口都从内核 p_start 取秒与微秒；补证不改变既有身份与前后关系复核。
        mac_kinfo_process_info(pid, &info, size)
    }
    #[cfg(not(target_pointer_width = "64"))]
    { let _ = pid; Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "当前架构未核对系统进程信息布局")) }
}

#[cfg(target_os = "macos")]
fn mac_process_list_count(query: impl FnOnce() -> i32) -> std::io::Result<usize> {
    // libproc 把底层 -1 转成 0 并保留 errno；必须区分真实失败与合法空表，且不能读取旧 errno。
    unsafe { *libc::__error() = 0; }
    let found = query();
    let error = unsafe { *libc::__error() };
    if found < 0 || (found == 0 && error != 0) {
        return Err(if error != 0 { std::io::Error::from_raw_os_error(error) }
            else { std::io::Error::other("系统进程列表返回无效结果") });
    }
    Ok(found as usize)
}

#[cfg(target_os = "macos")]
fn mac_matching_process(identity: MacProcessIdentity) -> std::io::Result<Option<MacProcessInfo>> {
    Ok(mac_process_info(identity.pid)?.filter(|info| info.identity == identity && !info.zombie))
}

#[cfg(target_os = "macos")]
fn mac_process_ids(owner: u32, children: bool) -> std::io::Result<Vec<u32>> {
    let list = |buffer, size| unsafe {
        if children { libc::proc_listchildpids(owner as i32, buffer, size) }
        else { libc::proc_listpgrppids(owner as i32, buffer, size) }
    };
    // 这两个 libproc 包装器返回 PID 个数，不是缓冲区字节数。
    let capacity = mac_process_list_count(|| list(std::ptr::null_mut(), 0))?;
    let mut pids = vec![0i32; capacity + 32];
    for _ in 0..3 {
        let found = mac_process_list_count(|| list(pids.as_mut_ptr() as *mut _, std::mem::size_of_val(pids.as_slice()) as i32))?;
        if found < pids.len() {
            pids.truncate(found);
            return Ok(pids.into_iter().filter(|pid| *pid > 0).map(|pid| pid as u32).collect());
        }
        pids.resize(pids.len() * 2, 0);
    }
    Err(std::io::Error::other("进程表持续增长，无法完整观察后代"))
}

#[cfg(target_os = "macos")]
struct MacObservedTree {
    root: u32, root_confirmed: bool, seen: Vec<MacProcessIdentity>, complete: bool,
    first_error: Option<(&'static str, Option<i32>)>, last_cleanup: Option<serde_json::Value>,
}

#[cfg(target_os = "macos")]
impl MacObservedTree {
    fn incomplete(&mut self, stage: &'static str, error: Option<i32>) {
        self.complete = false;
        self.first_error.get_or_insert((stage, error));
    }

    fn observe_waitable_root(&mut self, root: u32) {
        // libproc 不返回 zombie 的 BSD 信息；WNOWAIT 确认原 child 尚未回收，PID 不能复用。
        if self.root != root || !matches!(mac_root_waitable(root), Ok(true)) {
            self.incomplete("waitable_root", None); return;
        }
        let members = match mac_process_ids(root, false) {
            Ok(members) => members,
            Err(error) => { self.incomplete("waitable_group_list", error.raw_os_error()); return; }
        };
        let mut observed = Vec::new();
        for pid in members {
            match mac_process_info(pid) {
                Ok(Some(info)) if info.group == root && !info.zombie => observed.push(info.identity),
                Ok(_) => {}
                Err(error) => { self.incomplete("waitable_group_identity", error.raw_os_error()); }
            }
        }
        if !matches!(mac_root_waitable(root), Ok(true)) { self.incomplete("waitable_root_recheck", None); return; }
        self.root_confirmed = true;
        for identity in observed { if !self.seen.contains(&identity) { self.seen.push(identity); } }
        self.observe();
    }

    fn observe_root_group(&mut self, root: u32) {
        let recovering = !self.root_confirmed;
        // 初次 ESRCH 后，只能用尚未回收的原 child 归属补证，不能仅凭旧 PID 接受新进程。
        if recovering && !matches!(mac_root_waitable(root), Ok(false)) { return; }
        let anchor = match mac_process_info(root) {
            Ok(Some(info)) if info.group == root && (recovering || self.seen.contains(&info.identity)) => info.identity,
            Ok(_) => return,
            Err(error) => { self.incomplete("root_identity", error.raw_os_error()); return; }
        };
        let members = match mac_process_ids(root, false) {
            Ok(members) => members,
            Err(error) => { self.incomplete("root_group_list", error.raw_os_error()); return; }
        };
        let mut observed = Vec::new();
        for pid in members {
            match mac_process_info(pid) {
                Ok(Some(info)) if info.group == root && !info.zombie => observed.push(info.identity),
                Ok(_) => {}
                Err(error) => { self.incomplete("root_group_identity", error.raw_os_error()); }
            }
        }
        // 活根的启动身份与组号必须在成员身份读取前后都一致；已退出根另走 WNOWAIT。
        if !mac_process_info(root).is_ok_and(|info| info.is_some_and(|info| info.identity == anchor && info.group == root)) { return; }
        if recovering {
            if !matches!(mac_root_waitable(root), Ok(false)) { return; }
            self.root_confirmed = true;
            if !self.seen.contains(&anchor) { self.seen.push(anchor); }
        }
        for identity in observed { if !self.seen.contains(&identity) { self.seen.push(identity); } }
    }

    fn observe(&mut self) {
        let mut pending = self.seen.clone();
        while let Some(parent) = pending.pop() {
            match mac_matching_process(parent) {
                Ok(Some(_)) => {}
                Ok(None) => continue,
                Err(error) => { self.incomplete("parent_identity", error.raw_os_error()); continue; }
            }
            let children = match mac_process_ids(parent.pid, true) {
                Ok(children) => children,
                Err(error) if error.raw_os_error() == Some(libc::ESRCH) => continue,
                Err(error) => { self.incomplete("children_list", error.raw_os_error()); continue; }
            };
            let mut observed = Vec::new();
            for pid in children {
                match mac_process_info(pid) {
                    Ok(Some(info)) if info.parent == parent.pid && !info.zombie => observed.push(info.identity),
                    Ok(_) => {}
                    Err(error) => { self.incomplete("child_identity", error.raw_os_error()); }
                }
            }
            match mac_matching_process(parent) {
                Ok(Some(_)) => {}
                Ok(None) => continue,
                Err(error) => { self.incomplete("parent_identity_recheck", error.raw_os_error()); continue; }
            }
            for identity in observed {
                if !self.seen.contains(&identity) { self.seen.push(identity); pending.push(identity); }
            }
        }
    }

    fn owns_group(&self, group: u32) -> bool {
        self.seen.iter().any(|identity| mac_matching_process(*identity).is_ok_and(|info| info.is_some_and(|info| info.group == group)))
    }

    fn signal(&mut self, group: u32, signal: i32) {
        self.observe_root_group(group);
        self.observe();
        // 根 PID/PGID 已复用或整组已消失时，不凭旧的组号发送信号。
        if self.owns_group(group) { unsafe { libc::kill(-(group as i32), signal); } }
        for identity in &self.seen {
            // 过继后仍按已记录的微秒级启动时间核对，不向复用的 PID 发信号。
            if matches!(mac_matching_process(*identity), Ok(Some(_))) { unsafe { libc::kill(identity.pid as i32, signal); } }
        }
    }

    fn gone(&mut self, group: u32, reaped: bool) -> bool {
        self.observe();
        let (mut alive, mut identity_errors, mut group_alive, mut group_errors) = (0, 0, 0, 0);
        for identity in &self.seen {
            match mac_matching_process(*identity) {
                Ok(Some(_)) => alive += 1, Ok(None) => {}, Err(_) => identity_errors += 1,
            }
        }
        let group_listed = match mac_process_ids(group, false) {
            Ok(pids) => {
                for pid in pids {
                    match mac_process_info(pid) {
                        Ok(Some(info)) if !info.zombie => group_alive += 1,
                        Ok(_) => {}, Err(_) => group_errors += 1,
                    }
                }
                true
            }
            Err(_) => false,
        };
        let gone = self.root_confirmed && self.complete && alive == 0 && identity_errors == 0
            && group_listed && group_alive == 0 && group_errors == 0;
        // 只保存布尔值、计数与固定阶段／错误码，不记录 PID、路径、提示词或 CLI 原文。
        self.last_cleanup = Some(serde_json::json!({
            "type": "process_cleanup", "cleanupComplete": gone && reaped, "rootConfirmed": self.root_confirmed,
            "observationComplete": self.complete, "rootReaped": reaped, "trackedProcesses": self.seen.len(),
            "aliveProcesses": alive, "identityQueryErrors": identity_errors, "groupListed": group_listed,
            "groupAliveProcesses": group_alive, "groupIdentityErrors": group_errors,
            "firstObservationError": self.first_error.map(|(stage, error)| serde_json::json!({ "stage": stage, "osError": error })),
        }));
        gone
    }
}

#[cfg(target_os = "macos")]
struct MacProcessTree {
    observed: std::sync::Arc<std::sync::Mutex<MacObservedTree>>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    watcher: Option<std::thread::JoinHandle<()>>,
}

#[cfg(target_os = "macos")]
impl MacProcessTree {
    fn new(pid: u32) -> Self {
        let root = mac_process_info(pid);
        let complete = root.is_ok();
        let first_error = root.as_ref().err().map(|error| ("initial_root", error.raw_os_error()));
        let seen = root.ok().flatten().map(|info| vec![info.identity]).unwrap_or_default();
        let observed = std::sync::Arc::new(std::sync::Mutex::new(MacObservedTree {
            // 初次 ESRCH 可由未回收 child 的 WNOWAIT 补证；其他查询错误仍保持失败关闭。
            root: pid, root_confirmed: !seen.is_empty(), complete, seen, first_error, last_cleanup: None,
        }));
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (observed_bg, stop_bg) = (observed.clone(), stop.clone());
        let watcher = std::thread::Builder::new().name("imates-proc-tree".into()).spawn(move || {
            while !stop_bg.load(std::sync::atomic::Ordering::Relaxed) {
                if let Ok(mut tree) = observed_bg.lock() { tree.observe_root_group(pid); tree.observe(); }
                std::thread::sleep(Duration::from_millis(20));
            }
        }).ok();
        if watcher.is_none() { if let Ok(mut tree) = observed.lock() { tree.incomplete("observer_thread", None); } }
        Self { observed, stop, watcher }
    }

    async fn observe_before_reap(&self, root: u32) {
        let observed = self.observed.clone();
        let _ = tokio::task::spawn_blocking(move || {
            if let Ok(mut tree) = observed.lock() { tree.observe_waitable_root(root); }
        }).await;
    }

    async fn signal(&self, group: u32, signal: i32) {
        let observed = self.observed.clone();
        let _ = tokio::task::spawn_blocking(move || { if let Ok(mut tree) = observed.lock() { tree.signal(group, signal); } }).await;
    }

    async fn gone(&self, group: u32, reaped: bool) -> bool {
        let observed = self.observed.clone();
        tokio::task::spawn_blocking(move || observed.lock().is_ok_and(|mut tree| tree.gone(group, reaped))).await.unwrap_or(false)
    }
}

#[cfg(target_os = "macos")]
impl Drop for MacProcessTree {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(watcher) = self.watcher.take() { let _ = watcher.join(); }
    }
}

#[cfg(target_os = "macos")]
fn mac_root_waitable(root: u32) -> std::io::Result<bool> {
    let mut info = unsafe { std::mem::zeroed::<libc::siginfo_t>() };
    let result = unsafe { libc::waitid(libc::P_PID, root, &mut info, libc::WEXITED | libc::WNOHANG | libc::WNOWAIT) };
    if result == -1 { Err(std::io::Error::last_os_error()) } else { Ok(info.si_pid == root as i32) }
}

#[cfg(target_os = "macos")]
pub async fn try_wait(child: &mut Child, root: u32, tree: &ProcessTree) -> std::io::Result<Option<std::process::ExitStatus>> {
    if child.id().is_none() { return child.try_wait(); }
    // 运行时只做非阻塞退出探测，不让高频流事件等待整组快照。
    if !mac_root_waitable(root)? { return Ok(None); }
    tree.macos.observe_before_reap(root).await;
    child.try_wait()
}

/// 清理诊断只返回最后一次核对的匿名元信息，沿用 runner 的受限诊断文件。
pub fn cleanup_diagnostic(_tree: &ProcessTree) -> Option<String> {
    #[cfg(target_os = "macos")]
    { _tree.macos.observed.lock().ok()?.last_cleanup.as_ref().map(serde_json::Value::to_string) }
    #[cfg(not(target_os = "macos"))]
    { None }
}

#[cfg(windows)]
fn taskkill_tree(pid: u32) {
    let _ = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

#[cfg(windows)]
fn process_created(pid: u32) -> Option<u64> {
    use std::ffi::c_void;
    #[repr(C)]
    struct FileTime { low: u32, high: u32 }
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
        fn GetProcessTimes(process: *mut c_void, created: *mut FileTime, exited: *mut FileTime, kernel: *mut FileTime, user: *mut FileTime) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }
    unsafe {
        let process = OpenProcess(0x1000, 0, pid);
        if process.is_null() { return None; }
        let mut created = std::mem::zeroed();
        let mut exited = std::mem::zeroed();
        let mut kernel = std::mem::zeroed();
        let mut user = std::mem::zeroed();
        let ok = GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user) != 0;
        CloseHandle(process);
        if !ok { return None; }
        Some(((created.high as u64) << 32) | created.low as u64)
    }
}

#[cfg(windows)]
fn descendant_pids(root: u32) -> Vec<u32> {
    use std::ffi::c_void;
    #[repr(C)]
    struct ProcessEntry { size: u32, cnt_usage: u32, process_id: u32, default_heap_id: usize, module_id: u32,
        threads: u32, parent_process_id: u32, pri_class_base: i32, flags: u32, exe_file: [u16; 260] }
    unsafe extern "system" {
        fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> *mut c_void;
        fn Process32FirstW(snap: *mut c_void, entry: *mut ProcessEntry) -> i32;
        fn Process32NextW(snap: *mut c_void, entry: *mut ProcessEntry) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }
    let snap = unsafe { CreateToolhelp32Snapshot(0x00000002, 0) };
    if snap.is_null() || snap as isize == -1 { return Vec::new(); }
    let mut rows = Vec::new();
    unsafe {
        let mut entry = std::mem::zeroed::<ProcessEntry>();
        entry.size = std::mem::size_of::<ProcessEntry>() as u32;
        if Process32FirstW(snap, &mut entry) != 0 {
            loop {
                rows.push((entry.process_id, entry.parent_process_id));
                entry.size = std::mem::size_of::<ProcessEntry>() as u32;
                if Process32NextW(snap, &mut entry) == 0 { break; }
            }
        }
        CloseHandle(snap);
    }
    let mut pending = vec![root];
    let mut found = Vec::new();
    while let Some(parent) = pending.pop() {
        for (pid, ppid) in &rows {
            if *ppid == parent && *pid != parent { found.push(*pid); pending.push(*pid); }
        }
    }
    found
}

#[cfg(windows)]
fn bind_job(pid: u32) -> isize {
    use std::ffi::c_void;
    const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x2000;
    const JOB_OBJECT_BASIC_LIMIT_INFORMATION: i32 = 2;
    const PROCESS_SET_QUOTA: u32 = 0x0100;
    const PROCESS_TERMINATE: u32 = 0x0001;
    #[repr(C)]
    struct BasicLimit {
        per_process_user_time_limit: i64,
        per_job_user_time_limit: i64,
        limit_flags: u32,
        _pad0: u32,
        minimum_working_set_size: usize,
        maximum_working_set_size: usize,
        active_process_limit: u32,
        _pad1: u32,
        affinity: usize,
        priority_class: u32,
        scheduling_class: u32,
    }
    unsafe extern "system" {
        fn CreateJobObjectW(attrs: *mut c_void, name: *const u16) -> *mut c_void;
        fn SetInformationJobObject(job: *mut c_void, class: i32, info: *const c_void, len: u32) -> i32;
        fn AssignProcessToJobObject(job: *mut c_void, process: *mut c_void) -> i32;
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }
    unsafe {
        let job = CreateJobObjectW(std::ptr::null_mut(), std::ptr::null());
        if job.is_null() { return 0; }
        let mut info = std::mem::zeroed::<BasicLimit>();
        info.limit_flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if SetInformationJobObject(job, JOB_OBJECT_BASIC_LIMIT_INFORMATION, &info as *const BasicLimit as *const c_void, std::mem::size_of::<BasicLimit>() as u32) == 0 {
            CloseHandle(job);
            return 0;
        }
        let process = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid);
        if process.is_null() || AssignProcessToJobObject(job, process) == 0 {
            if !process.is_null() { CloseHandle(process); }
            CloseHandle(job);
            return 0;
        }
        CloseHandle(process);
        job as isize
    }
}

pub fn configure(command: &mut Command) {
    #[cfg(unix)]
    { use std::os::unix::process::CommandExt; command.as_std_mut().process_group(0); }
    #[cfg(windows)]
    { command.creation_flags(0x0800_0000); }
    command.kill_on_drop(true);
}

/// 即使父进程已经退出，也清理同一进程组中的后代。
pub async fn terminate_tree(child: &mut Child, pid: u32, _tree: &ProcessTree) -> bool {
    #[cfg(target_os = "macos")]
    {
        if matches!(mac_root_waitable(pid), Ok(true)) { _tree.macos.observe_before_reap(pid).await; }
        _tree.macos.signal(pid, libc::SIGTERM).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        _tree.macos.signal(pid, libc::SIGKILL).await;
        let _ = child.start_kill();
        let reaped = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                // 取消期间退出的根也必须先补原组快照，再回收；Done 由 try_wait 返回缓存。
                if let Some(status) = try_wait(child, pid, _tree).await? { return Ok::<_, std::io::Error>(status); }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await.is_ok_and(|result| result.is_ok());
        let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
        loop {
            if _tree.macos.gone(pid, reaped).await { return reaped; }
            if tokio::time::Instant::now() >= deadline { return false; }
            _tree.macos.signal(pid, libc::SIGKILL).await;
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let signal = |sig| {
            let rc = unsafe { libc::kill(-(pid as i32), sig) };
            if rc == 0 { true } else {
                std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
            }
        };
        let term_ok = signal(libc::SIGTERM);
        // 宽限内持续等待，不因父进程先退出而漏掉忽略 TERM 的后代。
        tokio::time::sleep(Duration::from_millis(100)).await;
        let kill_ok = signal(libc::SIGKILL);
        // macOS 对已经消失的进程组也可能返回 EPERM；用独立的进程表核对。
        let group_gone = if kill_ok { true } else {
            let mut probe = Command::new("/bin/ps");
            probe.args(["-axo", "pgid=,stat="]).stdin(std::process::Stdio::null());
            tokio::time::timeout(Duration::from_secs(1), probe.output()).await
                .is_ok_and(|result| result.is_ok_and(|output| {
                    output.status.success() && !String::from_utf8_lossy(&output.stdout).lines().any(|line| {
                        let mut fields = line.split_whitespace();
                        fields.next().and_then(|value| value.parse::<u32>().ok()) == Some(pid)
                            && fields.next().is_some_and(|state| !state.starts_with('Z'))
                    })
                }))
        };
        let _ = child.start_kill();
        let reaped = tokio::time::timeout(Duration::from_secs(2), child.wait()).await.is_ok();
        (term_ok || group_gone) && group_gone && reaped
    }
    #[cfg(windows)]
    {
        let mut command = Command::new("taskkill");
        command.args(["/PID", &pid.to_string(), "/T", "/F"]).creation_flags(0x0800_0000);
        command.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        let killed = tokio::time::timeout(Duration::from_secs(3), command.status()).await
            .is_ok_and(|result| result.is_ok_and(|status| status.success() || status.code() == Some(128)));
        let _ = child.start_kill();
        killed && tokio::time::timeout(Duration::from_secs(2), child.wait()).await.is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[cfg(target_os = "macos")]
    fn macos_eperm_fallback_keeps_the_same_microsecond_identity() {
        let original = mac_process_info(std::process::id()).unwrap().unwrap();
        let expected = original.identity;
        let mut fallback_called = false;
        let recovered = mac_process_info_with_fallback(expected.pid,
            |_| Err(std::io::Error::from_raw_os_error(libc::EPERM)),
            |pid| { fallback_called = true; assert_eq!(pid, expected.pid); Ok(Some(original)) },
        ).expect("EPERM 必须可由独立的公共进程信息补证").unwrap();
        assert!(fallback_called);
        assert_eq!(recovered.identity, expected, "补证后仍使用同一 PID 与微秒级启动时间");
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn macos_eperm_fallback_preserves_real_errors_and_exited_results() {
        let pid = std::process::id();
        let failed = mac_process_info_with_fallback(pid,
            |_| Err(std::io::Error::from_raw_os_error(libc::EPERM)),
            |_| Err(std::io::Error::from_raw_os_error(libc::EIO)),
        );
        assert_eq!(failed.err().unwrap().raw_os_error(), Some(libc::EIO), "真实补证失败不能当成进程退出");
        assert!(mac_process_info_with_fallback(pid,
            |_| Err(std::io::Error::from_raw_os_error(libc::EPERM)), |_| Ok(None),
        ).unwrap().is_none(), "补证确认无目标时才是已退出");
        let mut fallback_called = false;
        let failed = mac_process_info_with_fallback(pid,
            |_| Err(std::io::Error::from_raw_os_error(libc::EACCES)),
            |_| { fallback_called = true; Ok(None) },
        );
        assert_eq!(failed.err().unwrap().raw_os_error(), Some(libc::EACCES));
        assert!(!fallback_called, "只为完整 BSD 查询的 EPERM 启用补证");
        let mut unknown = MacObservedTree { root: pid, root_confirmed: true, seen: Vec::new(), complete: true,
            first_error: None, last_cleanup: None };
        unknown.incomplete("child_identity", Some(libc::EIO));
        assert!(!unknown.gone(0, true), "真实观察错误仍须阻止宣称清理完成");
    }

    #[test]
    #[cfg(all(target_os = "macos", target_pointer_width = "64"))]
    fn macos_sysctl_identity_matches_bsd_and_rejects_pid_reuse() {
        let pid = std::process::id();
        let full = mac_bsd_process_info(pid).unwrap().unwrap();
        let fallback = mac_process_info_with_fallback(pid,
            |_| Err(std::io::Error::from_raw_os_error(libc::EPERM)), mac_sysctl_process_info,
        ).unwrap().unwrap();
        assert_eq!(fallback.identity, full.identity, "公共补证和完整查询必须逐微秒指向同一进程");
        assert_eq!((fallback.parent, fallback.group, fallback.zombie), (full.parent, full.group, full.zombie));
        let stale = MacProcessIdentity { started: (full.identity.started.0, full.identity.started.1.wrapping_add(1)), ..full.identity };
        assert!(mac_matching_process(fallback.identity).unwrap().is_some());
        assert!(mac_matching_process(stale).unwrap().is_none(), "补证后仍不可把同 PID 的其他启动身份认作原进程");
    }

    #[test]
    #[cfg(all(target_os = "macos", target_pointer_width = "64"))]
    fn macos_sysctl_reads_public_identity_when_full_bsd_is_restricted() {
        // launchd 归系统用户；普通本地测试进程的完整 BSD 查询会遇到本次真实的 EPERM。
        let expected = mac_sysctl_process_info(1).unwrap().unwrap();
        let actual = mac_process_info(1).unwrap().unwrap();
        assert_eq!(actual.identity, expected.identity);
        assert_eq!((actual.parent, actual.group, actual.zombie), (expected.parent, expected.group, expected.zombie));
        assert!(actual.identity.started.0 > 0);
    }

    #[test]
    #[cfg(all(target_os = "macos", target_pointer_width = "64"))]
    fn macos_sysctl_rejects_short_reads_and_invalid_identity_fields() {
        let mut info = unsafe { std::mem::zeroed::<MacKinfoProc>() };
        info.started.tv_sec = 123; info.started.tv_usec = 456;
        info.status = libc::SRUN as i8; info.pid = 1234; info.parent = 1; info.group = 1234;
        let size = std::mem::size_of_val(&info);
        assert_eq!(mac_kinfo_process_info(1234, &info, size).unwrap().unwrap().identity.started, (123, 456));
        assert!(mac_kinfo_process_info(1234, &info, 0).unwrap().is_none(), "零长度保留为无记录，再由调用方补证进程已退出");
        assert!(mac_kinfo_process_info(1234, &info, size - 1).is_err());
        assert!(mac_kinfo_process_info(1235, &info, size).is_err());
        info.started.tv_sec = 0; assert!(mac_kinfo_process_info(1234, &info, size).is_err());
        info.started.tv_sec = 123;
        for usec in [-1, 1_000_000] {
            info.started.tv_usec = usec; assert!(mac_kinfo_process_info(1234, &info, size).is_err());
        }
        info.started.tv_usec = 456;
        info.parent = -1; assert!(mac_kinfo_process_info(1234, &info, size).is_err());
        info.parent = 1; info.group = 0; assert!(mac_kinfo_process_info(1234, &info, size).is_err());
        info.group = 1234; info.status = 0; assert!(mac_kinfo_process_info(1234, &info, size).is_err());
        info.status = libc::SZOMB as i8;
        assert!(mac_kinfo_process_info(1234, &info, size).unwrap().unwrap().zombie);
    }

    #[test]
    #[cfg(all(target_os = "macos", target_pointer_width = "64"))]
    fn macos_sysctl_confirms_exited_child_with_zero_length() {
        let mut child = std::process::Command::new("/usr/bin/true").spawn().unwrap();
        let pid = child.id();
        assert!(child.wait().unwrap().success());
        assert!(mac_sysctl_process_info(pid).unwrap().is_none(), "已回收的测试进程应由零长度结果确认消失");
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn macos_empty_sysctl_snapshot_requires_independent_esrch() {
        assert!(mac_empty_process_snapshot(Err(std::io::Error::from_raw_os_error(libc::ESRCH))).unwrap().is_none());
        assert!(mac_empty_process_snapshot(Ok(())).is_err(), "零长度不能覆盖仍存在的创建中进程");
        for error in [libc::EPERM, libc::EACCES, libc::EIO] {
            assert_eq!(mac_empty_process_snapshot(Err(std::io::Error::from_raw_os_error(error))).err().unwrap().raw_os_error(), Some(error));
        }
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn macos_process_list_distinguishes_wrapper_errors_from_empty_results() {
        for code in [libc::EPERM, libc::EIO] {
            let result = mac_process_list_count(|| { unsafe { *libc::__error() = code; } 0 });
            assert_eq!(result.err().expect("libproc 的零返回值带真实 errno 时不能算空表").raw_os_error(), Some(code));
        }
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn macos_process_list_does_not_reuse_stale_errno() {
        unsafe { *libc::__error() = libc::EPERM; }
        assert_eq!(mac_process_list_count(|| 0).unwrap(), 0, "合法空表不得继承上一次系统调用的 errno");
        assert_eq!(unsafe { *libc::__error() }, 0, "调用前必须清除旧 errno");
        assert_eq!(mac_process_list_count(|| 3).unwrap(), 3);
    }

    #[tokio::test]
    #[cfg(target_os = "macos")]
    async fn macos_pending_root_cancel_can_confirm_cleanup_after_the_pid_disappears() {
        let mut command = Command::new("/bin/sleep");
        command.arg("2");
        configure(&mut command);
        let mut child = command.spawn().unwrap();
        let root = child.id().unwrap();
        assert!(!mac_root_waitable(root).unwrap());
        let tree = track(root);
        {
            // 确定性重建初次 libproc 返回 ESRCH、尚待 child 归属补证的状态。
            let mut observed = tree.macos.observed.lock().unwrap();
            observed.seen.clear(); observed.root_confirmed = false;
            assert!(observed.complete);
        }
        let cleaned = terminate_tree(&mut child, root, &tree).await;
        assert!(mac_process_info(root).unwrap().is_none(), "隔离根进程已经消失");
        assert!(cleaned, "不能在取消时绕过 pending root 的归属补证而误报清理未完成");
    }

    #[tokio::test]
    #[cfg(target_os = "macos")]
    async fn macos_pending_root_recovers_detached_tracking_but_keeps_real_observation_errors() {
        use tokio::io::AsyncWriteExt;
        for observation_failed in [false, true] {
            let folder = tempfile::tempdir().unwrap();
            let mut command = Command::new(std::env::current_exe().unwrap());
            command.args(["--ignored", "--exact", "runner::tests::subprocess_helper", "--nocapture"])
                .current_dir(folder.path()).stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
            configure(&mut command);
            let mut child = command.spawn().unwrap();
            let root = child.id().unwrap();
            let tree = track(root);
            {
                let mut observed = tree.macos.observed.lock().unwrap();
                observed.seen.clear(); observed.root_confirmed = false;
                // 首次 ESRCH 可补证；真正的观察错误不能被后续成功快照抹掉。
                observed.complete = !observation_failed;
                observed.first_error = observation_failed.then_some(("initial_root", Some(libc::EPERM)));
            }
            let mut input = child.stdin.take().unwrap();
            input.write_all(b"detached_tree\n").await.unwrap(); input.shutdown().await.unwrap(); drop(input);
            let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
            let tracked_detached = loop {
                let recovered = {
                    let observed = tree.macos.observed.lock().unwrap();
                    let ready = std::fs::read_to_string(folder.path().join("pids")).is_ok_and(|pids| pids.lines().count() >= 2);
                    ready && observed.root_confirmed && observed.seen.iter().any(|identity|
                        mac_matching_process(*identity).is_ok_and(|info| info.is_some_and(|info| info.group != root)))
                };
                if recovered || tokio::time::Instant::now() >= deadline { break recovered; }
                tokio::time::sleep(Duration::from_millis(5)).await;
            };
            let cleaned = terminate_tree(&mut child, root, &tree).await;
            assert!(tracked_detached, "活根身份恢复后必须重新记录独立进程组后代");
            assert_eq!(cleaned, !observation_failed, "真实观察错误仍须失败关闭");
            let diagnostic: serde_json::Value = serde_json::from_str(&cleanup_diagnostic(&tree).unwrap()).unwrap();
            assert_eq!(diagnostic["rootConfirmed"], true);
            assert_eq!(diagnostic["observationComplete"], !observation_failed);
            assert_eq!(diagnostic["rootReaped"], true);
            assert_eq!(diagnostic["aliveProcesses"], 0);
            assert_eq!(diagnostic["groupAliveProcesses"], 0);
            if observation_failed {
                assert_eq!(diagnostic["firstObservationError"]["stage"], "initial_root");
                assert_eq!(diagnostic["firstObservationError"]["osError"], libc::EPERM);
            }
            for line in std::fs::read_to_string(folder.path().join("pids")).unwrap().lines() {
                assert!(mac_process_info(line.parse().unwrap()).unwrap().is_none(), "已记录测试后代必须清理");
            }
        }
    }

    #[tokio::test]
    #[cfg(target_os = "macos")]
    async fn macos_root_already_waitable_before_tracking_can_confirm_cleanup() {
        let mut command = Command::new("/usr/bin/true");
        configure(&mut command);
        let mut child = command.spawn().unwrap();
        let root = child.id().unwrap();
        tokio::time::timeout(Duration::from_secs(1), async {
            while !mac_root_waitable(root).unwrap() { tokio::time::sleep(Duration::from_millis(5)).await; }
        }).await.unwrap();
        assert!(mac_process_info(root).unwrap().is_none(), "macOS 不向 libproc 暴露已退出但尚未回收的根");
        let tree = track(root);
        assert!(try_wait(&mut child, root, &tree).await.unwrap().unwrap().success());
        assert!(terminate_tree(&mut child, root, &tree).await, "回收前的 WNOWAIT 归属核对必须允许快速成功根完成清理");
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn macos_pid_identity_rejects_reuse_and_incomplete_observation_cannot_claim_cleanup() {
        let identity = mac_process_info(std::process::id()).unwrap().unwrap().identity;
        assert!(mac_matching_process(identity).unwrap().is_some());
        let stale = MacProcessIdentity { started: (identity.started.0, identity.started.1.wrapping_add(1)), ..identity };
        assert!(mac_matching_process(stale).unwrap().is_none(), "同一 PID 的启动时间不同就不是原进程");
        let group = unsafe { libc::getpgrp() } as u32;
        let stale_tree = MacObservedTree { root: identity.pid, root_confirmed: true, seen: vec![stale], complete: true,
            first_error: None, last_cleanup: None };
        assert!(!stale_tree.owns_group(group), "不能仅凭复用 PID 或组号确认进程组归属");
        let current_tree = MacObservedTree { root: identity.pid, root_confirmed: true, seen: vec![identity], complete: true,
            first_error: None, last_cleanup: None };
        assert!(current_tree.owns_group(group));
        let mut unknown = MacObservedTree { root: identity.pid, root_confirmed: false, seen: Vec::new(), complete: false,
            first_error: None, last_cleanup: None };
        assert!(!unknown.gone(0, false), "观察失败时不能把空列表当作清理完成");
    }

    #[test]
    fn npm_shim_resolution_never_constructs_shell_arguments() {
        let temp = tempfile::tempdir().unwrap();
        let parent = temp.path();
        let script = parent.join("node_modules/@openai/codex/bin/codex.js");
        std::fs::create_dir_all(script.parent().unwrap()).unwrap();
        std::fs::write(&script, "").unwrap();
        std::fs::write(parent.join("node.exe"), "").unwrap();
        let shim = parent.join("codex.cmd");
        std::fs::write(&shim, "\"%_prog%\" \"%dp0%\\node_modules\\@openai\\codex\\bin\\codex.js\" %*").unwrap();
        let args = vec!["exec".into(), "a & echo secret".into()];
        let (program, argv) = resolve_program(&shim, &args, None).unwrap();
        assert_eq!(program, parent.join("node.exe"));
        assert_eq!(argv[0], script.to_string_lossy());
        assert_eq!(&argv[1..], args);
        std::fs::write(&shim, "echo 不认识的脚本").unwrap();
        assert!(resolve_program(&shim, &args, None).is_err());
    }

    #[test]
    fn claude_native_shim_resolves_to_the_executable_without_a_shell() {
        let temp = tempfile::tempdir().unwrap();
        let parent = temp.path();
        let exe = parent.join("node_modules/@anthropic-ai/claude-code/bin/claude.exe");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, "").unwrap();
        let shim = parent.join("claude.cmd");
        std::fs::write(&shim, "\"%dp0%\\node_modules\\@anthropic-ai\\claude-code\\bin\\claude.exe\"   %*").unwrap();
        let args = vec!["-p".into(), "a & calc.exe".into()];
        let (program, argv) = resolve_program(&shim, &args, None).unwrap();
        assert_eq!(program, exe);
        assert_eq!(argv, args);
        let legacy = parent.join("node_modules/@anthropic-ai/claude-code/cli.js");
        std::fs::write(&legacy, "").unwrap();
        std::fs::write(parent.join("node.exe"), "").unwrap();
        std::fs::write(&shim, "\"%_prog%\" \"%dp0%\\node_modules\\@anthropic-ai\\claude-code\\cli.js\" %*").unwrap();
        let (program, argv) = resolve_program(&shim, &args, None).unwrap();
        assert_eq!(program, parent.join("node.exe"));
        assert_eq!(argv[0], legacy.to_string_lossy());
        assert_eq!(&argv[1..], args);
    }
    #[test]
    fn dsh_direct_node_shim_is_parsed_without_a_shell() {
        let temp = tempfile::tempdir().unwrap();
        let node = temp.path().join("node.exe");
        let script = temp.path().join("apps").join("cli").join("lib").join("bin.js");
        std::fs::create_dir_all(script.parent().unwrap()).unwrap();
        std::fs::write(&node, "").unwrap();
        std::fs::write(&script, "").unwrap();
        let shim = temp.path().join("dsh.cmd");
        let line = format!("\"{}\" \"{}\" %*", node.display(), script.display());
        std::fs::write(&shim, format!("@echo off\r\n{line}\r\n")).unwrap();
        let args = vec!["headless".to_string(), "a & echo secret".to_string()];
        let (program, argv) = resolve_program(&shim, &args, None).unwrap();
        assert_eq!(program, node);
        assert_eq!(argv[0], script.to_string_lossy());
        assert_eq!(&argv[1..], args);

        let other = temp.path().join("python.exe");
        std::fs::write(&other, "").unwrap();
        for bad in [
            format!("{line} & calc.exe"),
            format!("\"{}\" \"{}\" %*", node.display(), temp.path().join("不存在.js").display()),
            format!("\"{}\" \"{}\" %*", other.display(), script.display()),
            "\"node.exe\" \"bin.js\" %*".to_string(),
            format!("{} {} %*", node.display(), script.display()),
            "echo 不认识的脚本".to_string(),
        ] {
            std::fs::write(&shim, format!("@echo off\r\n{bad}\r\n")).unwrap();
            assert!(resolve_program(&shim, &args, None).is_err(), "应拒绝：{bad}");
        }
    }

}
