# CLI 实测记录

> M0 产出。记录各 CLI 无头模式的实际行为，是适配器实现的依据。
> 实测日期：2026-11-10 · 平台：Windows 11 · pi 未安装，顺延到 M5。

## 证据范围与待验证项

本文件原有命令和行为是 Windows 11 的 M0 历史记录；保留原命令中的 `-C/--cwd` 等参数，不按设计改写实测证据。M2 macOS 实测另列于后，不能外推 Windows。M2 应用通过子进程 current_dir 设置目录，省略目录参数后的结论仅对已记录的平台和版本成立。

| 平台/CLI | 已有证据 | 待验证项 | 状态 |
|---|---|---|---|
| Windows / claude 2.1.319 | init 回显模式、同模式续接；M0 的 plan 拒绝项目文件但写入计划目录；2026-11-11 的跨模式 Write 工具层拒绝与成功写入，以及 `result.usage` / 费用语义 | 应用内 Discuss 走查 | 部分通过 |
| Windows / codex 0.167.0 | 新线程 read-only/workspace-write 行为、续接记忆；thread.started 不回显权限 | G1 resume 权限覆盖及 Discuss → Execute → Discuss；G2 累计/本回合语义；非 git cwd；新 runner 清理 | 未验证 |
| Windows / grok 1.0.77、dsh 0.2.2-rc.1 | M0 场景；M5 适配器与测试已落地，dsh 首回合 cwd 正确 | grok 跨模式工具仍拒写；dsh 临时账号补丁三回合通过，但应用原 headless 认证路径未改；团队窗口验收未完成 | M5 验收受阻 |
| macOS / Claude Code 2.1.322、Codex 0.167.0 | G1/G2 CLI 基线；G3 Claude 模式回显和续接；M2 应用对话、点名、停止与草稿保存见后文 | Claude 工具层拒写；Finder/Dock PATH；异常退出后的实际恢复 | 部分通过 |
| pi | M0 未安装 | M5 安装登录后重新实测 | 未执行 |

判定规则：权限回显只能证明回显内容，文件写入行为只对已记录的平台/CLI 版本/模式成立；能写上级目录只证明项目目录不是可靠边界，不能推断全盘写权限。用量字段存在不等于其语义已确认；第二回合数值变大不能证明累计，无法判定时归一值保持 None、保留 rawUsage。

