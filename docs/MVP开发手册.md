# imates MVP 开发手册

> 版本：2026-11-10（第 3 版，按参考项目调研加固适配层与协作流程）· 状态：M1 已完成
> 实施计划：[M0 CLI 实测](plans/2026-11-10-M0-CLI实测.md)、[M1 骨架](plans/2026-11-10-M1-骨架.md)
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
| 权限 | 项目目录内全自动，不弹审批 |
| 像素动画 | 办公室场景，小人固定工位、不走动 |
| 编排核心 | Tauri 的 Rust 后端 |
| 项目规章 | 创建项目时必经的一步，存为项目文件夹里的 `.imates/rules/*.md` |
| Agent 人设 | 四段式文档（定位、职责、边界、工作风格），存数据库 |
| 起草方式 | 内置助理 Agent 生成草稿，用户确认后才保存 |

### MVP 不做

并行执行、MCP 工具协议、危险操作审批、小人走动、Agent 间私聊、云同步、多窗口。

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

原则：所有状态和进程都归 Rust 管，前端只渲染和发请求。界面刷新或崩溃不影响运行中的 Agent。

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
pub enum RunMode { Discuss, Execute }   // 只读 / 项目目录内可写

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
    TurnCompleted { final_text: String, usage: Option<Usage> },
    Error { message: String },
}

