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

- 版本：
- 可执行文件：
- 提示词传入方式：
- 工作目录指定方式：

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
- stdout 是否每行都是合法 JSON：
- 其他注意事项：

## dsh

- 版本：
- 可执行文件：
- 提示词传入方式：
- 工作目录指定方式：

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
- stdout 是否每行都是合法 JSON：
- 其他注意事项：
