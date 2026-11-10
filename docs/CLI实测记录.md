# CLI 实测记录

> M0 产出。记录各 CLI 无头模式的实际行为，是适配器实现的依据。
> 实测日期：2026-11-10 · 平台：Windows 11 · pi 未安装，顺延到 M5。

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

2026-11-10 在已设置 `DEEPSEEK_API_KEY` 的 PowerShell 里，用 Git Bash 跑了下面三条命令，以及 blocked、escape。进程能启动，密钥也被接受。五个模型回合都在调用工具前因余额不足结束。讨论、执行、续接的 stdout 含用户主目录，且没有模型正文，不作为成功样本提交。已提交的只有未知会话的 `error.jsonl`。

| 场景 | 完整命令 |
|---|---|
| 讨论（只读） | `dsh headless --json --permission-mode read-only --approval-policy never -`，提示词经 stdin |
| 执行（自动） | `dsh headless --json --permission-mode workspace-write --approval-policy never -`，提示词经 stdin |
| 续接 | `dsh headless --json --session-id <id> --permission-mode read-only --approval-policy never -`，提示词经 stdin |

### 事件格式

每次模型回合的 stdout 都是 6 行合法 JSON，结构相同。没有文本、工具调用和用量。下面的示例来自这次 discuss，路径已换成 `<HOME>`。这份内容没有入库。

| 关注点 | 事件类型与字段路径 | 样本中的一行示例 |
|---|---|---|
| 会话 ID | 第一行 `type: session` 的 `sessionId`，形如 `session-…`。`final.sessionId` 再出现一次，值相同。`cwd` 是启动时的工作目录 | `{"type":"session","sessionId":"session-f8659c30-dacd-45d4-b816-f0fc1a517326","cwd":"<HOME>\\AppData\\Local\\Temp\\imates-m0"}` |
| 文本增量 | 未观察到。`final.text` 是空字符串，输出里没有「测试目录」 | |
| 工具调用 | 未观察到。`step` 停在 1，没有工具事件 | |
| 回合结束与最终文本 | `status.phase` 为 `turn_end`，以及 `type: final`。失败时 `reason.kind` 为 `error`，`reason.error.code` 为 `QUOTA`，`status` 为 402，`text` 为空 | `{"type":"final","sessionId":"session-f8659c30-dacd-45d4-b816-f0fc1a517326","reason":{"kind":"error","error":{"message":"Insufficient Balance (request_id: …)","code":"QUOTA","status":402}},"text":""}` |
| 用量 | 未观察到 | |

中间 `status.phase` 依次为 `turn_start`、`step_start`、`step_end`、`turn_end`。

未知会话走另一条路径，见下面「出错时的表现」。

### 行为

