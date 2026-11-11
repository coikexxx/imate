use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::{Child, Command};

/// 只接受可核对的 npm 标准入口。`.js` 入口把参数作为 argv 传给 node；
/// 原生 `.exe` 入口直接启动。两种都不经过 cmd.exe。
pub fn resolve_program(program: &Path, args: &[String], path_env: Option<&std::ffi::OsStr>) -> Result<(PathBuf, Vec<String>), String> {
    if !program.extension().is_some_and(|e| e.eq_ignore_ascii_case("cmd")) {
        return Ok((program.to_path_buf(), args.to_vec()));
    }
    let parent = program.parent().ok_or("CLI 启动器没有父目录")?;
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
    #[cfg(not(windows))]
    { let _ = pid; ProcessTree {} }
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
pub async fn terminate_tree(child: &mut Child, pid: u32) -> bool {
    #[cfg(unix)]
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
}
