# imates MVP 开发手册

> 版本：2026-11-11（第 4 版，按 M2 多 Agent 评审校准权限、运行契约与 M3 前置规则）· 状态：M1 已完成；M2 实现已完成，实际验收未闭环。2026-11-11 决定暂不补测剩余三项。M3 实现已落地，自动测试通过，真实 CLI 团队讨论未执行；M5 grok、dsh 与 Grok 本机代理已接入，窗口部分验收通过，跨模式拒写、dsh 余额与 pi 安装仍阻塞
> 实施计划：[M0 CLI 实测](plans/2026-11-10-M0-CLI实测.md)、[M1 骨架](plans/2026-11-10-M1-骨架.md)、[M2 单 Agent 对话](plans/2026-11-10-M2-单Agent对话.md)、[M3 团队协作](plans/2026-11-11-M3-团队协作.md)、[M5 其余适配器](plans/2026-11-11-M5-其余适配器.md)（实现完成，验收受阻，pi 待安装）
> 设计依据：[参考项目调研](参考项目调研.md)

## 1. 产品定义

imates 是一个桌面应用，让一个人像管理团队一样管理多个 AI Agent。每个项目有一组分工不同的 Agent，它们围绕用户下发的指令讨论、争辩、达成一致并落实工作。

用户能做的事：

- 在首页看到自己管理多少个项目、每个项目多少个 Agent、项目当前状态
- 创建项目时定好规章制度（设计规范、开发规范等），可由内置助理起草
- 给每个 Agent 定人设（能干什么、边界在哪），可由内置助理起草
- 进入项目，查看成员列表、项目文件列表、规章制度
- 在对话框下发指令，观看 Agent 的讨论与执行过程
- 在像素风办公室里看每个 Agent 是在干活还是在摸鱼

### 已确认的关键决策

| 议题 | 决策 |
|---|---|
| 项目类型 | 通用，不限软件开发或文档创作 |
| Agent 运行时 | 每个 Agent 由一个本地 CLI 的无头进程驱动，应用探测本机已装 CLI |
| MVP 适配的 CLI | claude、codex、grok、dsh（DeepSeek Harness）、pi |
| 协作模式 | 组长负责制 |
| 权限 | 执行模式自动运行；写入范围取决于 CLI 自身能力，应用不保证限制在项目目录内 |
| 像素动画 | 办公室场景，小人固定工位、不走动 |
| 编排核心 | Tauri 的 Rust 后端 |
| 项目规章 | 创建项目时必经的一步，存为项目文件夹里的 `.imates/rules/*.md` |
| Agent 人设 | 四段式文档（定位、职责、边界、工作风格），存数据库 |
| 起草方式 | 内置助理 Agent 生成草稿，用户确认后才保存 |

### MVP 不做

并行执行、MCP 工具协议集成、危险操作审批、小人走动、Agent 间私聊、云同步、多窗口。讨论轮可以并行发言；“不做并行执行”专指多个执行回合同时改同一项目文件。

## 2. 总体架构

```
React UI（首页 / 项目页：成员、办公室、群聊、文件树）
   ↕ Tauri command（请求）+ event（流式推送）
Rust 核心 (src-tauri/src)
 ├─ store         SQLite 持久化
 ├─ cli_registry  探测本机 CLI 及版本
 ├─ adapters      每个 CLI 一个适配器，实现同一个 trait
 ├─ orchestrator  组长负责制的轮次调度
 ├─ assistant     内置助理：起草规章与人设
 ├─ rules         读写项目文件夹里的规章文件
 ├─ runner        子进程启动、逐行读取、超时、杀进程树
 └─ fs_watch      监听项目文件夹
```

原则：所有状态和进程都归 Rust 管，前端只渲染和发请求。界面刷新或前端渲染进程崩溃不取消 Rust 管理的运行；整个应用退出/崩溃的清理与恢复按第 9 节处理。

### 技术选型

| 层 | 选型 |
|---|---|
| 桌面壳 | Tauri 2（已有骨架） |
| 后端异步与进程 | tokio |
| 数据库 | rusqlite（bundled），启动时按版本号执行迁移脚本 |
| 文件监听 | notify |
| CLI 路径解析 | which（能解析 Windows 的 `.cmd` 垫片） |
| 文件夹选择、打开文件 | tauri-plugin-dialog、tauri-plugin-opener |
| 前端 | React 19 + TypeScript + Vite（已有） |
| 前端状态与路由 | zustand、react-router |
| 样式 | Tailwind CSS 4 |
| 像素画布 | 原生 Canvas 2D，不引入游戏引擎 |
| 测试 | Rust：cargo test；前端：vitest |

### 目录结构

```
src-tauri/src/
  lib.rs               注册 command、初始化状态
  commands.rs          Tauri command 入口（薄层，只做参数转换）
  store/               mod.rs、migrations.rs、各表的读写函数
  cli_registry.rs
  runner.rs
  adapters/            mod.rs（trait 与统一事件）、claude.rs、codex.rs、grok.rs、dsh.rs、pi.rs、fake.rs
  orchestrator/        mod.rs（状态机）、directive.rs（组长指令解析）、prompts.rs（提示词模板）
  assistant.rs         起草规章与人设、草稿解析
  rules.rs             .imates/rules/ 的列出、读取、保存、删除
  templates/           内置的规章模板与预设角色人设模板（Markdown）
  fs_watch.rs
src-tauri/tests/fixtures/   录制的各 CLI 真实输出样本
src/
  pages/               Home.tsx、Project.tsx
  components/          MemberList、ChatPanel、FileTree、RulesPanel、ProjectWizard、
                       AgentForm、PersonaEditor、ProjectCard
  office/              OfficeCanvas.tsx、sprites.ts（像素图数据）、layout.ts（工位排布）
  stores/              projects.ts、project.ts（单项目的成员/消息/任务）
  api.ts               command 调用与 event 订阅的类型化封装
```

## 3. CLI 适配层

### 统一接口

```rust
pub enum RunMode { Discuss, Execute }   // 讨论意图 / 执行意图；不构成文件系统隔离保证

pub struct TurnRequest {
    pub prompt: String,
    pub session_id: Option<String>,     // None 表示该 Agent 的首个回合
    pub cwd: PathBuf,                   // 项目文件夹
    pub mode: RunMode,
    pub model: Option<String>,
}

pub struct CommandSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub stdin: Option<String>,          // 提示词经 stdin 传入
    pub prompt_file: Option<PathBuf>,   // 或经临时文件传入
}

pub enum AgentEvent {
    SessionStarted { session_id: String },
    TextDelta { text: String },
    ToolUse { name: String, summary: String },
    ToolState { active: bool },         // 适配器按真实事件跟踪活动工具，不把开始事件当完成
    TurnCompleted { final_text: String, usage: Option<Usage> },
    Error { message: String },
}

pub trait CliAdapter: Send + Sync {
    fn id(&self) -> &'static str;                       // "claude" / "codex" / ...
    fn detect(&self) -> Option<CliInfo>;                // 路径与版本，未安装返回 None
    fn build_command(&self, req: &TurnRequest) -> CommandSpec;
    fn environment(&self) -> Vec<(String, Option<String>)> { vec![] } // 仅覆盖本次子进程
    fn parse_line(&mut self, line: &str) -> Vec<AgentEvent>;
}
```

### 进程模型

