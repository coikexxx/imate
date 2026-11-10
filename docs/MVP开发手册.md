# imates MVP 开发手册

> 版本：2026-11-10 · 状态：设计已确认，待出实施计划

## 1. 产品定义

imates 是一个桌面应用，让一个人像管理团队一样管理多个 AI Agent。每个项目有一组分工不同的 Agent，它们围绕用户下发的指令讨论、争辩、达成一致并落实工作。

用户能做的事：

- 在首页看到自己管理多少个项目、每个项目多少个 Agent、项目当前状态
- 进入项目，查看成员列表、项目文件列表
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
  fs_watch.rs
src-tauri/tests/fixtures/   录制的各 CLI 真实输出样本
src/
  pages/               Home.tsx、Project.tsx
  components/          MemberList、ChatPanel、FileTree、AgentForm、ProjectCard
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
- 角色设定写在首个回合提示词的开头，不依赖各家的系统提示参数。
- 提示词不走命令行参数（避免 Windows 下 `.cmd` 的转义问题），优先 stdin，其次临时文件。
- Windows 下启动进程加 `CREATE_NO_WINDOW`，避免闪黑窗。
- 取消与超时都通过杀进程树实现：Windows 用 `taskkill /T /F`，macOS 用进程组。
- 子进程输出按 UTF-8 逐行读取；无法解析为 JSON 的行记入运行日志，不中断回合。

### 各 CLI 的调用方式

下表来自各 CLI 的 `--help`，标"实测"的项在 M0 阶段逐个验证并回填本表。

| CLI | 无头调用 | 流式输出 | 续接 | 讨论模式（只读） | 执行模式（自动） | 本机状态 |
|---|---|---|---|---|---|---|
| claude | `claude -p` | `--output-format stream-json --verbose` | `--resume <id>` | `--permission-mode plan` | `--permission-mode bypassPermissions`（实测） | 已装 2.1.319 |
| codex | `codex exec -C <dir>` | `--json`（实测） | `codex exec resume <id>` | `-s read-only` | `-s workspace-write` | 已装 0.167.0 |
| grok | `grok -p --cwd <dir>` | `--output-format streaming-json` | `-r <id>` | `--permission-mode plan` | `--permission-mode bypassPermissions`（实测） | 已装 1.0.77 |
| dsh | `dsh headless "<任务>"` | 实测 | 实测（帮助里有 `--resume <session>`） | 实测 | 实测 | 已装 0.2.2-rc.1，仅 `.cmd` 垫片 |
| pi | 实测 | 实测 | 实测 | 实测 | 实测 | 未安装，M5 前需先装 |

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
3. **讨论轮**：被点名成员并行发言（讨论模式）。下一轮每人能看到上一轮全部发言。每轮结束组长裁定。达到轮数上限时，编排器告知组长"必须给出 assign 或 reply"。
4. **决议**：组长的 assign 包含决议摘要与任务清单，存为决议消息和任务记录。
5. **执行**：按任务顺序逐个执行（执行模式），成员的最终输出作为汇报写入群聊。组长也可以给自己派任务。
6. **验收**（讨论模式）：全部任务完成后组长审阅汇报并输出指令。

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

- 取回复中最后一个 `imates` 代码块。
- 缺失、JSON 非法、成员名不存在、action 在当前阶段不合法时，把具体错误发回组长要求重发，仅重试一次。
- 仍失败则把原文作为普通发言展示，指令状态置为"等待用户"，写一条系统消息说明原因。

### 上下文传递

每个 Agent 记录 `last_seen_message_id`。每回合的提示词由两部分组成：自上次以来群里的新消息（标明发言人），加上本回合的要求。历史不重复灌入。

### 提示词模板

`orchestrator/prompts.rs` 中维护六个模板：

- 角色开场：项目名、成员花名册、本人的名字、角色与职责。仅首个回合使用。
- 组长研判：附指令协议说明。
- 成员讨论：议题、要求给出立场和理由、可以反驳他人、不得修改文件。
- 组长裁定：当前轮次、剩余轮数。
- 成员执行：任务标题与详情、要求完成后给出简短汇报（做了什么、改了哪些文件、遗留问题）。
- 组长验收：各任务的编号与汇报（`rework` 用编号指明打回哪个任务）。

### 用户干预

- **停止**：杀掉该项目所有运行中的进程，运行记录标为已取消，写一条系统消息。
- **插话**：运行期间发送的消息入队，在下一个回合边界交给组长研判。
- **点名**：输入以 `@成员名` 开头的消息，绕过组长，由该成员以执行模式直接处理并回复。

### Agent 状态

`idle`（空闲摸鱼）/ `thinking`（进程已启动，尚无输出）/ `talking`（讨论模式下有文本输出）/ `working`（执行模式下运行中）/ `error`。状态变化通过事件推给前端，同时驱动成员列表和像素动画。

### 成本护栏

可在设置页调整：讨论轮数上限（默认 3）、单任务返工上限（默认 2）、单回合超时（默认 10 分钟）。CLI 返回用量时记入运行记录并在成员卡片上展示累计值。

## 5. 数据模型

SQLite 文件位于应用数据目录。项目文件留在用户选择的文件夹里，应用不复制。

```sql
projects(id, name, folder_path,
         status,                             -- idle / discussing / executing / waiting_user
         created_at, updated_at)

agents(id, project_id, name, role, duty, cli, model, is_leader,
       session_id, last_seen_message_id, seat_index, created_at)

messages(id, project_id, sender_type,        -- user / agent / system
         sender_agent_id,
         kind,                               -- instruction / chat / discussion / decision / report / system
         content, round, created_at)

tasks(id, project_id, instruction_message_id, assignee_agent_id,
      title, detail, order_index,
      status,                                -- pending / running / done / failed
      rework_count, report_message_id, created_at, finished_at)

runs(id, agent_id, mode,                     -- discuss / execute
     command, session_id, started_at, ended_at, exit_code,
     status,                                 -- running / ok / failed / timeout / cancelled / interrupted
     usage_json, error)

settings(key, value)
```

