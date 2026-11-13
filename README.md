# imates

像管理团队一样管理多个 AI Agent 的桌面应用。每个项目有一组分工不同的 Agent，围绕你下发的指令讨论、达成一致并落实工作。

当前进度（2026-11-13）：M0、M1 已完成，M2–M5 的核心路径已补 Windows 原生验收。按用户授权，Claude 官方账号验证阻断后改用 Codex 组长，七项核心冒烟、真实文件交付、停止与异常恢复通过；macOS 未覆盖项保持原状态。详见 [核心流程验收单](docs/核心流程验收单.md) 和 [本轮 Windows 验收记录](docs/验收记录-2026-11-13-Windows.md)。

M5 的 Grok、Dsh 与统一代理已接入。Windows 实测通过 Grok 跨模式重建会话后写文件、两者直接执行与同会话续接、Dsh 完整团队与团队用量，以及并行讨论停止后的进程清理。Dsh 团队终端首次被 Windows ACL 拦截，单次授权后成功，失败记录保留；gpt-6 历史上游错误不改写。pi 按 2026-11-13 决议保持未接入，可选的仅 Grok/Dsh 安装环境助理路径未覆盖。Codex 的自动单次提权已核对实际 sandbox、文件与 Git 提交，后续恢复普通权限。见 [M5 计划](docs/plans/2026-11-11-M5-其余适配器.md) 和 [提权记录](docs/plans/2026-11-13-受阻与一次性提权.md)。

Dsh 使用：进入「系统设置 / 应用管理 / DeepSeek Harness」配置认证与模型来源；也可从 Dsh 成员表单进入设置。成员模型可选择已配置的 Web/headless provider、model 和所声明的推理强度。明确选择后沿用该供应商在 Dsh 中的凭据；留空沿用全局默认认证。应用不保存登录令牌或 API Key。

## 开发

```bash
npm install
npm run tauri dev
```

测试：`npm test`（前端）、`cd src-tauri && cargo test`（后端）。

## 文档

- [MVP 开发手册](docs/MVP开发手册.md)：产品定义、架构、协作流程、数据模型与里程碑
- [CLI 实测记录](docs/CLI实测记录.md)：各 CLI 无头模式的调用参数、事件格式与行为
- 实施计划：[M0 CLI 实测](docs/plans/2026-11-10-M0-CLI实测.md)、[M1 骨架](docs/plans/2026-11-10-M1-骨架.md)、[M2 单 Agent 对话](docs/plans/2026-11-10-M2-单Agent对话.md)、[M3 团队协作](docs/plans/2026-11-11-M3-团队协作.md)
- [多设备开发指南](docs/多设备开发指南.md)：Windows / Intel Mac / Apple Silicon Mac 之间切换开发的环境清单与协作约定

## Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