- 一个回合一个进程，跑完即退。Agent 的记忆靠各 CLI 自带的会话续接。
- 首个回合从输出里拿到会话 ID，存入 `agents.session_id`，之后每回合带上它续接。
- 人设与项目规章写在首个回合提示词的开头，不依赖各家的系统提示参数。
- 提示词不走命令行参数，优先 stdin，其次临时文件。原因有三：Windows 下 `.cmd` 垫片收不了多行参数；以 `-` 开头的提示词会被当成选项；命令行有长度上限。
- 子进程的 `current_dir` 是应用的工作目录基准。适配器仅在 CLI 确实要求时再传对应目录参数；重复传入的 CLI 参数不是安全边界。M0 命令表记录的是当时实测的调用形式，不自动等同于适配器最终构造的命令。
- 项目文件夹不保证是 git 仓库，适配器的参数要让 CLI 在非 git 目录里也能启动。
- Windows 下启动进程加 `CREATE_NO_WINDOW`，避免闪黑窗。
- 取消与超时都通过杀进程树实现：macOS 用进程组。Windows 使用 `taskkill /T /F`，并在系统允许时把进程放进带 `KILL_ON_JOB_CLOSE` 的 Job。父进程先退出后，`taskkill /T` 清不掉被过继的后代，因此再按进程创建时间清理已经观察到的后代。进程表快照放在独立线程，不占用 stdout/stderr 排空。
- 子进程输出按 UTF-8 逐行读取；无法解析为 JSON 的行记入运行日志，不中断回合。
- 回合成功的判据：收到 `TurnCompleted`、进程退出码为 0、没有致命 `Error`，且未取消、未超时。任何失败、取消或超时一旦成立，后续迟到的完成事件不能把回合改回成功。stdout 关闭后进程在宽限期内仍不退出就杀掉并按失败处理。失败回合里已输出的文本保留，随失败消息一起展示。
- 权限证据按 CLI 分级记录：运行事件回显的实际模式、针对明确 CLI 版本实测通过的模式、尚未验证。不能要求 CLI 回显它没有提供的字段，也不能把参数已传入当成权限已生效。M0 已确认执行模式可写到工作目录之外；讨论模式也可能写入 CLI 自身目录，界面不得暗示 `Discuss` 是强隔离。
- `TurnCompleted.usage` 只填入经该 CLI 实测确认的本回合用量。若语义未知，归一字段保留 `None`，原始用量可单独留作诊断；不得猜测累计/单回合语义或无条件减基线。缓存输入属于输入子集时不得重复相加。
- CLI 状态分为“已安装”和“已接入”：前者由本机探测得出，后者表示应用已有受支持适配器。已安装但未接入的 CLI 可显示安装信息，但不可创建或启动对应 Agent。
- 应用不隔离或清理用户级 CLI 配置，也不保证覆盖项目内 `AGENTS.md`、`CLAUDE.md` 等 CLI 原生指令。CLI 可读取哪些配置由各 CLI 决定；配置可能与 imates 规章冲突，应用不宣称跨 CLI 的统一优先级。

### 各 CLI 的调用方式

下表基于 M0 的 Windows 11 样本；版本是样本版本，不代表当前开发机版本。应用统一通过 `current_dir` 设置工作目录，表中省去 M0 人工命令里的 `-C` / `--cwd`；去掉目录参数后的行为在对应平台验收。pi 未安装，顺延到 M5。细节见 [CLI 实测记录](CLI实测记录.md)。

| CLI | 无头调用 | 事件输出粒度 | 续接 | 讨论意图（非强隔离） | 执行模式（自动） | M0 样本状态 |
|---|---|---|---|---|---|---|
| claude | `claude -p`，提示词经 stdin | `--output-format stream-json --verbose`，本次没有 token 增量 | `--resume <session_id>`，ID 不变 | `--permission-mode plan`（拦住工作目录里的写，但会写自己的计划目录） | `--permission-mode bypassPermissions`（能写到工作目录外） | Windows 2.1.319 |
| codex | `codex exec --json -`，提示词经 stdin | `--json`，整段 `agent_message`，没有 token 增量 | `codex exec resume <thread_id> --json -`（不能加 `-s`，权限覆盖待 G1） | `-s read-only` | `-s workspace-write`（Windows 样本可写到工作目录外） | M0 样本 0.167.0 |
| grok | `grok --prompt-file <文件>` | `--output-format streaming-messages-json`，整段消息。要逐字增量改用 `streaming-json` | `-r <session_id>`，ID 不变 | `--permission-mode plan`（拦住工作目录里的写，但会写自己的会话目录） | `--permission-mode bypassPermissions`（能写到工作目录外） | Windows 1.0.77；本机代理恢复联网，同模式执行/续接通过，plan→execute 工具仍拒写 |
| dsh | `dsh headless -`，提示词经 stdin；runner 设置 current_dir，M0 人工调用在 Git Bash 用 `cmd //c` | `--json`，整段 `text`，另有 `thinking`。用量在 `step_end.usage`（camelCase） | `--session-id <id>`，ID 不变，`turn` 递增 | `--permission-mode read-only --approval-policy never`（沙箱拒绝写入） | `--permission-mode workspace-write --approval-policy never`（能写到工作目录外） | Windows 0.2.2-rc.1；M5 首回合 cwd 正确，余额不足未完成 |
| pi | 实测 | 实测 | 实测 | 实测 | 实测 | 未安装，M5 前需先装 |

某个 CLI 没有强制只读模式时的兜底：仍按其实际能力运行，并在提示词里明确要求"本回合只讨论，不要修改任何文件"；界面标为讨论意图，不承诺文件绝对不变。claude 和 grok 的 `plan` 会写各自的计划/会话目录，提示词兜底仍然要留。dsh 没有 `--model`，模型在 profile 配置里，`agents.model` 对它不起作用。

M0 确认：claude 的 `bypassPermissions`、codex 的 `workspace-write`、grok 的 `bypassPermissions` 和 dsh 的 `workspace-write` 都能把文件写到工作目录之外。

参考项目里的 codex 写法（`--skip-git-repo-check`、`-c sandbox_mode=`、`approval_policy="never"`）M0 没有单独重跑。M2 候选命令用 `-c` 同时覆盖首次和续接权限，但必须通过 M2 附录 G 的行为实测。帮助文本只能证明参数被接受，不能证明权限生效。非 git 目录和用量累计语义也需验证。

### 适配器能力与证据

