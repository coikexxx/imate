# CLI 实测记录

> M0 产出。记录各 CLI 无头模式的实际行为，是适配器实现的依据。
> 实测日期：2026-11-10 · 平台：Windows 11 · pi 未安装，顺延到 M5。

## claude

- 版本：2.1.319（Claude Code）
- 可执行文件：npm 全局命令 `claude`（`%APPDATA%\npm\claude`）
- 提示词传入方式：`-p` 时经 stdin（本次未跑通模型回合）
- 工作目录指定方式：未测。进程的当前目录即工作目录（命令未使用单独的目录参数）

### 命令

| 场景 | 完整命令 |
|---|---|
| 讨论（只读） | |
| 执行（自动） | |
| 续接 | |

### 事件格式

| 关注点 | 事件类型与字段路径 | 样本中的一行示例 |
|---|---|---|
| 会话 ID | | |
| 文本增量 | | |
| 工具调用 | | |
| 回合结束与最终文本 | | |
| 用量 | | |

### 行为

- 只读模式是否拦住写文件：
- 执行模式是否全程无确认：
- 执行模式能否写到工作目录之外：
- 续接是否生效：
- 出错时的表现（退出码、stdout、stderr）：
- stdout 是否每行都是合法 JSON：认证失败的那次输出是合法 JSON 行，但不是一次成功的讨论回合
- 其他注意事项：`--permission-mode` 的可选值为 `acceptEdits`、`auto`、`bypassPermissions`、`manual`、`dontAsk`、`plan`。2026-11-10 跑 discuss 时退出码为 1，stderr 为空。stdout 有 `system/init`、`assistant`、`result/success`。最后一条 `type: result` 的 `is_error` 为 true，`terminal_reason` 为 `api_error`，`result` 文本是登录已到时限、无法续期，需要 `claude auth login`。没有调用模型，execute、resume、blocked、escape、error 都未跑，也没有提交样本。

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

帮助里确认的写法如下。除缺密钥的那次 discuss 之外，这些命令没有在有模型的情况下跑通。

| 场景 | 完整命令 |
|---|---|
| 讨论（只读） | `dsh headless --json --permission-mode read-only --approval-policy never -`，提示词经 stdin |
| 执行（自动） | `dsh headless --json --permission-mode workspace-write --approval-policy never -`，提示词经 stdin |
| 续接 | `dsh headless --json --session-id <id> --permission-mode read-only --approval-policy never -`，提示词经 stdin |

### 事件格式

只看到缺密钥时的事件，没有工具调用、文本增量和用量。因此没有提交样本。

| 关注点 | 事件类型与字段路径 | 样本中的一行示例 |
|---|---|---|
| 会话 ID | 第一行 `type: session` 的 `sessionId`，形如 `session-…`。结束时 `final.sessionId` 再出现一次 | `{"type":"session","sessionId":"session-1ca4fa2a-4a42-4cb9-9693-ba0976c24992","cwd":"<HOME>\\AppData\\Local\\Temp\\imates-m0"}` |
| 文本增量 | 未观察到 | |
| 工具调用 | 未观察到 | |
| 回合结束与最终文本 | `final`。失败时 `reason.kind` 为 `error`，`text` 为空字符串 | `{"type":"final","sessionId":"session-1ca4fa2a-4a42-4cb9-9693-ba0976c24992","reason":{"kind":"error","error":{"code":"MISSING_CREDENTIAL"}},"text":""}` |
| 用量 | 未观察到 | |

中间还有 `status`，`phase` 依次为 `turn_start`、`step_start`、`step_end`、`turn_end`。

### 行为

- 只读模式是否拦住写文件：未测。配置里有预设 `read-only`（sandbox `read-only`，默认 approval `ask`）
- 执行模式是否全程无确认：未测。预设 `workspace-write` 的默认 approval 是 `ask`。命令行可以用 `--approval-policy never` 盖掉。另有 `danger-full-access`，approval 为 `never`，本次没有用
- 执行模式能否写到工作目录之外：未测
- 续接是否生效：未测。帮助里的参数是 `--session-id`，不是 `--resume`。未知 id 会报错。`--permission-mode` 在续接时仍然可以带
- 出错时的表现（退出码、stdout、stderr）：本次 discuss 退出码 1。stderr 是 `MISSING_CREDENTIAL`：provider `deepseek-official` 没有 API key。stdout 仍是 JSONL，`turn_end.reason.error.code` 和 `final.reason.error.code` 都是 `MISSING_CREDENTIAL`。没有跑到未知会话 ID 那个错误场景
- stdout 是否每行都是合法 JSON：这次失败输出的 6 行都是合法 JSON。stderr 是纯文本，没有并进 stdout
- 其他注意事项：headless 没有 `--model`。模型写在 profile 配置 `agent-default-model` 里，这次是 provider `deepseek-official`、model `deepseek-flash`。手册里的 `agents.model` 对 dsh 没有对应的命令行参数，适配器传了也不会生效。Windows 上 `bash` 工具关闭，启用的是 `pwsh`。本机环境变量和用户级环境变量里都没有 `DEEPSEEK_API_KEY`，所以 execute、resume、blocked、escape 都没跑。

## 对适配器设计的影响

1. codex 可以用参数 `-` 从 stdin 读提示词。dsh 的帮助也写明 `-` 读 stdin，进程能启动。grok 本次用的是 `--prompt-file`，文件方式可用，stdin 没测。claude 的 `-p` 按帮助从 stdin 读，但登录失败，没有跑完。`CommandSpec` 已有的 `stdin` 和 `prompt_file` 够用：grok 走 `prompt_file`，其余走 `stdin`。
2. codex 没有文本增量，群聊只能在每条 `agent_message` 完成时整段显示。grok 选定的 `streaming-messages-json` 也是整段；它的 `streaming-json` 才有 `type:text` 的增量，会话 ID 要等最后的 `end.sessionId`。claude 和 dsh 没有跑到模型输出，不能判断。
3. codex 的 `thread_id` 和 grok 的 `session_id` 续接后都不变，适配器不用每回合回写。claude 未测。dsh 的 `sessionId` 在失败那次的开头和 `final` 里相同，续接本身未测。
4. codex 的 `-s read-only` 拦住了工作目录里的写文件。grok 的 `plan` 拦住了 `blocked.txt`，但把计划写进了自己的会话目录，讨论模式不可靠，提示词里仍要写明本回合不要改文件。claude、dsh 的只读行为未测。
5. codex 的 `workspace-write` 和 grok 的 `bypassPermissions` 都能把文件写到工作目录之外。创建 Agent 的表单要保留这条安全提示。claude、dsh 未测。

没有发现 `CliAdapter` 必须加方法。dsh 的模型不在命令行上，适配器应忽略 `agents.model`。dsh 的续接参数是 `--session-id`，不是手册里原先写的 `--resume`。