约束：同一项目内 Agent 名字唯一；每个项目恰好一个 `is_leader = 1`；每个项目最多 8 个 Agent。Agent 的实时状态只存在内存里，不落库。

## 6. 前后端接口

### Command

| 分组 | command |
|---|---|
| CLI | `list_clis` |
| 项目 | `list_projects`、`create_project`、`delete_project`、`get_project` |
| Agent | `create_agent`、`update_agent`、`delete_agent` |
| 对话 | `list_messages`、`send_message`、`stop_project` |
| 任务 | `list_tasks` |
| 文件 | `list_files`、`open_file` |
| 设置 | `get_settings`、`update_settings` |

`delete_project` 只删除应用内的记录，不动项目文件夹。

### Event

| event | 载荷 | 用途 |
|---|---|---|
| `message:new` | 完整消息 | 群聊追加 |
| `message:delta` | agent_id、文本片段 | 流式显示正在输出的发言 |
| `agent:status` | agent_id、状态 | 成员列表与像素动画 |
| `task:updated` | 完整任务 | 任务进度 |
| `files:changed` | project_id、变更路径 | 文件树刷新与高亮 |

## 7. 界面

### 首页

顶部一行总览（N 个项目 · M 个 Agent），下面是项目卡片网格。卡片显示名称、文件夹路径、Agent 数、当前状态（空闲 / 讨论中 / 执行中 / 等待用户）、最近活动时间。最后一张是"新建项目"，填名称并选择本地文件夹。

### 项目页

三栏布局：

- **左栏 成员**：像素头像、名字、角色、所用 CLI、状态、累计用量。底部"添加成员"。
- **中栏**：上半部分是像素办公室画布，下半部分是群聊消息流与输入框。运行中时发送按钮旁出现"停止"。
- **右栏 文件树**：只读展示项目文件夹，忽略 `.git`、`node_modules` 等目录。点击文件用系统默认程序打开。最近一次指令中被改动的文件高亮。

### 新建 Agent 表单

名字、角色（预设：组长 / 产品 / 开发 / 测试 / 撰稿 / 审校，可自定义）、职责描述、CLI（只列探测到的，未探测到的置灰并提示安装）、模型（可选，留空用 CLI 默认）。项目的第一个 Agent 默认是组长。

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

## 8. 异常处理

| 情况 | 处理 |
|---|---|
| CLI 未安装或已卸载 | 成员状态置为 error，提示重新选择 CLI |
| CLI 未登录、调用失败 | 该回合失败，原始报错写入系统消息 |
| 讨论轮中某成员失败 | 记为"未发言"，本轮照常继续 |
| 执行中任务失败 | 任务标为 failed，交给组长验收时决定打回或答复用户 |
| 组长回合失败 | 指令置为"等待用户"，可点重试 |
| 回合超时 | 杀进程树，按失败处理 |
| 应用重启 | 状态为 running 的运行记录标为 interrupted，写系统消息，指令置为"等待用户" |
| 项目文件夹不存在 | 项目卡片标红，进入后提示重新选择文件夹 |

## 9. 测试策略

- **适配器**：M0 录制每个 CLI 的真实输出，存入 `tests/fixtures/`。单测逐行喂给 `parse_line`，断言产出的事件序列；另测 `build_command` 在两种模式、有无会话 ID 下的参数。
- **编排器**：`adapters/fake.rs` 按脚本返回预设回复。用它覆盖全部路径：直接答复、讨论后派工、多轮讨论到上限、打回返工、指令块非法、成员失败、停止、插话。不消耗 token。
- **存储**：对内存 SQLite 测迁移与各表读写。
- **前端**：vitest 覆盖 store 的事件归并逻辑和工位排布算法。
- **端到端**：每个里程碑结束时，用真实 CLI 按验收标准手动走一遍。

## 10. 里程碑

| 阶段 | 内容 | 验收标准 |
|---|---|---|
| M0 CLI 实测 | 五个 CLI 各跑无头调用（pi 若未装则顺延到 M5），录制输出样本，回填第 3 节参数表 | 每个已装 CLI 都有参数表和样本文件 |
| M1 骨架 | SQLite 与迁移、项目与 Agent 的增删改、CLI 探测、首页和项目页外壳 | 能建项目、加成员，重启后数据还在 |
| M2 单 Agent 对话 | runner、claude 与 codex 适配器、流式群聊、停止、`@成员` 点名 | 点名一个成员能对话并在项目文件夹里改文件 |
| M3 团队协作 | 指令协议解析、提示词模板、编排状态机、任务记录、护栏 | 一条指令走完"讨论 → 决议 → 执行 → 验收 → 交付" |
| M4 文件与动画 | 文件监听与文件树、像素办公室 | 改动的文件高亮；状态变化实时反映到小人 |
| M5 其余适配器 | grok、dsh、pi | 五种 CLI 都能当成员参与讨论和执行 |
| M6 打磨打包 | 设置页、报错文案、用量展示、Win 与 mac 安装包 | 在干净机器上可安装使用 |

依赖关系：M0 → M1 → M2 → M3 是主线，必须按序。M4 的文件树只依赖 M1，像素办公室依赖 M2 的状态事件；M5 依赖 M2。M4 和 M5 可以在 M3 之后以任意顺序进行。

## 11. MVP 之后

按价值排序的候选项：并行执行（每个 Agent 独立工作副本后合并）、危险操作审批、通过 MCP 工具让 Agent 主动互相喊话、小人走动与会议桌、更多 CLI。