| CLI | M0 权限证据 | 会话与模式切换 | 用量归一规则 | 接入阶段 |
|---|---|---|---|---|
| claude | `system/init.permissionMode` 可核对。Windows 2.1.319 在 2026-11-11 有工具层拒写：plan 下 Write 被拒绝且目标文件不存在，bypassPermissions 下 Write 成功。M0 另有写入计划目录的样本。macOS 2.1.322 仍无工具层证据。以上都不等于硬隔离 | Windows 2.1.319 同一 session 的 plan → bypassPermissions → plan 已核对模式、续接和文件事实。macOS 2.1.322 只核对了模式回显和续接，该次 plan 请求没有触发写工具 | macOS G3 与 Windows 2.1.319 都对照了 `result.usage` 与 `model_usage_since_last_result`；后者的 `costUSD` 是本回合费用，`total_cost_usd` 为会话累计；不重复累加 assistant usage | M2 |
| codex | `thread.started` 无权限字段；Windows M0 有新会话样本，macOS 0.167.0 G1 已有工具和文件事实 | macOS 同模式续接与 Discuss → Execute → Discuss 已通过 G1；结果不外推其他版本/平台 | macOS G2 已证实 `turn.completed.usage` 为线程累计，同会话按可靠基线取差；缓存输入不重复计入 input | M2 |
| grok | `system/init.permissionMode` 可核对；本次 plan 的 write 被拒且文件不存在，不等于硬隔离 | Windows 1.0.77 经本机代理恢复联网。应用同一执行会话创建文件/续接通过；G4 plan→execute 的回显为 bypassPermissions，但 write 仍按 plan 拒绝，跨模式失败、团队混合模式未通过；macOS 未验证 | G4 两回合 input 从 38776 降至 1203、费用从 0.092212 降至 0.02762，确认不是会话累计；应用成功回合的 token/费用已保存 | M5 已接入，同模式执行通过，混合模式待整改 |
| dsh | `session` 不回显权限；Windows M0 沙箱行为已有样本；M5 首回合 cwd 与进程目录一致 | M0 同模式续接已有证据；2026-11-11 M5 首回合返回 QUOTA / 402，跨模式未验证；macOS 未验证 | 按 M0 `step_end.usage` 逐步归集，`final` 不含 usage；M5 因余额不足未产生可归一用量，追加验证未完成 | M5 已接入，真实 CLI 验收受阻 |
| pi | 未安装、无样本 | 未验证 | 未验证 | M5 |

有权限回显时必须核对，回显不符立即失败；没有回显时只能报告“此版本/平台行为已验证”或“未验证”。G1 至少覆盖同模式续接和同一会话的 `Discuss → Execute → Discuss`，以工具确实尝试写入、工具结果及文件状态共同判定，不能仅凭模型声称遵守只读。跨模式不通过时，不能用单个 `session_id` 承载混合模式；适配器保持该能力阻塞，回填分模式会话或重建会话方案及测试后再启用。

`CliInfo` 至少包含 `id / label / installed / supported / path / version`。`installed` 不证明已登录或可运行；M2 仅 claude/codex 的 `supported` 为真；M5 已加入 grok/dsh，pi 仍为假。G1/G3 未通过的能力仍不得宣称已验收。用户自定义 CLI 配置仍是后续需求。

### 平台与配置继承

- 证据按“操作系统、CLI 版本、启动来源、模式、续接场景”记录。Windows M0 结论不得直接外推 macOS；M2 的 G3 单列 macOS 桌面启动后的 PATH 探测、首回合/续接、权限及进程树清理验收。
- 探测和启动使用同一份环境及已解析的绝对 CLI 路径。macOS 桌面启动时不能假设有交互 shell 的 PATH；先使用进程 PATH，缺失时用有超时的登录 shell 只获取 PATH，再通过普通进程参数启动 CLI。取环境失败应返回可诊断的未找到状态，不把用户 prompt 拼成 shell 命令。
- 2026-11-11 用户新增授权 Grok 本机代理配置：首页和项目页提供「Grok 代理」，默认 system。优先继承代理环境变量，否则 Windows 用 WinHTTP 读取当前用户静态代理；手动模式支持带显式端口的 HTTP/HTTPS/SOCKS5 地址，直连清除 Grok 子进程的代理变量。设置保存在应用 settings 表，只影响后续 Grok 回合，不修改系统或 CLI 全局配置。PAC/自动检测提示手填；macOS/Linux 自动读取系统图形设置尚未实现，可继承环境或手填。代理只进入子进程环境，不进入命令参数与运行日志。
- MVP 保留 CLI 原生配置继承行为。M0 只证明 Claude 输出过本机技能，未证明全部 hooks、MCP、插件都启用。应用“不做 MCP”指不提供 MCP 配置和调度集成，不代表底层 CLI 没有 MCP。
- imates 提示词声明本回合目标、当前规章和人设；若 CLI 原生配置产生冲突，记录并展示 CLI 的错误或限制，不伪造统一的指令优先级。隔离参数及其副作用需另行实测后才能采用，不改写用户的全局配置。
- 修改 Agent 的 CLI 或项目文件夹时，事务内清空受影响的 `session_id`，重置用量基线；下一次使用完整恢复上下文。运行期间拒绝这些修改。修改模型也清空会话，避免复用不同模型的计数基线。

### 安全边界

执行模式可写到项目目录之外，实际边界取决于 CLI 自己的沙箱，应用层不做额外隔离。创建 Agent 与首次启用执行模式的界面上要写明这一点；不以提示词或复选框冒充技术隔离。适配器不得宣称隔离了用户全局配置或项目已有的 CLI 原生指令。

## 4. 协作流程

### 一条指令的生命周期

```
用户指令
  → 组长研判 ──→ reply（直接答复，结束）
        │
        ├──→ discuss ──→ 成员并行发言 ──→ 组长裁定 ──→ discuss（下一轮，最多 3 轮）
        │                                      └──→ assign / reply
        └──→ assign ──→ 成员串行执行并汇报 ──→ 组长验收 ──→ reply（交付，结束）
                                                    ├──→ rework（打回，每任务最多 2 次）
                                                    └──→ assign（追加任务）
```

1. **下发**：指令写入项目群聊。
2. **组长研判**（讨论模式）：组长收到指令、团队花名册、群里的新消息，输出一条指令。
3. **讨论轮**：被点名成员并行发言（讨论模式）。下一轮每人能看到上一轮全部发言。每轮结束组长裁定。讨论轮数按整条指令累计，达到上限时，编排器告知组长"必须给出 assign 或 reply"。
4. **决议**：组长的 assign 包含决议摘要与任务清单，存为决议消息和任务记录。
5. **执行**：按任务顺序逐个执行（执行模式），成员的最终输出作为汇报写入群聊。组长也可以给自己派任务。某个任务失败时停止往下执行，本批余下的任务标为 `cancelled`，直接进入验收。后面的任务通常依赖前面的产出，不能在前置失败后照常放行。
6. **验收**（讨论模式）：本批任务全部完成或有任务失败后，组长审阅汇报并输出指令。有任务失败时，提示词里注明哪个任务失败、哪些任务没有执行，并提醒失败的任务可能已经改了一部分文件。
7. **交付**：组长 reply 时，本条指令若还有失败或未执行的任务，编排器在答复后追加一条系统消息把它们列出来，不让交付看起来比实际完整。

各阶段组长可用的 action：

| 阶段 | 可用 action |
|---|---|
| 研判 | `reply`、`discuss`、`assign` |
| 裁定 | `discuss`（未到轮数上限时）、`assign`、`reply` |
| 验收 | `reply`、`rework`、`assign` |
| 干预研判 | `reply` + `continue/end`、`assign` + `replace`（见用户干预） |

### 组长指令协议

组长每次回复末尾必须带一个 `imates` 代码块，内容是一个 JSON 对象。代码块之前的文字作为组长的发言显示在群聊。

````
```imates
{"action": "discuss", "to": ["前端", "测试"], "topic": "登录页用表单校验库还是手写"}
```
````

| action | 字段 | 含义 |
|---|---|---|
| `reply` | `message` | 答复用户，本条指令结束 |
| `discuss` | `to`（成员名数组）、`topic` | 发起一轮讨论 |
| `assign` | `decision`、`tasks`（每项含 `to`、`title`、`detail`） | 公布决议并派工 |
| `rework` | `task_id`、`feedback` | 打回某个任务 |