配置继承：Claude init 输出过本机技能，尚无证据证明全部 hooks、MCP 或插件启用；MVP 保留 CLI 原生配置继承，不宣称已隔离用户/项目指令。完整验证步骤见 [M2 附录 G](plans/2026-11-10-M2-单Agent对话.md#附录-g版本与平台实测门)。

## claude

- 版本：2.1.319（Claude Code）
- 可执行文件：npm 全局命令 `claude`（`%APPDATA%\npm\claude`）
- 提示词传入方式：`claude -p`，提示词经 stdin
- 工作目录指定方式：没有目录参数。先 `cd` 到目标目录再启动。`system/init` 的 `cwd` 就是这个目录

### 命令

| 场景 | 完整命令 |
|---|---|
| 讨论（只读） | `claude -p --output-format stream-json --verbose --permission-mode plan`，提示词经 stdin |
| 执行（自动） | `claude -p --output-format stream-json --verbose --permission-mode bypassPermissions`，提示词经 stdin |
| 续接 | 在讨论命令后加 `--resume <session_id>`。续接后 `session_id` 不变 |

### 事件格式

`system/init` 会带上工具列表和本机技能，一行很长，下面只摘字段。

| 关注点 | 事件类型与字段路径 | 样本中的一行示例 |
|---|---|---|
| 会话 ID | 多数事件都有 `session_id`。会话一开始的 `system/init` 里就有，并带 `permissionMode` | `{"type":"system","subtype":"init","session_id":"7a83cd2c-cc9d-4f7f-aafb-06ad043be63b","permissionMode":"plan","model":"claude-sonnet-5-5"}` |
| 文本增量 | 本次没有逐段增量。正文在 `assistant.message.content[]` 里 `type` 为 `text` 的 `text`。帮助里有 `--include-partial-messages`，这次没开 | `{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"我是 Claude，一个在终端里协助你完成软件工程任务的 AI 助手。"}]}}` |
| 工具调用 | 同在 `assistant.message.content[]`，`type` 为 `tool_use`。读文件的 `name` 是 `Read`，`input.file_path`。写文件的 `name` 是 `Write`，还有 `input.content`。工具结果在下一条 `user.message.content[]`，`type` 为 `tool_result` | `{"type":"tool_use","name":"Read","input":{"file_path":"<HOME>\\AppData\\Local\\Temp\\imates-m0\\README.md"}}` |
| 回合结束与最终文本 | `type: result`。成功时 `subtype` 为 `success` 且 `is_error` 为 false，最终文本在 `result` | `{"type":"result","subtype":"success","is_error":false,"result":"我是 Claude……","session_id":"7a83cd2c-cc9d-4f7f-aafb-06ad043be63b"}` |
| 用量 | `result.usage` 的 `input_tokens`、`output_tokens`、`cache_read_input_tokens`、`cache_creation_input_tokens`。费用在 `result.total_cost_usd` | `{"input_tokens":4,"output_tokens":234,"cache_read_input_tokens":29991,"cache_creation_input_tokens":30228}` |

### 行为

- 只读模式是否拦住写文件：工作目录里的 `blocked.txt` 没有创建。`plan` 模式下模型改用 `Write` 把计划写到了用户目录的 `.claude/plans/`。所以 `plan` 挡住了任务文件，没有挡住 Claude 自己的计划目录。测完已删除那份计划文件。
- 执行模式是否全程无确认：是。`bypassPermissions` 直接调用 `Write`，进程没有挂起，退出码 0，`hello.txt` 内容为 `hi`，stderr 为空。`system/init.permissionMode` 与请求的模式一致。
- 执行模式能否写到工作目录之外：能。同一权限用 `Write` 在上一级目录创建了 `imates-m0-escape.txt`。测完已删除。
- 续接是否生效：生效。`--resume` 续接 execute 的会话后，`session_id` 仍是 `cfba7141-e804-4a67-b6ea-50623844e7c4`，回答提到创建了内容为 hi 的 `hello.txt`。
- 出错时的表现（退出码、stdout、stderr）：`--model no-such-model` 退出码 1。stderr 有一行 `[claude-code:unrecognized_model]`。stdout 仍有 JSON 事件，最后 `result` 的 `subtype` 仍是 `success`，但 `is_error` 为 true，`result` 文本说明模型不存在。`error.jsonl` 用 `2>&1` 合并，所以中间有 1 行纯文本。
- stdout 是否每行都是合法 JSON：discuss、execute、resume 是。error 因为并进了 stderr，有 1 行纯文本。
- 其他注意事项：`--permission-mode` 的可选值是 `acceptEdits`、`auto`、`bypassPermissions`、`manual`、`dontAsk`、`plan`。登录过期时进程退出码 1，`result.is_error` 为 true，`terminal_reason` 为 `api_error`。重新 `claude auth login` 之后，上述场景都跑通了。样本里的用户主目录已换成 `<HOME>`。`system/init` 的 `apiKeySource` 不是令牌本身。

## codex

- 版本：codex-cli 0.167.0
- 可执行文件：npm 全局命令 `codex`（`%APPDATA%\npm\codex`）
- 提示词传入方式：`codex exec` 的 PROMPT 参数写成 `-`，或省略 PROMPT，从 stdin 读取
- 工作目录指定方式：`-C` / `--cd <dir>`

### 命令

| 场景 | 完整命令 |
|---|---|
| 讨论（只读） | `codex exec --json -s read-only -C <dir> -`，提示词经 stdin |
| 执行（自动） | `codex exec --json -s workspace-write -C <dir> -`，提示词经 stdin |
| 续接 | `codex exec resume <thread_id> --json -`，提示词经 stdin。`resume` 不接受 `-s` |

### 事件格式

| 关注点 | 事件类型与字段路径 | 样本中的一行示例 |
|---|---|---|
| 会话 ID | `thread.started` 的 `thread_id` | `{"type":"thread.started","thread_id":"01a1c3f2-85eb-71d2-a133-6a6d4627d764"}` |
| 文本增量 | 没有逐段增量。整段文本在 `item.completed`，路径 `item.type=agent_message` 的 `item.text`。一个回合可以有多条 | `{"type":"item.completed","item":{"id":"item_0","type":"agent_message","text":"我是 Codex，一个能与你协作阅读代码、解决问题并完成开发任务的 AI 助手；我会读取当前目录下的 README.md 并复述其内容。\n"}}` |
| 工具调用 | 没有单独的读文件或写文件事件。读写都是 shell：`item.started` / `item.completed`，`item.type` 为 `command_execution`，命令在 `item.command`，输出在 `item.aggregated_output`。读 README 用的是 `Get-Content`，写 `hello.txt` 用的是 `Set-Content` | `{"type":"item.started","item":{"id":"item_1","type":"command_execution","command":"\"C:\\\\Program Files\\\\PowerShell\\\\7\\\\pwsh.exe\" -Command \"Get-Content -LiteralPath .\\\\README.md -Raw\"","aggregated_output":"","exit_code":null,"status":"in_progress"}}` |
| 回合结束与最终文本 | 最后一条 `agent_message` 的 `item.text` 是给用户的正文。`turn.completed` 只有用量，没有最终文本。失败时另有 `turn.failed`，错误在 `error.message` | `{"type":"turn.completed","usage":{"input_tokens":94593,"cached_input_tokens":54528,"cache_write_input_tokens":0,"output_tokens":399,"reasoning_output_tokens":84}}` |
| 用量 | `turn.completed.usage` 的 `input_tokens`、`cached_input_tokens`、`output_tokens`、`reasoning_output_tokens` | 同上 |

### 行为

- 只读模式是否拦住写文件：拦住了。`-s read-only` 下要求创建 `blocked.txt`，退出码 0，文件不存在，模型回复当前环境为只读、写入失败。
- 执行模式是否全程无确认：是。`-s workspace-write` 未加审批参数，直接执行 `Set-Content`，退出码 0，`hello.txt` 内容为 `hi`，stderr 为空。
- 执行模式能否写到工作目录之外：能。同一 `workspace-write` 在上一级目录创建了 `imates-m0-escape.txt`，命令退出码 0。测完已删除该文件。
- 续接是否生效：生效。续接 execute 会话后 `thread_id` 不变（`01a1c3f4-575d-7eb3-87e8-6237ed333189`），回答提到创建了 `hello.txt`。`codex exec resume --help` 没有 `-s`，本次续接未指定沙箱。
- 出错时的表现（退出码、stdout、stderr）：`--model no-such-model` 退出码 1。stdout 仍是 JSONL：先是 `item.completed` 且 `item.type` 为 `error`（找不到模型元数据），然后多次 `type: error`（404，模型不被支持，重试 5 次），最后 `turn.failed`。stderr 没有单独的非 JSON 文本。错误信息里的上游地址已换成 `<API_HOST>`。
- stdout 是否每行都是合法 JSON：是。discuss、execute、resume、error 四份样本的非 JSON 行都是 0。
- 其他注意事项：群聊不能按 token 流式显示，只能在每条 `agent_message` 完成时整段展示。工具输出里的中文可能乱码（PowerShell 代码页），最终 `agent_message` 的中文是正常的。`input_tokens` 单回合约 6 万到 9 万，含大量缓存读取。样本里的用户主目录已换成 `<HOME>`。

## grok

- 版本：grok 1.0.77（stable）
- 可执行文件：`%USERPROFILE%\.grok\bin\grok`
- 提示词传入方式：`--prompt-file <文件>`。这样调用会跑完就退出，没有进入交互界面。`-p` 把提示词放在参数里，本次没用。没有测 stdin。
- 工作目录指定方式：`--cwd <dir>`

### 命令

| 场景 | 完整命令 |
|---|---|
| 讨论（只读） | `grok --prompt-file <文件> --output-format streaming-messages-json --cwd <dir> --permission-mode plan` |
| 执行（自动） | `grok --prompt-file <文件> --output-format streaming-messages-json --cwd <dir> --permission-mode bypassPermissions` |
| 续接 | 在上一条命令后加 `-r <session_id>`。续接后 `session_id` 不变 |

### 事件格式

样本用的是 `streaming-messages-json`。`system/init` 那一行会带上全部工具名，很长，下面只摘字段。

| 关注点 | 事件类型与字段路径 | 样本中的一行示例 |
|---|---|---|
| 会话 ID | 第一行 `system/init` 的 `session_id`。结束事件里再出现一次，值相同 | `{"type":"system","subtype":"init","session_id":"01a1c3f9-0dcd-7242-ac16-9904ab7a6eec","model":"grok-4.7","permissionMode":"plan",...}` |
| 文本增量 | 这种格式没有逐段增量。正文在 `assistant.message.content[]` 里 `type` 为 `text` 的 `text` | `{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"我先读当前目录的 README.md，再用一句话介绍自己并复述文件内容。"}, ...]}}` |
| 工具调用 | 同在 `assistant.message.content[]`，`type` 为 `tool_use`，名字是 `name`，参数是 `input`，调用号是 `id`。读文件是 `read_file`，写文件是 `write`。工具结果在下一条 `user.message.content[]`，`type` 为 `tool_result` | `{"type":"tool_use","name":"read_file","id":"call-...","input":{"target_file":"<HOME>\\AppData\\Local\\Temp\\imates-m0\\README.md"}}` |
| 回合结束与最终文本 | `type: result`。成功时 `subtype` 为 `success`，`is_error` 为 false，最终文本在 `result`。失败时 `subtype` 为 `error_during_execution`，说明在 `errors` | `{"type":"result","subtype":"success","is_error":false,"result":"我是 xAI 开发的 Grok 4.7……","session_id":"01a1c3f9-0dcd-7242-ac16-9904ab7a6eec"}` |
| 用量 | `result.usage` 的 `input_tokens`、`output_tokens`、`cache_read_input_tokens`、`cache_creation_input_tokens` | `{"input_tokens":37525,"output_tokens":521,"cache_read_input_tokens":3200,"cache_creation_input_tokens":0,"server_tool_use":{"web_search_requests":0}}` |

### 行为

- 只读模式是否拦住写文件：工作目录里的 `blocked.txt` 没有创建。`plan` 模式最后停在计划，并收到工具结果 `Non-interactive plan-only run: plan mode stays on`。同一回合里模型用 `search_replace` 把计划写到了 grok 自己的会话目录 `plan.md`（在用户主目录的 `.grok/sessions/` 下，不在工作目录里）。所以 `plan` 挡住了任务文件，没有挡住会话目录。
- 执行模式是否全程无确认：是。`bypassPermissions` 直接调用 `write`，进程没有挂起，退出码 0，`hello.txt` 内容为 `hi`，stderr 为空。
- 执行模式能否写到工作目录之外：能。同一权限用 `write` 在上一级目录创建了 `imates-m0-escape.txt`。测完已删除。
- 续接是否生效：生效。`-r` 续接 execute 的会话后，`session_id` 仍是 `01a1c3fa-67ee-7753-b052-bd4b77b54600`，回答提到创建了内容为 hi 的 `hello.txt`。
- 出错时的表现（退出码、stdout、stderr）：`-m no-such-model` 退出码 1。stdout 先有 `system/init`（`model` 为 `no-such-model`），再有 `result`，`subtype` 为 `error_during_execution`，`errors` 为 `unknown model id`。stderr 另有一行同样的纯文本。`error.jsonl` 用 `2>&1` 合并，所以最后一行不是 JSON。
- stdout 是否每行都是合法 JSON：discuss、execute、resume 是。error 因为并进了 stderr，有 1 行纯文本。
- 其他注意事项：`--permission-mode` 的可选值是 `default`、`acceptEdits`、`auto`、`dontAsk`、`bypassPermissions`、`plan`。`--output-format` 还有 `plain`、`json`、`streaming-json`。对比过 `streaming-json`：事件类型是 `available_commands`、`thought`、`text`、`usage`、`tool_call`、`tool_call_update`、`end`，文本增量在 `{"type":"text","data":"…"}`，会话 ID 只在最后一条 `end.sessionId`。它的事件种类更多，所以样本和后续场景改用 `streaming-messages-json`。群聊若要逐字显示，适配器应改用 `streaming-json`，并在回合结束时才拿到会话 ID。顶层 `--prompt-file` / `-p` 是单回合无头。`grok agent` 是另一套进程（`stdio`、`headless` 的 WebSocket relay、`serve`、`leader`），不是这次用的调用方式。`system/init` 里的 `apiKeySource` 值是 `oauth`，没有令牌。样本里的用户主目录已换成 `<HOME>`。在 Grok 会话里再跑无头 `grok` 没有出现嵌套失败。

## dsh

- 版本：0.2.2-rc.1
- 可执行文件：npm 全局的 `dsh.cmd`（`%APPDATA%\npm\dsh.cmd`）。Git Bash 里要写成 `cmd //c "dsh ..."`
- 提示词传入方式：参数 `-` 表示读 stdin。帮助里的完整写法是 `dsh --profile headless`，直接写 `dsh headless` 也能启动
- 工作目录指定方式：没有目录参数。必须先 `cd` 到目标目录再启动，适配器用子进程的 `current_dir`

### 命令

2026-11-10 充值后再跑了一次。下面三条命令，加上 blocked 和 escape，模型回合都完成了。讨论、执行、续接的样本已脱敏入库。进程退出码打在终端里，没有落盘；这几次成功 stdout 的 `final.reason.kind` 都是 `completed`。

| 场景 | 完整命令 |
|---|---|
| 讨论（只读） | `dsh headless --json --permission-mode read-only --approval-policy never -`，提示词经 stdin |
| 执行（自动） | `dsh headless --json --permission-mode workspace-write --approval-policy never -`，提示词经 stdin |
| 续接 | `dsh headless --json --session-id <id> --permission-mode read-only --approval-policy never -`，提示词经 stdin |

### 事件格式

一个回合可以有多步。每步先有 `thinking` 和 `text` 或 `tool_call`，再有 `tool_result`，然后 `step_end` 带用量。`text` 是这一步的整段文字，不是逐 token。

| 关注点 | 事件类型与字段路径 | 样本中的一行示例 |
|---|---|---|
| 会话 ID | 第一行 `type: session` 的 `sessionId`，形如 `session-…`。`final.sessionId` 再出现一次，值相同。`cwd` 是启动时的工作目录 | `{"type":"session","sessionId":"session-af4b6cd9-c143-4e9a-a36e-2fbfeae47afe","cwd":"<HOME>\\AppData\\Local\\Temp\\imates-m0"}` |
| 文本增量 | 没有逐 token。整段在 `type: text` 的 `text`。另有 `type: thinking`，`text` 可以是空字符串 | `{"type":"text","text":"我叫 DeepSeek Harness 智能体，由 deepseek-flash 驱动，可以帮你阅读文件、编写代码、检索资料并执行多步骤任务。"}` |
| 工具调用 | `type: tool_call`。读文件的 `tool` 是 `read`，参数 `input.file_path`。写文件的 `tool` 是 `write`，还有 `input.content`。shell 的 `tool` 是 `pwsh`，命令在 `input.command`。结果是下一条 `type: tool_result`，用 `callId` 对应。失败时 `status` 为 `error` | `{"type":"tool_call","callId":"call_00_ET_Rq6f3fDjBGv2TBBOmFcX9928","tool":"read","input":{"file_path":"<HOME>\\AppData\\Local\\Temp\\imates-m0\\README.md"}}` |
| 回合结束与最终文本 | `status.phase` 为 `turn_end`，`reason.kind` 为 `completed`。最终文本在 `final.text`。失败时 `reason.kind` 为 `error`，`text` 为空 | `{"type":"final","sessionId":"session-af4b6cd9-c143-4e9a-a36e-2fbfeae47afe","reason":{"kind":"completed"},"text":"README.md 只有一行内容：…"}` |
| 用量 | `status.phase` 为 `step_end` 的 `usage`。字段是 camelCase：`inputTokens`、`outputTokens`、`cacheReadTokens`、`cacheWriteTokens`、`totalTokens` | `{"inputTokens":8154,"outputTokens":102,"cacheReadTokens":0,"cacheWriteTokens":0,"totalTokens":8256}` |

中间 `status.phase` 依次为 `turn_start`、`step_start`、`step_end`、`turn_end`。

### 行为

- 只读模式是否拦住写文件：拦住了。`read-only` 下 `write` 的 `tool_result.status` 为 `error`，`error.name` 为 `FsError`，`error.code` 为 `FS_SANDBOX_DENIED`，结果文本含 `file access denied under read-only mode`。`blocked.txt` 不存在。报错里提到可以升级沙箱再试一次，但 `--approval-policy never` 会自动拒绝需要审批的提权，文件没有被写成。
- 执行模式是否全程无确认：是。`workspace-write` 直接调用 `write`，进程没有挂起。`hello.txt` 内容为 `hi`。stderr 只有一行模型列表，没有报错。预设 `workspace-write` 的默认 approval 是 `ask`，这次用 `--approval-policy never` 盖掉。另有 `danger-full-access`，本次没有用。
- 执行模式能否写到工作目录之外：能。同一 `workspace-write` 用 `write` 在上一级目录创建了 `imates-m0-escape.txt`，读回内容为 `x`。测完已删除。
- 续接是否生效：生效。`--session-id` 续接 execute 的会话后，`sessionId` 仍是 `session-22cff073-10b6-4e6c-af0f-9aa9c35a230c`，`turn` 为 2。回答复述了上一回合要创建内容为 hi 的 `hello.txt`。未知 id 会报错。续接命令上仍然可以带 `--permission-mode`。
- 出错时的表现（退出码、stdout、stderr）：成功回合的进程退出码没有落盘。未知会话 `--session-id session-does-not-exist` 的合并输出有两行：第一行 JSON `{"type":"error","message":"session \"session-does-not-exist\" does not exist; omit --session-id to start a new Session"}`，第二行是同样文字，前缀 `dsh: `。它的退出码也没有落盘。更早没有密钥时，discuss 退出码是 1，错误码 `MISSING_CREDENTIAL`，那份输出已删除。余额不足时 `reason.error.code` 为 `QUOTA`、`status` 为 402，消息为 `Insufficient Balance`，`final.text` 为空；那次讨论、执行、续接的输出没有入库。
- stdout 是否每行都是合法 JSON：discuss 14 行、execute 18 行、resume 8 行都是。error 因为并进了 stderr，有 1 行纯文本。成功回合的 stderr 没有并进 stdout。
- 其他注意事项：headless 没有 `--model`。模型写在 profile 配置 `agent-default-model` 里，这次是 provider `deepseek-official`、model `deepseek-flash`。手册里的 `agents.model` 对 dsh 没有对应的命令行参数，适配器传了也不会生效。Windows 上 `bash` 工具关闭，启用的是 `pwsh`。`pwsh` 工具结果里的中文可能乱码，最终 `text` 的中文正常，样本里有「测试目录」。成功时 stderr 仍有一行 `[llm-deepseek-api-key] model listing from https://api.deepseek.com/models loaded 2 model(s), skipped 0 row(s)`，这只是模型列表提示，不是失败。样本里的用户主目录已换成 `<HOME>`。

## 对适配器设计的影响

1. claude 的 `-p`、codex 的 `-`、dsh 的 `-` 都从 stdin 读提示词，这次 dsh 的模型回合也完成了。grok 本次用的是 `--prompt-file`，文件方式可用，stdin 没测。`CommandSpec` 已有的 `stdin` 和 `prompt_file` 够用：grok 走 `prompt_file`，其余走 `stdin`。
2. claude 这次的 `stream-json` 没有逐段文本，整段在 `assistant` 消息里。帮助里的 `--include-partial-messages` 没开，不能据此否定增量能力。codex 没有文本增量。grok 选定的 `streaming-messages-json` 也是整段；它的 `streaming-json` 才有 `type:text` 的增量，会话 ID 要等最后的 `end.sessionId`。dsh 的 `type: text` 也是整段，群聊按事件整段显示。
3. claude 的 `session_id`、codex 的 `thread_id`、grok 的 `session_id`、dsh 的 `sessionId` 续接后都不变，适配器不用每回合回写。dsh 续接后 `turn` 会递增，这次记住了上一回合的 `hello.txt`。
4. codex 的 `-s read-only` 和 dsh 的 `read-only` 都拦住了工作目录里的写文件。claude 和 grok 的 `plan` 拦住了 `blocked.txt`，但分别把计划写进了 `.claude/plans/` 和 grok 自己的会话目录。这两个 CLI 的讨论模式不可靠，提示词里仍要写明本回合不要改文件。
5. claude 的 `bypassPermissions`、codex 的 `workspace-write`、grok 的 `bypassPermissions`、dsh 的 `workspace-write` 都能把文件写到工作目录之外。创建 Agent 的表单要保留这条安全提示。

没有发现 `CliAdapter` 必须加方法。dsh 的模型不在命令行上，适配器应忽略 `agents.model`。dsh 的续接参数是 `--session-id`。用量字段是 camelCase，和其他 CLI 的 snake_case 不同。适配器要把 `final.reason.kind` 为 `error`、且 `error.code` 为 `QUOTA` 或 `MISSING_CREDENTIAL` 当成回合失败，空的 `final.text` 不是成功回复。未知会话是另一条 `type: error`，stderr 会再打一行 `dsh: ` 前缀。成功时 stderr 的模型列表行不要当成失败。


## M2 实施前置验证（macOS，2026-11-10）

本节是本机重新执行的结果，不替代上面的 Windows M0 样本。平台 macOS 27.0.1（26A434）arm64；Codex 0.167.0、Claude Code 2.1.322。启动来源为 Codex 桌面任务的终端进程，尚未完成 Finder/Dock 启动环境验收。全部模型调用均在仓库外新建的非 git 临时项目目录，提示词经 stdin，工作目录通过子进程 cwd 设置，无 `-C`。原始 stdout/stderr 及会话文件留在临时目录和 CLI 自身会话目录，未提交。

### G1：Codex 沙箱与续接——已通过本机 CLI 基线

使用计划中的 `-c sandbox_mode=...`、`-c approval_policy="never"`、`--json --skip-git-repo-check`。同一线程顺序验证如下，所有 CLI 进程退出码均为 0，均收到 `turn.completed`，没有审批等待：

| 回合 | 模式 | 工具与文件事实 |
|---|---|---|
| 首次 | read-only | 命令实际尝试创建 blocked-first.txt，工具退出 1，`operation not permitted`，文件不存在 |
| 同模式续接 | read-only | 再次实际尝试创建 blocked-same.txt，工具退出 1，沙箱拒绝，文件不存在；正确复述上一回合文件名 |
| 同线程切换 | workspace-write | 命令退出 0，codex-created.txt 内容为 g1-execute；线程 ID 不变 |
| 同线程切回 | read-only | 分别尝试修改已有文件与创建新文件，两个工具均退出 1；原文件仍为 g1-execute，新文件不存在 |

另建独立 Execute 线程：首次创建 execute-first.txt，续接后更新为 second 并创建 execute-resumed.txt。文件事实、记忆与线程 ID 均符合预期，两个 CLI 进程都退出 0、收到完成事件。独立目录由 `git rev-parse` 确认不是 git 仓库。同一执行线程续接前后 `turn.completed.usage` 分别为 input 46875/output 244 与 input 94442/output 507；session token_count 累计同步增长。

注意：部分被拒绝的命令没有在 `--json` 中产生 `command_execution` 事件，但该线程的本地 session 记录含实际工具请求和 `exit_code: 1 / operation not permitted` 工具结果，stderr 也记录 seatbelt 拒绝。G1 使用这些工具证据与最终文件状态共同判断，没有仅凭模型正文判定。

### G2：Codex 用量——已证实本机版本为线程累计

同线程四次 `turn.completed.usage` 与各回合末 session `token_count.info.total_token_usage` 逐字段对应；不能将其直接当作每回合用量：

| 回合 | 累计 input_tokens | 累计 cached_input_tokens | 累计 output_tokens |
|---|---:|---:|---:|
| 首次 Discuss | 45612 | 26368 | 97 |
| 同模式 Discuss 续接 | 91674 | 72064 | 211 |
| 切到 Execute | 139444 | 118656 | 316 |
| 切回 Discuss | 237440 | 215424 | 569 |

session 同时提供 `last_token_usage`，它是最后一次模型请求，不等于整个 CLI 回合。实现须保存 rawUsage 累计读数，同一 Agent/session 的后续回合取与最近可靠基线的差；新线程首回合以零为基线。续接缺基线、字段缺失或计数下降时归一值保持 None，不能强行当零。缓存读为输入子集，不重复相加；无费用字段则 costUsd=None。Windows/其他版本不能仅凭本机结论宣称已验收。

### G3：Claude 模式、续接与用量——部分通过，工具层拒写未验证

同一 session 顺序执行 plan → bypassPermissions → plan，每次 `system/init.permissionMode` 与请求一致、session ID 不变、退出码 0、`result.is_error=false`。

- plan 首回合要求实际调用 Write 创建测试文件，模型明确拒绝调用工具，文件不存在。
- bypassPermissions 续接实际调用 Write，工具返回创建成功，hello-claude.txt 内容为 g3-execute，并正确复述首回合文件名。
- 切回 plan 要求改写旧文件及创建新文件，模型再次拒绝调用工具；原文件内容不变，新文件不存在。追加一次普通创建请求仍无工具调用。
- 因三次 plan 请求均未触发写工具，本机只证实权限回显及模型遵循讨论意图，**没有取得工具层拒写证据**。不能把 G3 的“写入先拒绝、再成功、再次拒绝”全部标为通过，也不能声称 plan 是文件系统隔离。
- 用量对照：`result.usage` 的 input/output/cache 字段与 `model_usage_since_last_result` 对应；`total_cost_usd` 在同 session 内累计（0.273949 → 0.3301658 → 0.365098 → 0.3801448）。后续回合费用分别为 0.0562168、0.0349322、0.0150468，与各回合 `model_usage_since_last_result.*.costUSD` 对应。实现不得将 total_cost_usd 直接当本回合费用；没有可靠 since-last-result 字段时费用保持 None。

### 额外可观察行为

Codex 运行期间 stdout 出现了两个 `item.type=error` 配置诊断事件，但后续仍成功执行命令并发出 `turn.completed`，进程退出码为 0。适配器不能把所有该类事件直接视为致命错误；对可识别的配置警告保留脱敏诊断并继续，`turn.failed`、明确模型错误仍锁定失败。不修改用户全局配置。

CLI stderr 出现插件元数据警告及远程插件缓存 401，实际模型回合仍成功。这只证明观测到了这些诊断，不证明全部 hooks/MCP/插件启用，也不等于模型登录失败。

### 尚未执行项（截至 2026-11-10 前置验证）

G3 的 Finder/Dock 环境、runner 进程树取消/超时/正常退出、异常恢复和 UI 交互仍待对应 Task；本次尚未写生产代码。Claude 工具层拒写的验收口径需先解决，不能标 M2 完成。

## M2 实施验收补记（macOS，2026-11-11）

环境为 macOS 27.0.1（26A434）arm64，Claude Code 2.1.322、Codex 0.167.0。通过 `open` 从开发终端启动本机调试应用；这不等同于 Finder/Dock 脱离终端环境的启动验收。所有真实 CLI 交互均在仓库外的临时项目中完成，原始响应没有进入仓库。

- Claude 与 Codex 均完成文件创建、读取及同一 CLI 会话续接；`@成员` 消息实际路由到 Codex 成员。
- UI Stop 后 run 记为 `cancelled`，运行所有权记录被清除。runner 自动化已覆盖真实测试子进程的 stdin、静默超时、工具超时、持续 stderr、长行、EOF 宽限、取消和 Unix 进程树清理；异常杀 app 后的真实残留与恢复没有手动测试。
- 助理实际生成四段人设并用反馈续接同一会话，确认后才保存。规章草稿应用到编辑器不写磁盘，只有明确确认保存后才写入。取消向导和成员表单均未留下保存结果。
- 代码测试覆盖启动恢复、interrupted 状态、运行所有权及同目录清理确认阻塞；app 异常退出后的实际重启和人工清理确认仍未手动验收。本节没有 Windows runner 结果，见下一节。
- Claude `system/init.permissionMode` 回显与续接已核对，但 Discuss 请求没有触发写工具，所以本机没有工具层拒写证据；该能力状态为“模式已核对、工具层拒写未验证”，不宣称硬隔离。

以上仅适用于所记 macOS/CLI 版本和启动路径。macOS 上的 Claude 工具层拒写、Finder/Dock PATH 和异常恢复手验仍未完成。Windows 补测见下一节，macOS 上的这些项仍以本节为准。

## M2 Windows 验收补记（2026-11-11）

环境为 Windows 11 10.0.26300，Claude Code 2.1.319。真实 CLI 在仓库外的临时目录运行，直接启动 npm 全局垫片所指向的 `claude.exe`。本机垫片的入口是 `bin/claude.exe`，不是旧的 `cli.js`；解析器两种都接受，且不经过 `cmd.exe`。参数与适配器首回合一致：`-p --output-format stream-json --verbose --permission-mode plan`，提示词经 stdin，工作目录由进程 cwd 设置，无 `-C`。原始输出留在临时目录，未提交。这次没有打开应用窗口。

### Claude plan 模式

两次请求的进程退出码都是 0，`permissionMode` 都是 `plan`，stderr 都为空，目标文件都不存在，输出里都没有计划目录。

- 第一次模型没有调用工具。这条只记为文字拒绝。
- 第二次模型调用了 `Write`，写入目标是临时目录中的 `blocked-tool.txt`，内容为 `denied-probe`。随后出现 `permission_denied`，`tool_name` 为 `Write`，`decision_reason_type` 为 `mode`。工具结果 `is_error` 为 true，权限决定为 `reject`。工具结果文本为：`Cannot write to <PATH>\blocked-tool.txt while in plan mode.` 回合最终 `subtype` 仍是 `success`，`result.is_error` 为 false。工具被拒绝时，整回合仍可能被 CLI 标成成功。

上面两次请求没有续接。随后在另一个仓库外临时目录、同一启动方式下，用同一个 session 连续跑了四回合。四次进程退出码都是 0，stderr 都为空，`permissionMode` 都与当次参数一致，session 不变，输出里没有计划目录，计划目录也没有新增文件。每次工具被拒绝时，工具结果 `is_error` 为 true，回合最终 `subtype` 仍是 `success`，`result.is_error` 为 false。

| 回合 | 模式 | 工具与文件 |
|---|---|---|
| 首次 | plan | 调用 Write 创建 `blocked-tool.txt`，工具层拒绝，文件不存在 |
| 续接 | bypassPermissions | 调用 Write 创建 `hello-claude.txt`，工具返回创建成功，内容为 `g3-execute` |
| 续接 | plan | 调用 Write 改写 `hello-claude.txt`，工具层拒绝，内容仍为 `g3-execute`。这一次没有再调用创建工具 |
| 再续接 | plan | 调用 Write 创建 `blocked-again.txt`，工具层拒绝，文件不存在，`hello-claude.txt` 仍为 `g3-execute` |

拒绝文本仍是 `Cannot write to <PATH>\<文件名> while in plan mode.`，`decision_reason_type` 为 `mode`。这覆盖该 Windows 版本和这条启动路径上的 plan → bypassPermissions → plan。没有在应用里点选 Discuss。M0 的另一条 plan 请求曾经把计划写进 Claude 自己的计划目录，因此讨论模式仍然不是文件系统隔离。macOS 2.1.322 仍然没有工具层证据。

四回合的 `result.usage` 与同条 `model_usage_since_last_result` 的 input、output、cache read、cache creation 一致。`costUSD` 等于相邻两次 `total_cost_usd` 的差，所以 `total_cost_usd` 是会话累计，`costUSD` 是本回合费用。助手消息里较小的 usage 不作为回合用量。

| 回合 | input | output | cache read | cache creation | 本回合 costUSD | total_cost_usd |
|---|---:|---:|---:|---:|---:|---:|
| 首次 plan | 4 | 709 | 41535 | 18565 | 0.0855115 | 0.0855115 |
| bypassPermissions | 4 | 173 | 61063 | 445 | 0.0096243 | 0.0951358 |
| 回到 plan | 4 | 324 | 63535 | 2126 | 0.0181055 | 0.1132413 |
| 再次 plan | 4 | 209 | 66222 | 453 | 0.0105322 | 0.1237735 |

### runner 与异常恢复

`cargo test --lib -- --test-threads=8` 在本机连续两次通过：112 项通过，2 项忽略。忽略项是隔离辅助进程，由对应测试实际拉起。`cargo build` 通过，仍有既有的 10 项 dead_code 警告。本次没有重跑 `npm test` 和 `npm run build`。

Windows 用例覆盖了关闭 stdout 后的 EOF 宽限、父进程先退出并留下后代、取消后进程树消失，以及 10000 行 stderr 后状态仍为成功。父进程退出后，只对根进程执行 `taskkill /T` 清不掉被过继的 `cmd`。实现改为在独立线程记录后代，并用进程创建时间核对后再清理；系统允许时同时使用 `KILL_ON_JOB_CLOSE`。全表快照若放在读循环里，会把排空拖过测试用的 150 毫秒宽限，把已经退出码为 0 的成功回合误判为管道未结束。

跨进程恢复测试通过：辅助进程写下 `running` 的 run 和运行所有权，并留下一个仍在运行的子进程；测试只杀掉辅助进程，然后重新构造应用状态。run 变为 `interrupted`，项目为 `waiting_user`，同目录新发送被拒绝，该子进程仍在。确认清理之后可以再发送，确认动作本身不杀掉该子进程。应用窗口中的退出、崩溃，以及“我已处理残留进程”的点击，都没有做。

### 仍未验收

2026-11-11 决定暂不补测下面三项。它们保持未验收，不把 M2 标为完成，开发继续进入 M3。

- macOS Finder 或 Dock 脱离开发终端后的 PATH。
- macOS Claude 2.1.322 的工具层拒写。
- 应用窗口内的异常退出，以及“我已处理残留进程，解除阻塞”的点击。macOS 上已经走过的对话、点名、停止和草稿不在这项里。后端跨进程恢复测试已经通过，它不代替这次窗口点击。

## M5 实测（Windows，2026-11-11）

环境：Windows 11 10.0.26300；grok 1.0.77（50ca579fa996）、dsh 0.2.2-rc.1，与 M0 样本版本一致。所有调用都在仓库外新建的非 git 临时目录中执行，不传 `--cwd`。grok 直接启动 `grok.exe`，使用 `--prompt-file`；dsh 将本机 `dsh.cmd` 已核对的一行启动器解析为绝对 `node.exe` 和入口脚本，通过参数数组直接启动，提示词经 stdin。本次没有经过 `cmd.exe`，核对的是应用的实际启动方式。

### G4 grok：网络阻塞，未完成

首回合命令为 `grok --prompt-file <提示词文件> --output-format streaming-messages-json --permission-mode plan`，工作目录由子进程 cwd 指定。要求实际调用写工具创建 `blocked-first.txt`。180 秒内 stdout 为 0 字节，没有收到 init、工具或完成事件；stderr 为 6,000 字节，包含 11 次 `error sending request for url`、5 次 `model catalog fetch timed out` 和 3 次 `Settings fetch failed`。超时后通过 PID 终止本次测试进程树并等待退出，没有触碰其他 Grok 会话。

| 回合 | 结果 | 会话、工具与文件事实 |
|---|---|---|
| 1，plan | 测试在 180 秒终止；不是正常 CLI 完成退出 | 未获得 session、cwd 或权限回显，无工具事件，`blocked-first.txt` 不存在 |
| 2，bypassPermissions 续接 | 未执行 | 无可续接的首回合，未创建 `created.txt` |
| 3，plan 续接 | 未执行 | 跨模式切回及工具层拒写未验证 |

不能以文件不存在认定只读生效：本次根本没有进入可观测的模型回合。需恢复 Grok 网络访问后重新执行 G4。

### G5 dsh：原 headless API Key 路径返回 402，后续账号核对见文末

首回合命令为 `dsh headless --json --permission-mode read-only --approval-policy never -`，使用上文解析后的真实入口启动。20.39 秒后退出码为 1。session 事件的 cwd 与临时项目目录一致，session ID 为 `session-99adadb5-f0f6-475c-9039-f00ed53d6496`；无权限回显。没有工具调用。最终 `reason.kind=error`、`error.code=QUOTA`、`error.status=402`，错误正文为 `Insufficient Balance`。未修改任何测试目标文件。

| 回合 | 结果 | 会话、工具与文件事实 |
|---|---|---|
| 1，read-only | QUOTA / 402 / Insufficient Balance | cwd 正确；无 write 调用；`blocked-first.txt` 不存在，不能据此验收只读拒写 |
| 2，workspace-write 续接 | 未执行 | 原 headless 路径返回 402，停止后续回合 |
| 3，read-only 续接 | 未执行 | 跨模式切回及工具层拒写未验证 |

唯一 step_end 没有 usage，final 的键为 type、sessionId、reason、text。这里不能归一为 0 token，也没有可用的本回合/累计对照。该次 API Key 路径失败不证明 Web 登录账号余额不足；后续账号路径复测见文末。

### 已确认与保留项

两个适配器的解析与用量规则以同版本的 M0 真实样本及测试为依据，本次失败没有产生与该证据矛盾的用量读数；没有据此编造新的归一语义。G4/G5 跨模式验收、成功回合用量追加对照未完成；窗口部分走查见下文，macOS 未验证。pi 未安装，未录制样本或编写适配器。

本次原始输出仅保存在仓库之外的临时目录，未入库；记录中的路径与请求标识已省略。网络及认证路径确认后按 M5 Task 7 重跑，不将这次失败记为通过。


### M5 窗口走查与复测（增加代理配置前）

2026-11-11 再次独立复测同版本 CLI：Grok 首回合仍在 180 秒无 stdout 后终止，stderr 6,000 字节，包含网络错误、模型列表超时及设置获取失败。Dsh 在 11.34 秒以退出码 1 返回 QUOTA / 402 / Insufficient Balance，cwd 正确、无工具调用、无有效 usage，后续回合停止。

实际打开 Tauri 应用，在仓库外临时目录通过向导创建 Claude 组长与 Grok、Dsh 成员。CLI 列表和版本、Pi 禁用、Dsh 模型禁用及 profile 说明均通过。点名 Grok 创建 a.txt 后无回复，点击停止，确认本次 Grok 与子进程退出、提示词临时文件删除、run 为 cancelled、指令为 waiting_user，a.txt 未创建。没有把未创建文件当作只读能力证据，也未把单个直接回合停止扩张为团队讨论停止通过。

错误模型点名的 run 为 failed，系统消息保留 CLI 原文：`Couldn't set model 'imates-m5-no-such-model-20261111': Invalid params: "unknown model id". Run 'grok models' to see available models.`，成员显示执行出错。模型已恢复空值并重新打开核对；恢复后成功对话待联网复验。两个直接回合的 usage_json 均为 null，没有成功团队回合，团队用量仍未验收。Dsh 窗口执行、完整团队协作、仅 Grok/Dsh 环境的助理均未验证；Pi 仍未安装。

完整逐项结果见 M5 Task 9。截图、原始输出与只读数据库摘要仅留本机临时目录，未提交。用户随后要求 Grok 使用本机代理，将以新增 Task 10 实现后继续验收。


### Grok 本机代理与继续验收（2026-11-11）

用户明确要求 Grok 使用本机代理。本机 Windows 已开启 HTTP/HTTPS 静态代理，应用启动环境没有代理变量。仅对测试进程补入本机代理后，`grok models` 1.22 秒返回退出码 0 和模型列表。新增应用配置默认跟随本机代理，Windows 经 WinHTTP 读取静态地址；可选手动代理/直连，配置只传给 Grok 子进程，不改系统或 CLI 配置。

真实 Tauri 窗口确认检测到本机代理，手动输入非法 FTP 地址得到明确报错且内容保留；有效手动地址、直连均保存并重新打开核对，最终恢复 system。配置持久化为 `{"mode":"system","url":""}`。应用内默认模型的 Grok 创建 a.txt，磁盘内容严格为 hi；再次点名问上一条任务，返回 a.txt / hi，两回合 session ID 相同，数据库两条 run 均为 ok，第二条命令含 -r，代理没有出现在命令参数中。错误模型恢复后的成功对话由此验证，Task 9 清单 4、8 通过。

| 应用直接回合 | inputTokens | outputTokens | cacheReadInputTokens | costUsd |
|---|---:|---:|---:|---:|
| 创建 a.txt | 20182 | 1097 | 65536 | 0.079714 |
| 同会话复述任务 | 20359 | 252 | 1664 | 0.043062 |

这些是直接回合，并非团队回合，不替代清单 10 的团队用量验收。

G4 的三回合复测在第二回合遇到失败，因此第三回合未执行：

| 回合 | 实测事实 | 结果 |
|---|---|---|
| plan 首回合 | 31.27 秒、exit=0、cwd 正确、回显 plan；实际 write 返回 is_error=true，工具原文 `Rejected: file edits are not allowed in plan mode`，blocked-first.txt 不存在 | 模式回显、工具拒写通过 |
| 同 session 的 bypassPermissions | 9.18 秒、exit=0、cwd 正确、回显 bypassPermissions；write 仍返回上述 plan 拒写，created.txt 不存在；正确复述上一回合文件名 | 跨模式失败；CLI 的 success 不能代替文件验收 |
| 切回 plan | 未执行 | 第二回合已失败，停止后续 |

G4 两回合 input_tokens 为 38776 / 1203，modelUsage.costUSD 为 0.092212 / 0.02762，下降证实不是会话累计。本次沿用 M0 的本回合用量归一结论。混合模式仍不能宣称通过：需另行修订/验证模式切换或分模式会话方案；本机代理功能没有改写权限策略。

Dsh 仍为 QUOTA / 402，未再调用；Pi 未安装。完整团队讨论/执行/验收、团队用量未通过。后端代理新增 6 个测试先失败后通过；全套串行测试 168 passed、2 ignored，前端 32 passed，两端构建通过。默认并行测试出现原有 runner 时序用例失败（部分回答尚未输出即触发 100ms 取消，或 180ms 空闲超时），串行原断言全部通过，没有删断言、放宽期望或跳过用例。原始输出、截图和数据库摘要仍仅保存在仓库外临时目录。

### 应用统一代理（2026-11-11）

用户将 Grok 专用代理扩展为应用统一配置。所有已接入 CLI 的成员直接回合、团队回合和内置助理通过同一运行工厂读取设置；窗口入口统一为「网络代理」。当前支持 Claude、Codex、Grok、Dsh，Gemini 尚未接入，不在此次新增适配器。

真实 Tauri 窗口完成系统代理显示、非法地址报错保留输入、手动与直连保存/重新打开，以及项目页修改后首页读到同一配置的检查；最终恢复 system。只读数据库查询：测试前仅旧 grok_proxy 键，测试后为 network_proxy，值均为 system / 空地址。旧设置自动迁移，冲突优先保留统一配置，非法值不静默删除，迁移失败回滚由自动测试覆盖。

代理测试先 5 failed / 5 passed，实现后 10 passed；全套后端串行 172 passed、2 ignored，前端 32 passed，两端构建通过。所有已接入 CLI 的两种运行模式、设置改变前后的适配器环境快照及 runner 真实子进程传递均有断言；原有默认并行时序限制未处理。本次没有新增真实模型 API 请求，不代表所有供应商联网或完整团队流程已验收；Grok 跨模式拒写、Dsh 认证路径与 Pi 安装仍阻塞；Dsh 后续账号核对见下节。

### Dsh Web 账号与 headless 认证核对（2026-11-11）

用户确认通过 `dsh web` 的账号信息查看余额。核对本机启动器、版本与组合配置：Web 和应用使用同一 Dsh 0.2.2-rc.1 入口，但 Web 默认 provider 为 `deepseek-account`，headless 为 `deepseek-official`（API Key）。`deepseek-official` 不读取账号登录态。因此前述 QUOTA / 402 是 API Key 路径返回的错误，不能据此判断 Web 登录账号余额不足，也不能要求用户充值来修复账号接入。

本机 Dsh 源码 `packages/bundle/headless/cordis.patch.yml` 默认禁用 credentials、authorization 和 deepseek-account；本机组合配置中 credentials 已启用，authorization 与 deepseek-account 仍禁用。只临时切 provider 时返回 `ACCOUNT_SIGN_IN_REQUIRED`；再启用 deepseek-account 时 stderr 明确提示 `pending (waiting for service: authorization)`，说明该错误也可能来自服务未启动，而非用户没有登录。仅核对已保存 grant 的结构、存在性与 issuer 是否匹配，没有输出 token、API Key 或账号身份。

最终在仓库外临时目录通过 `--profile headless --patch <临时账号补丁> --json ... -`，使用以下不含凭据的配置：

```yaml
- id: agent-default-model
  config:
    provider: deepseek-account
    model: deepseek-flash
- id: credentials
  disabled: false
- id: authorization
  disabled: false
- id: deepseek-account
  disabled: false
```

该补丁只用于测试进程，未写入用户全局 profile，也未改动 imates 的现有适配器命令。提示词经 stdin，实际 node/入口直接启动，cwd 为仓库外非 git 临时目录。三回合均 exit=0、final.reason.kind=completed、cwd 正确，沿用同一个 session：

| 回合 | 耗时 | 工具与文件事实 | 结果 |
|---|---:|---|---|
| read-only | 18.53 秒 | write 返回 FS_SANDBOX_DENIED；blocked-first.txt 不存在 | 工具层拒写通过 |
| workspace-write 续接 | 10.93 秒 | write/read 完成；created.txt 严格为 g-execute；正确复述 blocked-first.txt | 执行与续接通过 |
| read-only 续接 | 9.97 秒 | 两次 write 均为 FS_SANDBOX_DENIED；created.txt 保持 g-execute；blocked-again.txt 不存在 | 切回只读通过 |

逐步汇总 `step_end.usage`，各步原始字段均存在，费用字段仍未知：

| 回合 | inputTokens | outputTokens | cacheReadInputTokens | cacheCreationInputTokens | costUsd |
|---|---:|---:|---:|---:|---|
| 1 | 2734 | 404 | 13824 | 0 | 空 |
| 2 | 654 | 220 | 26368 | 0 | 空 |
| 3 | 514 | 290 | 18560 | 0 | 空 |

inputTokens 随续接回合下降，支持逐回合归集而非会话累计；未重复累加 final。原始输出仅留仓库外临时目录。用户原有 Web 进程仍运行。

结论：已登录账号可以完成模型调用和同会话跨模式切换；原先“需要充值”的判断已更正。现有 imates 仍调用原 headless 路径，未自动采用该补丁，应用内 Dsh 创建文件、团队流程及团队用量仍未验收。下一步需确定账号接入方式，不能把临时诊断成功写成应用修复完成。

### Dsh 认证选项与已配置模型接入（Task 12，2026-11-11）

用户已明确要求增加认证选项并使用现有自定义模型，替代此前“忽略 agents.model”的设计。应用新增全局 headless/account 认证与 Web/headless 目录来源；成员选择存 {profile, provider, model}，具体配置和凭据由 Dsh 自己读取。默认保留原 headless 行为；明确选择模型使用其供应商凭据。首页、项目页和成员表单均有入口。

原生桥接在一次 headless 子进程内通过本机 app-boot 只读 API 组合来源配置，先准备凭据/授权/账号服务再开启 runner；目录模式禁用 runner。当前仅验证 Windows Dsh 0.2.2-rc.1。未改全局 profile，也未启动或关闭用户 Web。

新桥接真实目录读取：Web 约 5.47 秒、headless 约 4.68 秒，均 exit=0，仅产生 imates_dsh_catalog 事件。Web 返回 liberation、claude、kimi、gpt、deepseek-official、deepseek-account；headless 无 kimi/gpt，模型亦有不同。样本位于 src-tauri/tests/fixtures/dsh/models.json，只保留目录 ID/显示名，无端点或凭据。账号和自定义模型生成、窗口验收将在 Task 13 单独记录，此时仍未验收。

自动检查：后端串行 180 passed、2 ignored，前端 33 passed，Node 桥接 4 passed，cargo build/npm run build 均通过。

### Dsh 新桥接与应用验收（Task 13，2026-11-11）

Windows Dsh 0.2.2-rc.1；真实调用均在仓库外临时目录，应用使用统一 system 代理，独立 CLI 测试显式沿用本机代理。来源配置通过 Dsh 自带 app-boot 只读 API 组合，只在测试子进程内启用服务，没有改写用户 profile 或输出凭据。

**账号三回合（新桥接）：** 同一 session，全部 exit=0 / completed。

| 回合 | 耗时 | 工具及文件事实 | inputTokens | outputTokens | cacheReadTokens | cacheWriteTokens |
|---|---:|---|---:|---:|---:|---:|
| read-only | 10.47 秒 | write 返回 FS_SANDBOX_DENIED，glob 确认 blocked-first.txt 不存在 | 2936 | 488 | 22144 | 0 |
| workspace-write | 9.18 秒 | write/read 成功，created.txt 严格为 g-execute；正确复述首回合文件名 | 687 | 309 | 26496 | 0 |
| read-only | 9.66 秒 | 两次 write 均拒绝，created.txt 保持 g-execute，blocked-again.txt 不存在 | 661 | 482 | 28672 | 0 |

**自定义模型：** 显式选择 Web / gpt / gpt-6.1-sol（沿用 Dsh 中已有配置），18.32 秒创建 custom.txt，文件严格为 custom-model-ok；8.86 秒同 session 续接，无工具调用，正确复述文件名和内容。逐步汇总 input/output 分别为 11427/68、296/15；第一回合缓存字段并非每步都存在，汇总保持未知；第二回合 cacheRead=11008。费用仍未知。桥接没有把任一未知字段填成零。

另一次显式选择 Web / gpt / gpt-6 在 8.70 秒后 exit=1，原生 final.reason.error 为 `PI_AI_ERROR / Upstream request failed`；无正文、工具或文件，未回退其他模型。错误发生于已开始的模型回合，不能据此称其他自定义模型不可用；gpt-6.1-sol 的成功调用单独记录。没有进行反复重试或修改用户网关配置。

**真实 Tauri 窗口：** 通过 WebView2 调试接口发送鼠标/键盘及选择操作，未模拟 Tauri IPC。完成首页 headless 默认值检查；账号+headless 来源保存、重新打开；改为 Web 后项目页读取相同设置；成员下拉显示实际已配置模型；选择 Web/gpt/gpt-6.1-sol 后切 headless 来源，选择仍保留且标为“当前列表暂不可用”；恢复 Web、保存、重新编辑仍保留该模型。成员表单内的 Dsh 设置入口同样可用。截图已检查。

| 应用直接回合 | 文件/回复 | 运行结果 | inputTokens | outputTokens | cacheReadInputTokens | cacheCreationInputTokens | costUsd |
|---|---|---|---:|---:|---|---|---|
| 默认账号、model=null | b-account.txt 严格为 account-ui-ok | ok | 4047 | 970 | 33920 | 0 | 空 |
| Web/gpt/gpt-6.1-sol | c-custom.txt 严格为 custom-ui-ok | ok | 13732 | 107 | 空（部分步缺字段） | 空 | 空 |
| 自定义模型续接 | 无工具，正确复述 c-custom.txt/custom-ui-ok | ok，同 session | 1043 | 11 | 10952 | 空 | 空 |

数据库核对 agents.model 精确保存 {profile:web,provider:gpt,model:gpt-6.1-sol}；修改模型后账号会话失效，新建自定义模型 session，后续自定义回合保持该 session。三个成功直接回合 usage_json 均存在，仅存有依据的字段。未把这些直接回合计为团队用量验收。

**停止与清理：** 自定义模型再次启动后实际点击停止；本次运行 cancelled，指令进入 waiting_user；本次 Dsh 进程退出，应用创建的 mjs/json 辅助文件删除，已有文件不变。随后放弃这条测试指令，项目恢复 idle。成功结束和上游失败的临时桥接文件也已清理。测试窗口正常关闭；原 Dsh Web 仍监听本机 3080，未关闭用户进程。最终应用认证为 account，目录来源为 web，网络代理为 system；验收成员保留已选择的自定义模型。

Task 9 清单 1–5、8 已通过；其中旧第 2 项的“禁用模型框”已由用户新规格替代并重验。第 7 项 Grok/Dsh 直接回合停止通过，团队停止未验收。第 6/10 项团队完整流程/团队用量、第 9 可选项仍未验收；Grok 同 session 跨模式拒写、Pi 未安装保持原状，M5 不标完成。M2/M3 的未验收与 M3 整改状态不变。原始输出、截图和数据库摘要只留仓库外临时目录。

## M5 pi 采样（macOS arm64，2026-11-12）

本机 `pi 1.5.1` 已安装并能使用现有配置完成请求。四种样本都在仓库外的独立临时目录采集，启动时使用 `--mode json`，提示词经 stdin，`--session-dir` 指向该临时目录。为隔离这组样本，未加载扩展、MCP 和技能。

| 场景 | 实际结果 |
|---|---|
| 讨论 | 仅开放 `read`，真实调用一次并回复文本标记，退出 0 |
| 执行 | 开放 `read,write`，真实创建 `hello.txt` 并确认正文，退出 0 |
| 续接 | `--session <id>`，同 ID、不调用工具，正确回答 `hello.txt`，退出 0 |
| 启动错误 | 不存在的 ID 导致退出 1，无 stdout JSON，stderr 为 `No session found matching '<id>'` |

流式正文在 `message_update.assistantMessageEvent` 的 `text_delta` 中。工具以 `tool_execution_start/end` 配对；同一用户回合可有多个 `turn_end`，按其 assistant 消息归集用量，不再重复读取 `message_end` 和 `agent_end.messages`。例如讨论样本第二子轮 `input=148`、`cacheRead=896`、`output=11`、`totalTokens=1055`；缓存和非缓存输入分开，`totalFromProvider=false` 的零费用不当成真实零。最后出现 `agent_end` 和 `agent_settled`。本轮样本没有验证跨模式、Windows 或操作系统级文件隔离。

脱敏事件见 `src-tauri/tests/fixtures/pi/`，接线方案与验收见 M5 Task 8。
