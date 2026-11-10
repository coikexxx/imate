# CLI 实测记录

> M0 产出。记录各 CLI 无头模式的实际行为，是适配器实现的依据。
> 实测日期：2026-11-10 · 平台：Windows 11 · pi 未安装，顺延到 M5。

## 证据范围与待验证项

本文件原有命令和行为是 Windows 11 的 M0 历史记录；保留原命令中的 `-C/--cwd` 等参数，不按设计改写实测证据。M2 macOS 实测另列于后，不能外推 Windows。M2 应用通过子进程 current_dir 设置目录，省略目录参数后的结论仅对已记录的平台和版本成立。

| 平台/CLI | 已有证据 | 待验证项 | 状态 |
|---|---|---|---|
| Windows / claude 2.1.319 | init 回显模式、同模式续接、plan 拒绝项目写入、执行可写上级目录 | 跨模式续接、result.usage/费用的本回合语义、新 runner 的进程树处理 | 未验证 |
| Windows / codex 0.167.0 | 新线程 read-only/workspace-write 行为、续接记忆；thread.started 不回显权限 | G1 resume 权限覆盖及 Discuss → Execute → Discuss；G2 累计/本回合语义；非 git cwd；新 runner 清理 | 未验证 |
| Windows / grok 1.0.77、dsh 0.2.2-rc.1 | 下文已有场景；grok 回显模式，dsh 不回显 | M5 跨模式续接、用量归一、应用 current_dir 与进程行为 | 未验证 |
| macOS / Claude Code 2.1.322、Codex 0.167.0 | G1/G2 CLI 基线；G3 Claude 模式回显和续接；M2 应用对话、点名、停止与草稿保存见后文 | Claude 工具层拒写；Finder/Dock PATH；异常退出后的实际恢复；Windows runner 独立验证 | 部分通过 |
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
- 代码测试覆盖启动恢复、interrupted 状态、运行所有权及同目录清理确认阻塞；app 异常退出后的实际重启和人工清理确认仍未手动验收。Windows runner 平台行为未验证。
- Claude `system/init.permissionMode` 回显与续接已核对，但 Discuss 请求没有触发写工具，所以本机没有工具层拒写证据；该能力状态为“模式已核对、工具层拒写未验证”，不宣称硬隔离。

以上仅适用于所记 macOS/CLI 版本和启动路径。Claude 工具层拒写、Finder/Dock PATH、异常恢复手验及 Windows runner 验收未完成，因此 M2 尚未达到手册中的全部验收标准。