任务项可选 `replaces_task_id` 记录被替代的失败/取消旧任务。干预研判阶段另外要求 `disposition`，其余阶段禁止该字段。

解析规则：

- 只在回合的最终文本（`TurnCompleted.final_text`）里找指令，不看工具调用之间的过程文字。
- 仅接受最终文本末尾独立、闭合的一个 `imates` 围栏块；围栏外引用的 JSON、其他代码块及普通正文都不是命令。不再从正文扫描任意含 `action` 的对象，也不做全局弯引号/尾逗号替换。
- 只接受标准 JSON 与当前 action 声明的字段。成员匹配去掉开头一个 `@` 和首尾空白，采用与创建判重一致的 `trim().to_lowercase()`；匹配后立即绑定 Agent ID，后续调度不再依赖显示名。
- `discuss.to` 必须是 1–8 个不同的有效成员名；重复、未知或类型错误均按协议错误处理，不静默丢弃。
- 以下情况算错误，把具体错误发回组长要求重发，仅重试一次：
  - 缺少唯一末尾指令块、JSON 非法、字段类型不符或有未定义字段。
  - action 在当前阶段不可用（见上一小节的表）。
  - `assign` 的任务数不在 1 到 8 之间，某个任务的 `to` 不是现有成员，或 `title/detail/decision` 为空。整个派工拒绝执行，不部分落库。
  - `rework` 的 `task_id` 不属于本条指令、不是当前有效批次的 `done/failed` 任务，或已达返工上限。`cancelled` 任务不能靠返工自动复活。
  - `reply` 的 `message` 为空。
- 文本字段先去空白再校验：`title` 最多 200 字符，`topic` 最多 4,000，`detail/decision/feedback/message` 各最多 16,000；整块 UTF-8 最多 128 KiB。这些是应用协议限制，超限返回具体错误。
- 返工成功不自动恢复下游已取消任务。若仍需执行，组长必须用 `assign` 显式新建任务，并通过任务项可选 `replaces_task_id` 关联同一指令的失败/取消旧任务。旧记录保留；交付时按未被替代的有效任务判断遗留项。
- 重发后仍错误，则把原文作为普通发言展示，指令状态置为"等待用户"，写一条系统消息说明原因。

### 上下文传递

每个 Agent 记录 `last_seen_message_id`。每回合的提示词由两部分组成：自上次以来群里的新消息（标明发言人），加上本回合的要求。历史不重复灌入。

游标的推进规则（讨论轮是并行的，规则不严会漏消息）：

- 单回合取构建提示词时的最大消息 ID；并行讨论在整轮开始前固定同一消息上界、规章版本与花名册版本，再为全体成员构建输入。本轮其他成员发言从下一阶段才可见，不能随进程启动时序改变输入。
- 回合成功才推进游标。回合失败时游标不动，下一回合重新带上这些消息。
- 该 Agent 自己发的消息不回灌给它。

规章与人设变更也走这条通道（详见第 5 节）：规章通知携带版本、操作和必要全文，删除/清空显式撤销旧版；人设标记待更新，下一回合提示词开头附最新版。

重建会话时，角色开场之外必须注入恢复包：当前用户目标及补充、有效决议、任务与状态、当前任务验收要求和已有失败/部分写入事实。最近 20 条群消息只作补充，不能替代恢复包；M2 尚无任务实体时至少保留当前请求和该 Agent 的未读消息，不静默截成最近 20 条。新成员加入时将游标设为当时项目最大消息 ID，首次回合使用开场、上述背景和最近 20 条补充，不灌入全部历史。MVP 不按固定回合数主动轮换会话。

### 提示词模板

`orchestrator/prompts.rs` 中维护六个模板：

- 角色开场：项目名与项目描述、项目规章全文、本人的人设全文、成员花名册（其他成员只列名字、角色、定位和边界）。仅首个回合使用；会话续接失败后重开会话时再用一次（见第 9 节）。
- 组长研判：附指令协议说明。写明能直接答复就 `reply`，不要为了显得忙而发起讨论或派工；派工时每个任务的 `detail` 要写清要做什么、交付什么、怎样算完成。
- 成员讨论：议题、要求给出立场和理由、可以反驳他人、不得修改文件。
- 组长裁定：当前轮次、剩余轮数。
- 成员执行：任务标题与详情、要求完成后给出简短汇报（做了什么、改了哪些文件、遗留问题）。写明成员只汇报自己的任务，不能宣布整条指令完成。
- 组长验收：各任务的编号与汇报（`rework` 用编号指明打回哪个任务），要求对照项目规章和各成员的边界检查，违反规章或越界的打回。

所有成员回合（讨论和执行）的模板里都带以下约束：

- 本次调用只是你的一次发言，调度由应用负责。说完就结束，不要等其他成员回复，不要反复查看文件等别人的进展。
- 以本回合的要求为准。群消息里更早的计划和发言只是参考，不是待办。
- 不得修改 `.imates/`，尤其是规章目录；需要调整规章时在汇报中提出，由用户通过规章编辑入口处理。

### 用户干预

M2 仍是一项目一个活动回合，运行中拒绝再次发送；以下队列和干预语义在 M3 落地。每项目只有一个调度器，并行讨论由该调度器统一发起；同一 Agent 会话任一时刻最多一个子进程。不同项目解析到同一规范化工作目录（含符号链接别名）时，执行回合也须共享目录锁；锁覆盖完整执行回合及清理，助理的讨论回合不得借此获得写权限。

干预边界是并行讨论整轮结束、串行执行当前任务结束；失败取消后续任务的规则先于干预执行。正常组长回合则在该回合结束后处理干预；有待处理干预时，暂存组长结果并停止后续调度。

| 输入或操作 | 边界前 | 边界后与持久化结果 |
|---|---|---|
| 运行中普通插话 | 保存为当前指令的队列项，不抢占进程 | 暂停旧批次，进入干预研判；组长必须明确继续、替换或结束 |
| 运行中 `@成员` | 同样入队并绑定目标 Agent ID | 暂停旧批次，由目标成员直接 Execute；报告后交组长干预研判，不能默认恢复旧批次 |
| 停止 | 立即使当前调度代次失效，取消所有项目运行并等待进程树/管道清理 | 运行与正在执行的任务置 `cancelled`；未启动任务及未处理队列项也取消；指令置 `waiting_user`，写明部分文件可能已改动 |
| 等待用户时“重试” | 仅允许满足第 9 节安全条件的原阶段重放 | 保留指令 ID、已用轮数和总回合数，不重跑已执行任务；达到护栏后不能靠重试清零 |
| 等待用户时“补充” | 用户显式选择原指令并填写内容 | 进入干预研判，保留原任务事实，不自动重放失败执行 |
| 等待用户时“放弃” | 原指令置 `abandoned` | 取消剩余任务与队列项，保留历史及文件现状 |
| 等待用户时直接发送新消息 | 原指令置 `abandoned`，与新来源消息原子提交 | 创建新指令；不会把新消息误判为旧指令的隐式重试 |

