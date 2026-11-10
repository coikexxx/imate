# imates

像管理团队一样管理多个 AI Agent 的桌面应用。每个项目有一组分工不同的 Agent，围绕你下发的指令讨论、达成一致并落实工作。

当前进度：M1 骨架（项目、成员、人设、规章的创建与管理）。Agent 的实际运行在 M2 接入。

## 开发

```bash
npm install
npm run tauri dev
```

测试：`npm test`（前端）、`cd src-tauri && cargo test`（后端）。

## 文档

- [MVP 开发手册](docs/MVP开发手册.md)：产品定义、架构、协作流程、数据模型与里程碑
- 实施计划：[M0 CLI 实测](docs/plans/2026-11-10-M0-CLI实测.md)、[M1 骨架](docs/plans/2026-11-10-M1-骨架.md)
- [多设备开发指南](docs/多设备开发指南.md)：Windows / Intel Mac / Apple Silicon Mac 之间切换开发的环境清单与协作约定

## Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