pub trait CliAdapter: Send + Sync {
    fn id(&self) -> &'static str;                       // "claude" / "codex" / ...
    fn detect(&self) -> Option<CliInfo>;                // 路径与版本，未安装返回 None
    fn build_command(&self, req: &TurnRequest) -> CommandSpec;
    fn parse_line(&mut self, line: &str) -> Vec<AgentEvent>;
}
```

### 进程模型

- 一个回合一个进程，跑完即退。Agent 的记忆靠各 CLI 自带的会话续接。
- 首个回合从输出里拿到会话 ID，存入 `agents.session_id`，之后每回合带上它续接。
- 人设与项目规章写在首个回合提示词的开头，不依赖各家的系统提示参数。
- 提示词不走命令行参数，优先 stdin，其次临时文件。原因有三：Windows 下 `.cmd` 垫片收不了多行参数；以 `-` 开头的提示词会被当成选项；命令行有长度上限。
- 工作目录一律通过子进程的 `current_dir` 设置，不依赖各 CLI 自己的目录参数（有的 CLI 在续接时不接受它）。
- 项目文件夹不保证是 git 仓库，适配器的参数要让 CLI 在非 git 目录里也能启动。
- Windows 下启动进程加 `CREATE_NO_WINDOW`，避免闪黑窗。
- 取消与超时都通过杀进程树实现：Windows 用 `taskkill /T /F`，macOS 用进程组。
- 子进程输出按 UTF-8 逐行读取；无法解析为 JSON 的行记入运行日志，不中断回合。
- 回合成功的判据：收到 `TurnCompleted` 且进程退出码为 0，两者缺一都按失败处理。stdout 关闭后进程 30 秒内仍不退出就杀掉，按失败处理。失败回合里已经输出的文本保留，随失败消息一起展示。
- 适配器在会话开始的事件里核对实际生效的权限模式。与请求的模式不符时产出 `Error`，回合按失败处理。有的 CLI 在请求的模式不可用时会悄悄降级，写文件全被拒绝但结果仍报成功。
- `TurnCompleted.usage` 一律是本回合的用量。有的 CLI 报的是会话累计值，由适配器减去上一回合的累计值后再上报。

### 各 CLI 的调用方式

下表来自各 CLI 的 `--help`，标"实测"的项在 M0 阶段逐个验证并回填本表。

| CLI | 无头调用 | 流式输出 | 续接 | 讨论模式（只读） | 执行模式（自动） | 本机状态 |
|---|---|---|---|---|---|---|
| claude | `claude -p` | `--output-format stream-json --verbose` | `--resume <id>` | `--permission-mode plan` | `--permission-mode bypassPermissions`（实测） | 已装 2.1.319 |
| codex | `codex exec --skip-git-repo-check`（实测） | `--json`（实测） | `codex exec resume <id>` | `-c sandbox_mode="read-only"`（实测） | `-c sandbox_mode="workspace-write" -c approval_policy="never"`（实测） | 已装 0.167.0 |
| grok | `grok -p --cwd <dir>` | `--output-format streaming-json` | `-r <id>` | `--permission-mode plan` | `--permission-mode bypassPermissions`（实测） | 已装 1.0.77 |
| dsh | `dsh headless "<任务>"` | 实测 | 实测（帮助里有 `--resume <session>`） | 实测 | 实测 | 已装 0.2.2-rc.1，仅 `.cmd` 垫片 |
| pi | 实测 | 实测 | 实测 | 实测 | 实测 | 未安装，M5 前需先装 |

codex 一行参考了开源项目 The Pair 在 codex 0.166 上的实测结论，M0 要在本机版本上逐项核对：

- `exec resume` 不接受 `-s`，沙箱模式只能用 `-c sandbox_mode=` 指定。首回合和续接回合统一用这种写法。
- 不在 git 仓库里时，不加 `--skip-git-repo-check` 会拒绝启动。
- 用户自己的配置若要求审批，无头的执行回合会卡在没人回答的审批上，所以执行模式显式指定 `approval_policy="never"`，由沙箱负责边界。
- 用量是线程累计值，不是本回合的值。

claude 一行也有两点待 M0 核对：`plan` 模式下模型会不会把讨论发言写成"计划"；会话开始事件里的权限模式字段是否如实反映生效的模式。

某个 CLI 没有只读模式时的兜底：讨论模式仍用它的默认模式运行，并在提示词里明确要求"本回合只讨论，不要修改任何文件"。

### 安全边界

"限制在项目目录内"的强度取决于各 CLI 自己的沙箱，应用层不做额外隔离。创建 Agent 的界面上要写明这一点。

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

解析规则：

- 只在回合的最终文本（`TurnCompleted.final_text`）里找指令，不看工具调用之间的过程文字。
- 取其中最后一个 `imates` 代码块。没有代码块时，按花括号配对从后往前扫描，取最后一个含 `action` 字段的 JSON 对象。
- 解析 JSON 前先修掉常见的手写瑕疵：中文或弯引号换成直引号，去掉对象和数组末尾多余的逗号。
- 成员名比对前去掉开头的 `@` 和首尾空白，忽略大小写。
- `discuss.to` 里有不存在的成员名时，丢弃这些名字并写一条系统消息说明，其余照常执行；一个有效成员都不剩才算错误。
- 以下情况算错误，把具体错误发回组长要求重发，仅重试一次：
  - 找不到指令，或 JSON 修复后仍非法。
  - action 在当前阶段不可用（见上一小节的表）。
  - `assign` 的任务数不在 1 到 8 之间，某个任务的 `to` 不是现有成员，或 `title` 为空。任务不能像讨论点名那样悄悄丢弃。
  - `rework` 的 `task_id` 不属于本条指令，或该任务已达返工上限。
  - `reply` 的 `message` 为空。
- 重发后仍错误，则把原文作为普通发言展示，指令状态置为"等待用户"，写一条系统消息说明原因。

### 上下文传递

每个 Agent 记录 `last_seen_message_id`。每回合的提示词由两部分组成：自上次以来群里的新消息（标明发言人），加上本回合的要求。历史不重复灌入。

游标的推进规则（讨论轮是并行的，规则不严会漏消息）：

- 游标取构建本回合提示词那一刻的最大消息 ID，不取回合结束时的。否则别的成员在本回合进行期间发的消息会被跳过。
- 回合成功才推进游标。回合失败时游标不动，下一回合重新带上这些消息。
- 该 Agent 自己发的消息不回灌给它。

规章与人设的变更也走这条通道（详见第 5 节）：规章改动会产生一条带全文的系统消息；人设改动会把该 Agent 标记为待更新，下一回合提示词开头附上新人设。

### 提示词模板

`orchestrator/prompts.rs` 中维护六个模板：

- 角色开场：项目名与项目描述、项目规章全文、本人的人设全文、成员花名册（其他成员只列名字、角色、定位和边界）。仅首个回合使用；会话续接失败后重开会话时再用一次（见第 9 节）。
- 组长研判：附指令协议说明。写明能直接答复就 `reply`，不要为了显得忙而发起讨论或派工；派工时每个任务的 `detail` 要写清要做什么、交付什么、怎样算完成。
- 成员讨论：议题、要求给出立场和理由、可以反驳他人、不得修改文件。
- 组长裁定：当前轮次、剩余轮数。
- 成员执行：任务标题与详情、要求完成后给出简短汇报（做了什么、改了哪些文件、遗留问题）。写明成员只汇报自己的任务，不能宣布整条指令完成。
- 组长验收：各任务的编号与汇报（`rework` 用编号指明打回哪个任务），要求对照项目规章和各成员的边界检查，违反规章或越界的打回。

所有成员回合（讨论和执行）的模板里都带两句话：

- 本次调用只是你的一次发言，调度由应用负责。说完就结束，不要等其他成员回复，不要反复查看文件等别人的进展。
- 以本回合的要求为准。群消息里更早的计划和发言只是参考，不是待办。

### 用户干预

- **停止**：杀掉该项目所有运行中的进程，运行记录标为已取消，写一条系统消息。
- **插话**：运行期间发送的消息入队，在下一个回合边界交给组长研判。
- **点名**：输入以 `@成员名` 开头的消息，绕过组长，由该成员以执行模式直接处理并回复。

### Agent 状态

`idle`（空闲摸鱼）/ `thinking`（进程已启动，尚无输出）/ `talking`（讨论模式下有文本输出）/ `working`（执行模式下运行中）/ `error`。状态变化通过事件推给前端，同时驱动成员列表和像素动画。

### 成本护栏

可在设置页调整：

| 护栏 | 默认值 | 说明 |
|---|---|---|
| 讨论轮数上限 | 3 | 按整条指令累计 |
| 单任务返工上限 | 2 | |
| 单条指令回合总数上限 | 30 | 组长和成员的回合都计入。达到后指令置为"等待用户"并写系统消息。兜住"验收后反复追加派工"这类没有单独上限的循环 |
| 无输出超时 | 5 分钟 | 子进程这么久没有任何输出就杀掉 |
| 回合总时长上限 | 讨论 10 分钟，执行 30 分钟 | 有输出也不能超过 |

CLI 返回用量时记入运行记录并在成员卡片上展示累计值。

## 5. 规章制度与人设

### 内置助理

应用自带一个助理 Agent，专门起草规章和人设。它不属于任何项目、不占工位、不出现在群聊里。

- 所用 CLI 在设置页指定（`assistant_cli`），默认取探测到的第一个，顺序为 claude、codex、grok、dsh、pi。
- 以讨论模式（只读）在项目文件夹里运行，文件夹里已有内容时会先了解现状再起草。
- 只产出草稿，由用户确认后应用才保存。助理自己不写任何文件。
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
- 规章被修改（应用内编辑，或 fs_watch 发现 `.imates/rules/` 下的文件变化）时，向群聊写一条系统消息，内容是变更文件的最新全文。所有 Agent 下一回合经"新消息"通道收到。界面上这类系统消息默认折叠。

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
- 人设被修改时置 `agents.persona_dirty = 1`，该 Agent 下一回合的提示词开头附上新人设全文，然后清除标记。
- 有成员加入、离开或人设变更时，向群聊写一条系统消息，更新花名册摘要。

**约束强度**：边界是提示词层面的软约束。越界由组长在验收环节纠正，应用不做硬性拦截。

**不用 AI 的路径**：六个预设角色（组长、产品、开发、测试、撰稿、审校）各有一份内置人设模板。

## 6. 数据模型

SQLite 文件位于应用数据目录。项目文件和规章文件留在用户选择的文件夹里，应用不复制。

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

约束：同一项目内 Agent 名字唯一；每个项目恰好一个 `is_leader = 1`；每个项目最多 8 个 Agent。Agent 的实时状态只存在内存里，不落库。

ID 约定：`messages.id` 是自增整数，兼作"新消息"游标，`agents.last_seen_message_id` 指向它；其余表的 `id` 是 UUID 文本。`runs.agent_id` 可为空，内置助理的运行不属于任何成员。

## 7. 前后端接口

### Command

| 分组 | command |
|---|---|
| CLI | `list_clis` |
| 项目 | `list_projects`、`create_project`、`update_project`、`delete_project`、`get_project` |
| Agent | `create_agent`、`update_agent`、`delete_agent` |
| 规章 | `list_rules`、`save_rule`、`delete_rule`、`list_rule_templates` |
| 助理 | `draft_rules`、`draft_persona`、`cancel_draft`、`list_persona_templates` |
| 对话 | `list_messages`、`send_message`、`stop_project` |
| 任务 | `list_tasks` |
| 文件 | `list_files`、`open_file` |
| 设置 | `get_settings`、`update_settings` |

`delete_project` 只删除应用内的记录，不动项目文件夹（包括其中的 `.imates/rules/`）。

`draft_rules` 和 `draft_persona` 都接受一个可选的 `feedback` 参数，带上时表示按反馈重写上一版草稿。两者在助理回合结束后返回解析好的草稿，期间通过 `assistant:delta` 推送进度。向导中项目尚未入库，所以这两个 command 的输入（文件夹、项目描述、规章、已有成员人设）全部由参数传入，不从数据库读取。

### Event

| event | 载荷 | 用途 |
|---|---|---|
| `message:new` | 完整消息 | 群聊追加 |
| `message:delta` | agent_id、文本片段 | 流式显示正在输出的发言 |
| `agent:status` | agent_id、状态 | 成员列表与像素动画 |
| `task:updated` | 完整任务 | 任务进度 |
| `files:changed` | project_id、变更路径 | 文件树刷新与高亮 |
| `rules:changed` | project_id | 规章面板刷新 |
| `assistant:delta` | 文本片段 | 起草对话框里显示助理正在输出的内容 |

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

名字、角色（预设：组长 / 产品 / 开发 / 测试 / 撰稿 / 审校，可自定义）、CLI（只列探测到的，未探测到的置灰并提示安装）、模型（可选，留空用 CLI 默认）、人设。项目的第一个 Agent 默认是组长。

人设区是一个四段式编辑器。选预设角色时自动填入对应模板；点"让助理生成"则由助理参考项目描述、规章和已有成员起草，可编辑或给反馈重写。四段都非空才能保存。表单上注明：CLI 的沙箱强度各不相同，边界是软约束。

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
| 组长回合失败 | 先自动重试一次（组长回合都是只读的讨论模式，重试没有副作用）。报错明确指向未登录时不重试。仍失败则指令置为"等待用户"，可点重试 |
| 回合超时（无输出超时或总时长超限） | 杀进程树，按失败处理 |
| stdout 已关闭但进程 30 秒内不退出 | 杀进程树，按失败处理，已输出的文本保留 |
| 实际权限模式与请求的不符 | 该回合失败，系统消息说明这个 CLI 或模型不支持所需的模式，提示更换 |
| 会话续接失败（CLI 报会话不存在或已损坏） | 失败发生在任何工具调用之前时：清空该 Agent 的 `session_id`，用角色开场加最近 20 条群消息开新会话，本回合重试一次。否则按失败处理 |
| 达到单条指令回合总数上限 | 指令置为"等待用户"，写系统消息说明 |
| 应用重启 | 状态为 running 的运行记录标为 interrupted，写系统消息，指令置为"等待用户" |
| 项目文件夹不存在 | 项目卡片标红，进入后提示重新选择文件夹 |
| 助理起草失败或草稿格式不对 | 展示原始输出和报错，可重试，也可改用模板或手写，不阻塞向导 |
| 没有探测到任何 CLI | "让助理起草"按钮置灰并说明原因，模板和手写仍可用 |
| `.imates/rules/` 被外部清空 | 规章面板显示为空并提示补充；Agent 开场白里注明"本项目暂无规章" |

## 10. 测试策略

- **适配器**：M0 录制每个 CLI 的真实输出，存入 `tests/fixtures/`。单测逐行喂给 `parse_line`，断言产出的事件序列；另测 `build_command` 在两种模式、有无会话 ID 下的参数。
- **编排器**：`adapters/fake.rs` 按脚本返回预设回复。用它覆盖全部路径：直接答复、讨论后派工、多轮讨论到上限、打回返工、指令块非法、成员失败、停止、插话、任务失败后取消余下任务、组长回合失败后自动重试、达到回合总数上限、会话续接失败后重开会话。另测并行讨论下游标的推进（别人在本回合期间发的消息下一回合能看到，自己的发言不回灌）。指令解析单独测：无代码块时的花括号扫描、弯引号与尾逗号修复、成员名归一、点名了不存在的成员、各 action 的校验规则。不消耗 token。
- **助理**：用假适配器测规章草稿的分隔行解析（正常、缺 END、无 FILE 行、正文里含代码块）和人设四段校验；测开场白里规章、人设、花名册摘要的拼装，以及规章变更消息和 `persona_dirty` 的更新通知。
- **存储**：对内存 SQLite 测迁移与各表读写；对临时目录测规章文件的列出、保存、删除。
- **前端**：vitest 覆盖 store 的事件归并逻辑和工位排布算法。
- **端到端**：每个里程碑结束时，用真实 CLI 按验收标准手动走一遍。

## 11. 里程碑

| 阶段 | 内容 | 验收标准 |
|---|---|---|
| M0 CLI 实测 | 五个 CLI 各跑无头调用（pi 若未装则顺延到 M5），录制输出样本，回填第 3 节参数表 | 每个已装 CLI 都有参数表和样本文件 |
| M1 骨架 | SQLite 与迁移、CLI 探测、首页和项目页外壳、新建项目三步向导、规章文件读写与规章面板、人设编辑器、内置模板（此阶段不接助理） | 能用模板和手写走完向导建出带规章和成员的项目，重启后数据还在 |
| M2 单 Agent 对话 | runner、claude 与 codex 适配器、流式群聊、停止、`@成员` 点名、开场白注入规章与人设、助理起草规章和人设（含按反馈重写） | 助理能起草规章和人设；点名一个成员能对话并在项目文件夹里改文件，且回答体现其人设 |
| M3 团队协作 | 指令协议解析、提示词模板、编排状态机、任务记录、护栏、验收对照规章、应用内修改规章或人设后的更新通知 | 一条指令走完"讨论 → 决议 → 执行 → 验收 → 交付"；改规章后 Agent 下一回合按新规章行事 |
| M4 文件与动画 | 文件监听与文件树、外部修改规章文件的检测、像素办公室 | 改动的文件高亮；状态变化实时反映到小人 |
| M5 其余适配器 | grok、dsh、pi | 五种 CLI 都能当成员参与讨论和执行 |
| M6 打磨打包 | 设置页、报错文案、用量展示、Win 与 mac 安装包 | 在干净机器上可安装使用 |

依赖关系：M0 与 M1 互不依赖，可以并行；M2 同时依赖 M0 和 M1；M2 → M3 必须按序。M4 的文件树只依赖 M1，像素办公室依赖 M2 的状态事件；M5 依赖 M2。M4 和 M5 可以在 M3 之后以任意顺序进行。

## 12. MVP 之后

按价值排序的候选项：并行执行（每个 Agent 独立工作副本后合并）、危险操作审批、通过 MCP 工具让 Agent 主动互相喊话、小人走动与会议桌、更多 CLI。

调研参考项目后记下的候选项（依据见[参考项目调研](参考项目调研.md)）：

- 验收阶段允许组长用 `discuss` 请成员只读复核别人的产出。
- 失败细分类：区分限流、网络、服务端故障，按 CLI 报出的等待时间退避重试。
- 常驻进程加双向协议驱动 CLI，用来支持危险操作审批和中途打断。
- 第一轮讨论改为互相不可见的独立提案。