干预研判复用末尾 `imates` 协议，仅在该阶段要求额外字段 `disposition`：`reply + continue` 表示只答复本次补充并恢复保存的阶段；`assign + replace` 取消旧批次尚未开始的任务并创建新批次；`reply + end` 取消余项并结束原指令。其他组合均报协议错误，继续/替换不得清零已用护栏。队列按提交顺序逐项处理，停止及放弃后不自动重新投递。空闲时的 `@成员` 是独立的直接指令，不经过组长，结束后指令完成。

### Agent 状态

`idle`（空闲摸鱼）/ `thinking`（进程已启动，尚无输出）/ `talking`（讨论模式下有文本输出）/ `working`（执行模式下运行中）/ `error`。状态变化通过事件推给前端，同时驱动成员列表和像素动画。

### 成本护栏

可在设置页调整：

| 护栏 | 默认值 | 说明 |
|---|---|---|
| 讨论轮数上限 | 3 | 按整条指令累计 |
| 单任务返工上限 | 2 | |
| 单条指令回合总数上限 | 30 | 组长和成员的回合都计入。达到后指令置为"等待用户"并写系统消息。兜住"验收后反复追加派工"这类没有单独上限的循环 |
| 模型空闲阈值 | 5 分钟（初始值，待 M2 验收校准） | 已知工具运行中改用工具阈值；stdout/stderr 字节更新空闲时间 |
| 工具运行阈值 | 讨论 10 分钟、执行 30 分钟（初始值，待校准） | 从工具开始计时，不因持续输出或更多工具开始事件无限延长；缺工具结束事件时仍有总时长兜底 |
| 回合总时长上限 | 讨论 10 分钟，执行 30 分钟 | 有输出也不能超过；与工具阶段分别配置，M6 再提供设置 UI |

每个实际 CLI 尝试（含协议纠错、自动重试、点名回合）都计入 30 回合，不因并行或切换批次重置。M3 在并发启动前一次预留所有所需名额，不能只在完成时加计数而越过上限。

CLI 返回用量时记入运行记录；只有语义已验证的字段才累计展示。未知保持未知，不把缓存输入重复计费，不按输入 token 数直接推算费用；暂不承诺精确 token/费用硬上限。重复输出检测、按 CLI 限流与费用上限列入后续候选，不替代当前轮数和总时长护栏。

## 5. 规章制度与人设

### 内置助理

应用自带一个助理 Agent，专门起草规章和人设。它不属于任何项目、不占工位、不出现在群聊里。

- 所用 CLI 在设置页指定（`assistant_cli`），默认取已安装且已接入的第一个，顺序为 claude、codex、grok、dsh、pi。显式设置指向未接入 CLI 时明确报错；M6 再做设置 UI。
- 以讨论模式在项目文件夹里运行，文件夹里已有内容时先了解现状再起草；只读约束与隔离局限同第 3 节。
- 提示词要求只产出草稿、不修改项目文件；由用户确认后应用才保存。CLI 自身会话/计划文件仍可能被写入。
- 一次起草对话内（生成、按反馈重写）续接同一个 CLI 会话；对话框关闭后会话丢弃。
- 复用 runner 和适配器，没有单独的进程机制。

### 项目规章

**存储**：项目文件夹下的 `.imates/rules/*.md`，一份规章一个文件，文件名即标题（如 `协作规范.md`）。Agent 能随时自己翻阅，用户能用任何编辑器修改，也能随项目进 git。

**起草**：输入是项目名称、项目描述和文件夹现状。具体出哪几份由助理按项目性质判断，例如软件项目出开发规范和设计规范，文档项目出写作规范和审校标准。`协作规范.md` 每个项目都有，内容包括讨论时如何表态、汇报格式、遇到职责外的事怎么办。

**草稿格式**：规章正文是长 Markdown，不适合塞进 JSON。助理按分隔行输出，应用按行切分：

```
=== FILE: 协作规范.md ===
（Markdown 正文）
=== FILE: 开发规范.md ===
（Markdown 正文）
=== END ===
```

解析不到任何 `=== FILE:` 行时视为起草失败，展示原文并提示重试或改用模板。

**生效**：

- 每个 Agent 首个回合的开场白附上全部规章的全文。
- 组长验收时对照规章检查。
- 规章版本用有序的文件名与全文计算内容标识，覆盖新增、替换、删除、重命名和全部清空。更新通知包含版本、操作、有效文件列表及新增/替换全文，并明确旧版本失效；重命名表示旧名删除与新名新增。全部清空必须声明“旧规章全部失效，当前暂无规章”，现有会话也必须收到，不能只改首回合开场白。
- 应用内变更由 M3 通知，外部变化由 M4 的 fs_watch 触发同一通知路径。每回合读取对应快照，成功才确认已送达版本；失败后重发，同轮成员使用同一版本。系统消息默认折叠。
- 执行提示词禁止修改 `.imates/`，尤其是规章目录；这属于软约束。监听只能证明文件变化，不能证明修改者身份；不得从 fs_watch 事件推定是哪个 Agent 修改。

**不用 AI 的路径**：`templates/` 内置协作规范、开发规范、设计规范、写作规范四份模板，用户可以直接选用再手改。

### Agent 人设

**结构**：一份 Markdown，固定四段。

| 段落 | 内容 |
|---|---|
| `## 定位` | 一句话说明这个成员是谁 |
| `## 职责` | 能干什么，负责哪些产出 |
| `## 边界` | 不做什么；遇到职责外的事该交给谁 |
| `## 工作风格` | 讨论时的立场倾向，例如测试偏保守、产品偏用户视角 |

工作风格是为了让讨论有真实分歧，而不是所有人都附和组长。

**起草**：输入是项目描述、项目规章、要创建的角色名，以及已有成员的人设。助理据此划分边界，避免与已有成员重叠。输出就是这份四段式 Markdown，应用校验四个标题齐全。

**存储与生效**：

- 存在 `agents.persona`。
- 开场白注入本人人设全文；花名册里其他成员只列定位和边界，用来判断"这事该找谁"。
- 人设被修改时置 `agents.persona_dirty = 1`，该 Agent 下一回合提示词开头附上新人设全文并声明取代旧人设。仅在回合成功且数据库当前人设仍等于本次应用的版本时清除标记；运行期间的新编辑不能被旧回合确认覆盖。
- 有成员加入、离开或人设变更时，向群聊写一条系统消息，更新花名册摘要。

**约束强度**：边界是提示词层面的软约束。越界由组长在验收环节纠正，应用不做硬性拦截。

**不用 AI 的路径**：六个预设角色（组长、产品、开发、测试、撰稿、审校）各有一份内置人设模板。

## 6. 数据模型

SQLite 文件位于应用数据目录。项目文件和规章文件留在用户选择的文件夹里，应用不复制。下面第一段是 M1 已落地的 v1 schema；M2 复用它，M3 新实体通过追加迁移实现，不能修改 v1 脚本或把设计稿当成已实现代码。

```sql
projects(id, name, description, folder_path,
         status,                             -- idle / discussing / executing / waiting_user
         created_at, updated_at)

agents(id, project_id, name, role, persona, persona_dirty, cli, model, is_leader,
       session_id, last_seen_message_id, seat_index, created_at)

messages(id, project_id, sender_type,        -- user / agent / system
         sender_agent_id,
         kind,                               -- instruction / chat / discussion / decision / report / system
         content, round, created_at)

tasks(id, project_id, instruction_message_id, assignee_agent_id,
      title, detail, order_index,
      status,                                -- pending / running / done / failed / cancelled
      rework_count, report_message_id, created_at, finished_at)

runs(id, agent_id, mode,                     -- discuss / execute
     command, session_id, started_at, ended_at, exit_code,
     status,                                 -- running / ok / failed / timeout / cancelled / interrupted
     usage_json, error)

settings(key, value)
```

