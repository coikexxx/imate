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
fn mac_process_info(pid: u32) -> std::io::Result<Option<MacProcessInfo>> {
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
    let capacity = list(std::ptr::null_mut(), 0);
    if capacity < 0 { return Err(std::io::Error::last_os_error()); }
    let mut pids = vec![0i32; capacity as usize + 32];
    for _ in 0..3 {
        let found = list(pids.as_mut_ptr() as *mut _, std::mem::size_of_val(pids.as_slice()) as i32);
        if found < 0 { return Err(std::io::Error::last_os_error()); }
        if (found as usize) < pids.len() {
            pids.truncate(found as usize);
            return Ok(pids.into_iter().filter(|pid| *pid > 0).map(|pid| pid as u32).collect());
        }
        pids.resize(pids.len() * 2, 0);
    }
    Err(std::io::Error::other("进程表持续增长，无法完整观察后代"))
}

#[cfg(target_os = "macos")]
struct MacObservedTree { root: u32, root_confirmed: bool, seen: Vec<MacProcessIdentity>, complete: bool }

#[cfg(target_os = "macos")]
impl MacObservedTree {
    fn observe_waitable_root(&mut self, root: u32) {
        // libproc 不返回 zombie 的 BSD 信息；WNOWAIT 确认原 child 尚未回收，PID 不能复用。
        if self.root != root || !matches!(mac_root_waitable(root), Ok(true)) {
            self.complete = false; return;
        }
        let members = match mac_process_ids(root, false) {
            Ok(members) => members,
            Err(_) => { self.complete = false; return; }
        };
        let mut observed = Vec::new();
        for pid in members {
            match mac_process_info(pid) {
                Ok(Some(info)) if info.group == root && !info.zombie => observed.push(info.identity),
                Ok(_) => {}
                Err(_) => { self.complete = false; }
            }
        }
        if !matches!(mac_root_waitable(root), Ok(true)) { self.complete = false; return; }
        self.root_confirmed = true;
        for identity in observed { if !self.seen.contains(&identity) { self.seen.push(identity); } }
        self.observe();
    }

    fn observe_root_group(&mut self, root: u32) {
        let anchor = match mac_process_info(root) {
            Ok(Some(info)) if info.group == root && self.seen.contains(&info.identity) => info.identity,
            Ok(_) => return,
            Err(_) => { self.complete = false; return; }
        };
        let members = match mac_process_ids(root, false) {
            Ok(members) => members,
            Err(_) => { self.complete = false; return; }
        };
        let mut observed = Vec::new();
        for pid in members {
            match mac_process_info(pid) {
                Ok(Some(info)) if info.group == root && !info.zombie => observed.push(info.identity),
                Ok(_) => {}
                Err(_) => { self.complete = false; }
            }
        }
        // 活根的启动身份与组号必须在成员身份读取前后都一致；已退出根另走 WNOWAIT。
        if !mac_process_info(root).is_ok_and(|info| info.is_some_and(|info| info.identity == anchor && info.group == root)) { return; }
        for identity in observed { if !self.seen.contains(&identity) { self.seen.push(identity); } }
    }

    fn observe(&mut self) {
        let mut pending = self.seen.clone();
        while let Some(parent) = pending.pop() {
            match mac_matching_process(parent) {
                Ok(Some(_)) => {}
                Ok(None) => continue,
                Err(_) => { self.complete = false; continue; }
            }
            let children = match mac_process_ids(parent.pid, true) {
                Ok(children) => children,
                Err(error) if error.raw_os_error() == Some(libc::ESRCH) => continue,
                Err(_) => { self.complete = false; continue; }
            };
            let mut observed = Vec::new();
            for pid in children {
                match mac_process_info(pid) {
                    Ok(Some(info)) if info.parent == parent.pid && !info.zombie => observed.push(info.identity),
                    Ok(_) => {}
                    Err(_) => { self.complete = false; }
                }
            }
            match mac_matching_process(parent) {
                Ok(Some(_)) => {}
                Ok(None) => continue,
                Err(_) => { self.complete = false; continue; }
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

    fn gone(&mut self, group: u32) -> bool {
        self.observe();
        self.root_confirmed && self.complete && self.seen.iter().all(|identity| matches!(mac_matching_process(*identity), Ok(None)))
            && mac_process_ids(group, false).is_ok_and(|pids| pids.into_iter().all(|pid| {
                match mac_process_info(pid) { Ok(None) => true, Ok(Some(info)) => info.zombie, Err(_) => false }
            }))
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
        let seen = root.ok().flatten().map(|info| vec![info.identity]).unwrap_or_default();
        let observed = std::sync::Arc::new(std::sync::Mutex::new(MacObservedTree {
            // 初次 ESRCH 可由未回收 child 的 WNOWAIT 补证；其他查询错误仍保持失败关闭。
            root: pid, root_confirmed: !seen.is_empty(), complete, seen,
        }));
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (observed_bg, stop_bg) = (observed.clone(), stop.clone());
        let watcher = std::thread::Builder::new().name("imates-proc-tree".into()).spawn(move || {
            while !stop_bg.load(std::sync::atomic::Ordering::Relaxed) {
                if let Ok(mut tree) = observed_bg.lock() { tree.observe_root_group(pid); tree.observe(); }
                std::thread::sleep(Duration::from_millis(20));
            }
        }).ok();
        if watcher.is_none() { if let Ok(mut tree) = observed.lock() { tree.complete = false; } }
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

    async fn gone(&self, group: u32) -> bool {
        let observed = self.observed.clone();
        tokio::task::spawn_blocking(move || observed.lock().is_ok_and(|mut tree| tree.gone(group))).await.unwrap_or(false)
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
    // 运行时只做非阻塞退出探测，不让高频流事件等待整组快照。
    if !mac_root_waitable(root)? { return Ok(None); }
    tree.macos.observe_before_reap(root).await;
    child.try_wait()
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
        let reaped = tokio::time::timeout(Duration::from_secs(2), child.wait()).await.is_ok_and(|result| result.is_ok());
        let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
        loop {
            if _tree.macos.gone(pid).await { return reaped; }
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
        let stale_tree = MacObservedTree { root: identity.pid, root_confirmed: true, seen: vec![stale], complete: true };
        assert!(!stale_tree.owns_group(group), "不能仅凭复用 PID 或组号确认进程组归属");
        let current_tree = MacObservedTree { root: identity.pid, root_confirmed: true, seen: vec![identity], complete: true };
        assert!(current_tree.owns_group(group));
        let mut unknown = MacObservedTree { root: identity.pid, root_confirmed: false, seen: Vec::new(), complete: false };
        assert!(!unknown.gone(0), "观察失败时不能把空列表当作清理完成");
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
