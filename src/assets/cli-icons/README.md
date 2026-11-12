# CLI 图标

应用管理里各 CLI 的 logo，由 `src/lib/cliIcons.ts` 引用，`src/components/CliIcon.tsx` 显示。

## 来源与许可

- 来源包：`@lobehub/icons-static-svg` 1.102.0
- 许可：MIT
- 文件原样复制，未做任何修改。

## 文件与原图标名的对应

| 文件 | 原图标名 |
|---|---|
| `claude.svg` | `claudecode-color` |
| `codex.svg` | `codex-color` |
| `grok.svg` | `grok` |
| `dsh.svg` | `deepseek-color` |
| `gemini.svg` | `geminicli-color` |
| `opencode.svg` | `opencode` |
| `openclaw.svg` | `openclaw-color` |
| `hermes.svg` | `hermesagent` |
| `cursor.svg` | `cursor` |
| `qwen.svg` | `qwen-color` |
| `copilot.svg` | `githubcopilot` |
| `kimi.svg` | `kimi-color` |

## 说明

- Pi 在该图标集里没有对应 logo，界面用字母徽标「π」，不拿别的图标凑数。
- `copilot`、`cursor`、`grok`、`hermes`、`opencode` 是单色图标（SVG 里只有 `currentColor`），用 `<img>` 显示是黑色，所以在深色界面里由 `cliIcons.ts` 标记为 `mono`，显示时加 `invert` 转成白色。
- `kimi` 的图标同时含固定蓝色 `#1783FF` 与 `currentColor`，整体反相会改变蓝色，因此不标 `mono`。
- 品牌商标归各自公司所有，此处仅作识别用途。