约束：同一项目内 Agent 名字唯一；每个项目恰好一个 `is_leader = 1`；每个项目最多 8 个 Agent。M2 的创建/编辑/点名统一使用 `trim().to_lowercase()` 判重与匹配，并拒绝旧数据中的歧义名称；M3 追加唯一键迁移前先检测旧数据冲突，不自动重命名。Agent 的实时展示状态只存在内存里，不落库。

ID 约定：`messages.id` 是自增整数，兼作"新消息"游标，`agents.last_seen_message_id` 指向它；其余实体的 `id` 是 UUID 文本。`runs.agent_id` 可为空，内置助理的运行不属于任何成员。v1 删除成员会级联删除其 runs，但消息保留并置发送者为空；删除确认应说明运行历史随之删除，MVP 不承诺完整审计留存。

### M3 追加的持久化状态

```sql
instructions(id, project_id, source_message_id, kind, target_agent_id,
             phase,                          -- triage / discuss / decide / execute / review / intervention / direct
             status,                         -- running / waiting_user / completed / abandoned
             discuss_rounds, turn_count, current_batch,
             resume_phase, context_snapshot_json, wait_reason,
             created_at, updated_at, finished_at)

instruction_inputs(id, instruction_id, message_id,
                   kind,                     -- supplement / direct
                   target_agent_id,
                   status,                   -- queued / running / applied / cancelled
                   created_at, finished_at)

-- 追加到现有表，兼容已有 M2 数据；历史 runs 可为空。
tasks: instruction_id, batch_id, superseded_by_task_id
runs:  instruction_id, task_id
agents: name_key                             -- trim().to_lowercase()，项目内唯一
```

`source_message_id` 唯一且指向用户原始指令；`kind` 为 `team/direct`，直接点名的目标在接收时绑定 ID。`instruction_inputs` 保存运行期间的补充/点名，按来源 `message_id` 顺序处理；停止、放弃或重新启动后不能仅靠内存队列恢复投递。任务的旧 `instruction_message_id` 保留兼容，新调度以 `instruction_id` 为归属；已有 M2 run 不伪造所属指令。

`context_snapshot_json` 保存当前目标、消息上界、规章/花名册版本与快照、有效决议、任务状态及验收要求，并保存边界上暂存的组长指令和待执行动作，用于继续时恢复下一动作而不是重复调用已完成回合；不保存 CLI 思维过程或凭据。`resume_phase` 保存被干预打断的阶段，当前批次和已用护栏不因插话重置。

| 状态转换 | 同一事务内的写入 | 提交后动作 |
|---|---|---|
| 接收空闲项目的指令 | 用户消息、instruction、初始阶段与计数 | 调度首回合 |
| 发起一轮讨论或 CLI 尝试 | 固定上下文快照、预留总回合数、写 running run；讨论轮数只在发起整轮时增加 | 启动已预留的进程；启动失败终结对应 run，不回退已用次数 |
| 派工/替换批次 | 决议消息、任务、新批次、被替代任务关系、阶段及队列项状态 | 开始首个待执行任务 |
| 任务结束/失败 | run 终态、汇报消息、任务状态；失败时取消本批未开始任务 | 处理排队干预或进入验收 |
| 停止/放弃 | 已取消运行/任务/队列、指令等待或放弃状态 | 发终态事件，禁止旧代次继续调度 |
| 用户重试/补充 | 校验原指令和安全条件、保存恢复阶段或补充队列 | 处理指定阶段；不重新创建已成功任务 |

落库事务不能跨越 CLI 执行或网络等待；只有事务成功才能发事件。任务开始前先写 `running` 及 run 关联，即使在派工、启动、汇报边界崩溃，也能解释是否曾尝试执行。重启将运行中的 run 标为 `interrupted`、对应执行中任务标为 `failed`、未启动任务及队列取消，未结束指令统一转 `waiting_user`；不自动续跑或重试。失败事实保留在 runs/消息中，不能凭“没有完成报告”推断文件未变。

同一项目最多一条活动或等待处理指令；新指令替换等待指令时在事务内将旧指令置 `abandoned`。任务/输入必须属于同一项目与指令；`superseded_by_task_id` 只允许由同指令的新任务显式建立，不得指向自身或形成循环。返工增加原任务次数并建立新 run，保留旧 run 和报告消息；旧取消任务只有被显式新任务替代后才从交付遗留清单排除。

## 7. 前后端接口

### Command

| 分组 | command |
|---|---|
| CLI | `list_clis` |
| 项目 | `list_projects`、`create_project`、`update_project`、`delete_project`、`get_project` |
| Agent | `create_agent`、`update_agent`、`delete_agent` |
| 规章 | `list_rules`、`save_rule`、`delete_rule`、`list_rule_templates` |
| 助理 | `draft_rules`、`draft_persona`、`cancel_draft`、`list_persona_templates` |
| 对话 | `list_messages`、`send_message`、`stop_project`、`get_project_runtime`、`confirm_runtime_cleanup` |
| 指令（M3） | `list_instructions`、`resume_instruction`（显式 retry / supplement / abandon） |
| 任务 | `list_tasks` |
| 文件 | `list_files`、`open_file` |
| 设置 | `get_settings`、`update_settings` |

`delete_project` 只删除应用内的记录，不动项目文件夹（包括其中的 `.imates/rules/`）；删除确认说明这些文件将保留。

`draft_rules` 和 `draft_persona` 都接受一个可选的 `feedback` 参数，带上时表示按反馈重写上一版草稿。两者在助理回合结束后返回解析好的草稿，期间通过 `assistant:delta` 推送进度。向导中项目尚未入库，所以这两个 command 的输入（文件夹、项目描述、规章、已有成员人设）全部由参数传入，不从数据库读取。

### Event

| event | 载荷 | 用途 |
|---|---|---|
| `message:new` | `{ projectId, runId: string \| null, message: Message }` | 按 SQLite message.id 更新消息 |
| `message:delta` | `{ projectId, runId, seq, agentId, text }` | 显示当前回合输出片段 |
| `agent:status` | `{ projectId, agentId, runId: string \| null, status }` | 成员列表与像素动画 |
| `run:finished` | `{ projectId, runId, agentId, seq, status, error: string \| null }` | 项目回合终态：ok / failed / timeout / cancelled |
| `task:updated` | `{ projectId, instructionId, task }` | M3 任务进度 |
| `files:changed` | `{ projectId, paths }` | 文件树刷新与高亮 |
| `rules:changed` | `{ projectId, version }` | 规章面板刷新；M3 应用内、M4 外部变化 |
| `assistant:delta` | `{ draftId, runId, seq, text }` | 助理草稿进度，不进入项目群聊 |

以上为权威事件契约，载荷一律 camelCase。`seq` 从 1 开始，在同一 run 内单调递增，终态使用最终 seq。`get_project_runtime(projectId)` 返回 `{ active: RuntimeSnapshot | null, cleanupRequired: CleanupNotice[] }`：活动快照为 `{ projectId, runId, agentId, status, seq, partialText }`，清理提示为 `{ runId, projectId: string | null, folderPath }`，也包含同目录的助理残留。`confirm_runtime_cleanup(runId)` 只在用户明确确认已处理 interrupted 进程后解除对应目录阻塞，不能操作活跃回合；此标记不是自动清理的证明。所有权/清理标记存既有 settings 表，进程启动前写入，清理完成后才清除，不要求修改 v1 schema。M3 并行讨论再把 active 扩展为快照集合。

