# imates

像管理团队一样管理多个 AI Agent 的桌面应用。每个项目有一组分工不同的 Agent，围绕你下发的指令讨论、达成一致并落实工作。

当前进度：M0 CLI 实测已完成（pi 顺延 M5），M1 骨架已完成。M2 实现已完成，验收未闭环。2026-11-11 决定暂不补测三项：macOS Claude 2.1.322 工具层拒写、Finder/Dock PATH、应用窗口里的异常退出与清理确认。M3 团队协作已实现，自动测试已通过，真实 CLI 团队讨论未执行。详见 [M2 实施计划与验收记录](docs/plans/2026-11-10-M2-单Agent对话.md) 和 [M3 团队协作计划](docs/plans/2026-11-11-M3-团队协作.md)。

M5 的 grok、dsh 适配器与应用统一代理已接入：首页/项目页的「网络代理」供所有已接入 CLI 的成员、团队回合和内置助理共用，可跟随 Windows 静态代理、手填或直连，原 Grok 设置自动迁移。窗口清单 1–4、8 通过，7 部分通过；Grok 已实际创建文件并续接，成功直接回合保存 token/费用。后端串行 180 passed、2 ignored，前端 33 passed，Node 桥接 4 passed；此前默认并行的原有 runner 时序问题仍保留。M5 未完成验收：Grok 跨模式仍拒写，Dsh 认证与自定义模型选择已实现但新路径实测待完成，pi 未安装，完整团队协作/团队用量未验证。M3 整改仍待定。见 [M5 计划与验收记录](docs/plans/2026-11-11-M5-其余适配器.md)。

Dsh 使用：点击首页或项目页「Dsh 设置」，选择沿用 headless 或已登录的 DeepSeek 账号，并选择 Web/headless 模型来源。创建或编辑 Dsh 成员时，下拉可直接选用该来源已配置的模型；模型留空使用全局默认认证。明确选择模型后使用该供应商在 Dsh 中配置的凭据。应用不保存登录令牌或 API Key。

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
