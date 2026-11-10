# imates

像管理团队一样管理多个 AI Agent 的桌面应用。每个项目有一组分工不同的 Agent，围绕你下发的指令讨论、达成一致并落实工作。

当前进度：M0 CLI 实测已完成（pi 顺延 M5），M1 骨架已完成。M2 单 Agent 对话的实现已完成，macOS 实际验收部分通过；Claude Discuss 工具层拒写、异常恢复与 Finder/Dock PATH 尚未验收，因此 M2 尚未完成验收。详见 [M2 实施计划与验收记录](docs/plans/2026-11-10-M2-单Agent对话.md)。

## 开发

```bash
npm install
npm run tauri dev
```

测试：`npm test`（前端）、`cd src-tauri && cargo test`（后端）。

## 文档

- [MVP 开发手册](docs/MVP开发手册.md)：产品定义、架构、协作流程、数据模型与里程碑
- [CLI 实测记录](docs/CLI实测记录.md)：各 CLI 无头模式的调用参数、事件格式与行为
- 实施计划：[M0 CLI 实测](docs/plans/2026-11-10-M0-CLI实测.md)、[M1 骨架](docs/plans/2026-11-10-M1-骨架.md)、[M2 单 Agent 对话](docs/plans/2026-11-10-M2-单Agent对话.md)
- [多设备开发指南](docs/多设备开发指南.md)：Windows / Intel Mac / Apple Silicon Mac 之间切换开发的环境清单与协作约定

## Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