前端先订阅，再读取历史与快照；消息按 id upsert，增量按 `runId/seq` 去重，终态后迟到片段不复活旧回合，旧 run 状态也不得覆盖新 run。

消息与运行状态先提交数据库再发事件；事件发送失败不能把真实执行成功改为失败，前端通过重载恢复。各 CLI 当前样本主要为整段文本或工具事件，“流式”表示事件逐步到达，不承诺逐 token 打字机效果。

`Usage` 字段为 `inputTokens/outputTokens/cacheReadInputTokens/cacheCreationInputTokens/costUsd`，未知为 null，另有 `rawUsage` 只保存原始用量对象。原始用量与归一值分开，不把整段含敏感信息的 CLI 事件写入该字段。

## 8. 界面

### 首页

顶部一行总览（N 个项目 · M 个 Agent），下面是项目卡片网格。卡片显示名称、文件夹路径、Agent 数、当前状态（空闲 / 讨论中 / 执行中 / 等待用户）、最近活动时间。最后一张是"新建项目"，点击进入向导。

### 新建项目向导

1. **基本信息**：名称、本地文件夹、项目描述（要做什么、给谁用、有什么约束）。
2. **规章制度**：必经步骤。点"让助理起草"生成一组规章，或从内置模板里选。每份规章可编辑、删除，也可以新增；输入一句反馈可让助理重写。至少保留一份才能进入下一步。
3. **组建团队**：逐个添加成员（表单见下）。至少要有一个组长才能完成创建。

向导完成时才写入数据库和 `.imates/rules/`，中途取消不留痕迹。

### 项目页

三栏布局：

- **左栏 成员**：像素头像、名字、角色、所用 CLI、状态、累计用量。底部"添加成员"。
- **中栏**：上半部分是像素办公室画布，下半部分是群聊消息流与输入框。运行中时发送按钮旁出现"停止"。
- **右栏**：两个标签页。
  - **文件**：只读展示项目文件夹，忽略 `.git`、`node_modules`、`.imates` 等目录。点击文件用系统默认程序打开。最近一次指令中被改动的文件高亮。
  - **规章**：列出 `.imates/rules/` 下的规章，可查看、编辑、新增、删除，也可以让助理重写某一份。

点击成员可查看和编辑其人设。

### 新建 Agent 表单

名字、角色（预设：组长 / 产品 / 开发 / 测试 / 撰稿 / 审校，可自定义）、CLI（已知列表中未安装的提示安装，已安装但未接入的显示接入阶段并置灰）、模型（可选，留空用 CLI 默认）、人设。项目的第一个 Agent 默认是组长。

人设区是一个四段式编辑器。选预设角色时自动填入对应模板；点"让助理生成"则由助理参考项目描述、规章和已有成员起草，可编辑或给反馈重写。四段都非空才能保存。表单上注明：讨论只读要求及人设边界属于软约束；执行写入范围取决于 CLI，应用不保证限制在项目目录内，用户配置与项目原生指令也可能参与执行。

### 像素办公室

- Canvas 2D 绘制，逻辑分辨率小、整数倍放大，关闭图像平滑。
- 小人、桌子、电脑用代码里的像素矩阵定义，不引入外部素材。每个 Agent 按 ID 取一个固定配色。
- 工位按 `seat_index` 网格排布，最多 8 个。
- 状态与动画：

| 状态 | 动画 |
|---|---|
| idle | 随机一种摸鱼动作：喝咖啡、打瞌睡、刷手机 |
| thinking | 头顶省略号气泡 |
| talking | 头顶对话气泡，嘴部开合 |
| working | 敲键盘，屏幕闪烁 |
| error | 头顶红色感叹号 |

每种动画 2 到 4 帧，约每秒 4 帧。小人下方显示名字。

## 9. 异常处理

| 情况 | 处理 |
|---|---|
| CLI 未安装或已卸载 | 成员状态置为 error，提示重新选择 CLI |
| CLI 未登录、调用失败 | 该回合失败，原始报错写入系统消息 |
| 讨论轮中某成员失败 | 记为"未发言"，本轮照常继续 |
| 执行中任务失败 | 任务标为 failed，本批余下的任务标为 cancelled，直接进入组长验收，由组长决定打回、重新派工或答复用户。执行回合不自动重试，因为它可能已经改了文件 |
| 普通回合失败与自动重试 | 先按实际模式和失败类型判断，不按“是否组长”判断。仅对 Discuss、无任何工具调用、明确可重放的短暂调用失败自动重试一次；未登录、权限不符、取消、超时、协议外未知失败均不自动重试。Execute（含组长执行自己任务）不自动重试 |
| 回合超时（模型空闲、工具阈值或总时长超限） | 杀进程树，run 记 timeout，任务按执行失败处理；后续完成事件不能覆盖 |
| stdout 已关闭但进程 30 秒内不退出 | 杀进程树，按失败处理，已输出的文本保留 |
| 实际权限回显与请求不符 | 有回显时立即失败；无回显时按适配器能力表的版本/平台行为证据处理，不能伪造已核对 |
| 会话续接失败（CLI 明确报会话不存在或已损坏） | 仅在启动阶段明确拒绝会话、没有输出 Agent 正文且没有任何工具调用时，清空该 Agent 的 `session_id`，附角色开场与第 4 节完整恢复包重开一次。此例外不包含未知错误或执行过程中断；其他 Execute 失败不重试 |
| 达到单条指令回合总数上限 | 指令置为"等待用户"，写系统消息说明 |
| 应用正常退出 | 统一阻止新调度，取消所有项目及助理进程树，等待有界清理后退出；数据库记录与实际清理结果分别保存 |
| 应用异常退出及重启 | running run 记 interrupted，M2 项目、M3 指令转 waiting_user；M3 执行中任务记 failed，未启动任务/队列取消。无自动重放。数据库标记不代表进程已退出；持久化清理待确认标记，在确认残留已清理前拒绝同目录新运行。正常进程组清理不保证宿主崩溃时自动发生，不仅凭复用的 PID 盲杀 |
| 项目文件夹不存在 | 项目卡片标红，进入后提示重新选择文件夹 |
| 助理起草失败或草稿格式不对 | 展示原始输出和报错，可重试，也可改用模板或手写，不阻塞向导 |
| 没有探测到任何 CLI | "让助理起草"按钮置灰并说明原因，模板和手写仍可用 |
| `.imates/rules/` 被外部清空 | 规章面板提示补充；M4 发显式清空版本通知，已有会话下一回合撤销旧规章，新开场注明“本项目暂无规章” |
| CLI 升级导致未知事件或完成字段缺失 | 日志记录 CLI/平台版本、未知事件类型与解析失败计数；没有已识别的完成事件不能算成功。诊断须脱敏、限长，不以完整原始输出泄露路径或凭据 |