- 只读模式是否拦住写文件：没有得到结论。`read-only` 那次在工具调用前就以 QUOTA 结束。`blocked.txt` 不存在，是因为模型没有执行，不能当成沙箱拦住了写入。配置里的预设 `read-only` 仍是 sandbox `read-only`，默认 approval `ask`。
- 执行模式是否全程无确认：没有得到结论。`workspace-write` 加 `--approval-policy never` 已经发出，模型回合同样是 QUOTA。脚本开始前删过 `hello.txt`，结束后文件仍不存在。预设 `workspace-write` 的默认 approval 是 `ask`，命令行的 `--approval-policy never` 用来盖掉它。另有 `danger-full-access`，approval 为 `never`，本次没有用。
- 执行模式能否写到工作目录之外：没有得到结论。escape 场景同样是 QUOTA，上一级的 `imates-m0-escape.txt` 不存在。
- 续接是否生效：会话接上了，记忆没有验证。resume 使用 execute 的 `sessionId` `session-6cd2dd5b-8be9-4b62-8a28-3b855f6af6ce`，输出里的 `sessionId` 相同，`turn` 为 2。`final.text` 仍为空，输出里没有 `hello`。此前 execute 没有创建 `hello.txt`。帮助里的参数是 `--session-id`。未知 id 会报错。续接命令上仍然可以带 `--permission-mode`。
- 出错时的表现（退出码、stdout、stderr）：五个模型场景的 stderr 第一行都是 `[llm-deepseek-api-key] model listing from https://api.deepseek.com/models loaded 2 model(s), skipped 0 row(s)`，第二行是 `dsh: QUOTA: Insufficient Balance`。stdout 的 `turn_end.reason.error` 与 `final.reason.error` 都是 `code` 为 `QUOTA`、`status` 为 402、消息为 `Insufficient Balance (request_id: …)`。进程退出码打在运行脚本的终端里，没有写入文件，这里不补一个数字。未知会话 `--session-id session-does-not-exist` 的合并输出有两行：第一行是 JSON `{"type":"error","message":"session \"session-does-not-exist\" does not exist; omit --session-id to start a new Session"}`，第二行是同样的文字，前缀为 `dsh: `。这份 `error.jsonl` 没有用户主目录，已提交。它的进程退出码同样没有落盘。更早一次没有密钥时，discuss 的退出码是 1，错误码是 `MISSING_CREDENTIAL`；那份输出已删除，没有提交。
- stdout 是否每行都是合法 JSON：模型回合的 stdout 6 行都是合法 JSON，stderr 没有并进去。`error.jsonl` 用 `2>&1` 合并，第 2 行不是 JSON。
- 其他注意事项：headless 没有 `--model`。模型写在 profile 配置 `agent-default-model` 里，这次是 provider `deepseek-official`、model `deepseek-flash`。手册里的 `agents.model` 对 dsh 没有对应的命令行参数，适配器传了也不会生效。Windows 上 `bash` 工具关闭，启用的是 `pwsh`。这次密钥已被接受。DeepSeek 账户余额不足，所以中文正文、工具事件、用量、只读拦截和越界写都还要等充值后重跑。临时目录 `$TEMP/imates-m0` 先留着。

## 对适配器设计的影响

1. claude 的 `-p`、codex 的 `-`、dsh 的 `-` 都从 stdin 读提示词。dsh 这次会建会话并读 stdin；密钥有效，模型回合在出文本前以 QUOTA 402 结束。grok 本次用的是 `--prompt-file`，文件方式可用，stdin 没测。`CommandSpec` 已有的 `stdin` 和 `prompt_file` 够用：grok 走 `prompt_file`，其余走 `stdin`。
2. claude 这次的 `stream-json` 没有逐段文本，整段在 `assistant` 消息里。帮助里的 `--include-partial-messages` 没开，不能据此否定增量能力。codex 没有文本增量。grok 选定的 `streaming-messages-json` 也是整段；它的 `streaming-json` 才有 `type:text` 的增量，会话 ID 要等最后的 `end.sessionId`。dsh 这次没有模型正文，不能判断有没有文本增量。
3. claude 的 `session_id`、codex 的 `thread_id`、grok 的 `session_id` 续接后都不变，适配器不用每回合回写。dsh 续接后 `sessionId` 也不变，`turn` 从 1 增到 2，说明 `--session-id` 接上了同一会话。模型回合失败，记忆是否保留还没有验证。
4. codex 的 `-s read-only` 拦住了工作目录里的写文件。claude 和 grok 的 `plan` 都拦住了 `blocked.txt`，但分别把计划写进了 `.claude/plans/` 和 grok 自己的会话目录。这两个 CLI 的讨论模式不可靠，提示词里仍要写明本回合不要改文件。dsh 的 `read-only` 这次没有机会碰到文件。
5. claude 的 `bypassPermissions`、codex 的 `workspace-write`、grok 的 `bypassPermissions` 都能把文件写到工作目录之外。创建 Agent 的表单要保留这条安全提示。dsh 的 `workspace-write` 这次没有机会写文件。

没有发现 `CliAdapter` 必须加方法。dsh 的模型不在命令行上，适配器应忽略 `agents.model`。dsh 的续接参数是 `--session-id`。适配器要把 `final.reason.kind` 为 `error`、且 `error.code` 为 `QUOTA` 或 `MISSING_CREDENTIAL` 当成回合失败，空的 `final.text` 不是成功回复。未知会话是另一条 `type: error`，stderr 会再打一行 `dsh: ` 前缀。