重试、协议纠错与会话重建共享“同一逻辑回合最多一次自动补救”的额度，不能叠加重试循环。手动 retry 沿用同一指令及护栏，仅允许明确可重放的阶段；执行有部分写入时用户须通过补充/新派工决定后续操作。MVP 不承诺自动回滚；记录 Git HEAD/status 或文件变更只能辅助了解现状，不能称为回滚快照或可靠的修改者证据。

## 10. 测试策略

- **适配器**：M0 录制每个 CLI 的真实输出，存入 `tests/fixtures/`。单测逐行喂给 `parse_line`，断言产出的事件序列；另测 `build_command` 在两种模式、有无会话 ID 下的参数。
- **编排器**：FakeAdapter 覆盖正常流程、护栏、各干预状态转换、任务失败后取消余项、返工后显式替换下游任务，以及按实际模式/错误类型控制补救额度。并行成员必须读取同轮消息/规章/花名册快照；规章删除清空与人设运行期编辑也须覆盖。解析测试拒绝普通正文里的 action JSON、引用示例、多指令块、非末尾块、非法 JSON、类型/长度错误、重复/未知成员与错误状态返工。不消耗 token。
- **助理**：用假适配器测规章草稿的分隔行解析（正常、缺 END、无 FILE 行、正文里含代码块）和人设四段校验；测开场白里规章、人设、花名册摘要的拼装，以及规章变更消息和 `persona_dirty` 的更新通知。
- **存储**：对内存 SQLite 测迁移与各表读写；对临时目录测规章文件的列出、保存、删除。
- **崩溃边界**：M3 在派工事务前后、run 落库/进程启动、报告落库前后注入中断，重启后归属、护栏和任务状态可解释，不重复启动；停止后旧回合不得覆盖新状态。M2 先验证残留 run 转 interrupted、清理待确认标记和会话主动失效。
- **runner**：用真实测试子进程覆盖 stdin、模型静默、工具长时间运行、stderr 持续输出、超长行、后代持有管道、EOF 宽限与进程树取消；FakeAdapter 不能代替这些进程行为测试。平台集成测试分别记录 Windows/macOS 已执行项。
- **前端**：vitest 覆盖 store 的事件归并逻辑和工位排布算法。
- **端到端**：每个里程碑结束时，用真实 CLI 按验收标准手动走一遍。

## 11. 里程碑

| 阶段 | 内容 | 验收标准 |
|---|---|---|
| M0 CLI 实测 | 五个 CLI 各跑无头调用（pi 若未装则顺延到 M5），录制输出样本，回填第 3 节参数表 | 每个已装 CLI 都有参数表和样本文件 |
| M1 骨架 | SQLite 与迁移、CLI 探测、首页和项目页外壳、新建项目三步向导、规章文件读写与规章面板、人设编辑器、内置模板（此阶段不接助理） | 能用模板和手写走完向导建出带规章和成员的项目，重启后数据还在 |
| M2 单 Agent 对话 | runner、claude 与 codex 适配器、按 CLI 事件增量显示群聊、停止、`@成员`、开场上下文、助理起草（含反馈重写） | 点名与续接、草稿确认保存通过；G1 跨模式、G2 用量语义记录、G3 macOS 环境/进程验收完成；验证致命错误/取消不能被完成事件覆盖、部分文本保留、重启与清理边界 |
| M3 团队协作 | 指令与输入队列迁移、严格协议、状态机、任务/批次、护栏、规章版本与上下文更新 | 正常走完讨论到交付；部分写入失败不自动重做，余项取消；派工/启动/报告边界重启可恢复解释且不自动重跑；插话/点名/停止按状态表执行，旧结果不复活任务；同轮快照及删除/清空通知有效 |
| M4 文件与动画 | 文件监听与文件树、外部修改规章文件的检测、像素办公室 | 改动的文件高亮；状态变化实时反映到小人 |
| M5 其余适配器 | grok、dsh、pi | 五种 CLI 都能当成员参与讨论和执行 |
| M6 打磨打包 | 设置页、报错文案、用量展示、Win 与 mac 安装包 | 在干净机器上可安装使用 |

M2 当前已完成实现，macOS 实际 CLI/UI 验收部分通过。2026-11-11 在 Windows 11 10.0.26300 补了 runner 进程树自动化、跨进程恢复测试，以及 Claude Code 2.1.319 同一 session 的 plan → bypassPermissions → plan 工具事实和用量。同日决定暂不补测以下三项，它们保持未验收，M2 不标为完成：macOS Finder/Dock 启动 PATH、macOS Claude Code 2.1.322 的工具层拒写、应用窗口里的异常退出和“我已处理残留进程，解除阻塞”。

M3 实现已落地。同日自动测试为后端 `cargo test` 144 passed、2 ignored，前端 `npm test` 29 passed，`npm run build` 通过。真实 CLI 团队讨论未执行，因此不把手册第 11 节“正常走完讨论到交付”记为已验收。上述 M2 三项不因 M3 测试通过而改为完成。

M5 当前进度（2026-11-11）：Task 1–6 与 Task 10 已完成，Grok 新增本机代理（默认跟随系统，可手动或直连）。2026-11-11 后端串行 168 passed、2 ignored，前端 32 passed，cargo build 与 npm run build 通过；默认并行测试仍有原有 runner 时序用例失败，未修改断言。窗口清单 1–4、8 通过，7 部分通过；成功 Grok 直接回合保存 token/费用。G4 同会话 plan→execute 仍被工具按 plan 拒写，G5 dsh QUOTA / 402 余额不足，pi 未安装；团队协作及团队用量未验收。M5 未完成，M3 整改仍待定。实测详情见 [CLI 实测记录](CLI实测记录.md)，逐项结果见 [M5 Task 9/10](plans/2026-11-11-M5-其余适配器.md)。不改变 M2、M3 的未验收状态。

依赖关系：M0 与 M1 互不依赖，可以并行；M2 同时依赖 M0 和 M1；M2 → M3 必须按序。M4 的文件树只依赖 M1，像素办公室依赖 M2 的状态事件；M5 依赖 M2。M4 和 M5 可以在 M3 之后以任意顺序进行。

## 12. MVP 之后

按价值排序的候选项：并行执行（每个 Agent 独立工作副本后合并）、危险操作审批、通过 MCP 工具让 Agent 主动互相喊话、小人走动与会议桌、更多 CLI。

调研参考项目后记下的候选项（依据见[参考项目调研](参考项目调研.md)）：

- 验收阶段允许组长用 `discuss` 请成员只读复核别人的产出。
- 失败细分类：区分限流、网络、服务端故障，按 CLI 报出的等待时间退避重试。
- 常驻进程加双向协议驱动 CLI，用来支持危险操作审批和中途打断。
- 第一轮讨论改为互相不可见的独立提案。
- 重复输出检测、按 CLI 的并发与限流、会话轮换、精确用量/费用预算，以及用户自定义 CLI 配置。轮换必须先具备完整恢复包，用量/费用预算必须先有可信的归一语义；这些不作为本次整改的前置条件。

### 本次评审修订范围

采用《方案评审1》的最终收敛意见及末轮三项补充：统一适配器能力/运行契约、指令持久化与干预语义，修正目录参数、禁止成员改 `.imates/`、主动清空失效会话。M2 仍包含助理起草；不拆 M2b，也不借此次修订引入危险审批、自动回滚、周期性会话轮换或精确费用硬上限。评审及本文修订均不等于真实 CLI 验收，未验证项见 CLI 记录与 M2 附录 G。
