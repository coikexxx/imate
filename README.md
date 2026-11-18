# imates

像管理团队一样管理多个 AI Agent 的桌面应用。每个项目有一组分工不同的 Agent，围绕你下发的指令讨论、达成一致并落实工作。

当前进度（2026-11-18）：M0、M1 已完成；M2–M5 的已接入 CLI 核心路径保留 2026-11-13 Windows 原生验收结论。macOS 本轮已补验 Finder 启动、设置、真实团队交付与用量、超时和异常恢复；`00a4053` 的独立原生停止复验已通过，三次历史失败保留；同版本异常退出的清理阻塞、目标自然结束后人工确认及新点名也通过，最终 arm64 生产 DMG 已构建并通过镜像、payload 和严格 ad-hoc 签名核查，不把历史 Windows 通过外推到当前 Mac。详见 [核心流程验收单](docs/核心流程验收单.md) 和 [本轮 macOS 验收记录](docs/验收记录-2026-11-17-macOS.md)。

M5 的 Grok、Dsh 与统一代理已接入。Windows 实测通过 Grok 跨模式重建会话后写文件、两者直接执行与同会话续接、Dsh 完整团队与团队用量，以及并行讨论停止后的进程清理。Dsh 团队终端首次被 Windows ACL 拦截，单次授权后成功，失败记录保留；gpt-6 历史上游错误不改写。pi 按 2026-11-13 决议保持未接入，可选的仅 Grok/Dsh 安装环境助理路径未覆盖。Codex 的自动单次提权已核对实际 sandbox、文件与 Git 提交，后续恢复普通权限。见 [M5 计划](docs/plans/2026-11-11-M5-其余适配器.md) 和 [提权记录](docs/plans/2026-11-13-受阻与一次性提权.md)。

M6 打磨打包（2026-11-17）：系统设置新增「运行设置」（内置助理所用 CLI 与讨论轮数、返工、回合总数、空闲／工具／总时长护栏，团队指令开始时冻结、点名与起草每次启动读取）；群聊按停止、超时、CLI 不可用、权限受阻、回合失败、等待清理分别提示下一步；项目页新增「用量」标签，按成员与项目合计 CLI 回报的用量并标注覆盖范围，不设费用上限、不估算未报告的费用。历史 Windows x64 未签名 NSIS 来源为 `04ff2af`；macOS arm64 最终生产 DMG 来自 `00a4053`，已通过镜像、只读挂载、源 app／包内 payload 一致性、严格 ad-hoc 签名和卸载核查，SHA-256 见安装记录。ad-hoc 没有 Developer ID 或 Apple 公证，Gatekeeper 检查仍为 rejected。两平台干净机安装未执行，不把任何平台安装门记为完成。来源、哈希和边界见 [安装与发布验收](docs/安装与发布验收.md)，过程见 [M6 计划](docs/plans/2026-11-12-M6-打磨打包.md)。

Dsh 使用：进入「系统设置 / 应用管理 / DeepSeek Harness」配置认证与模型来源；也可从 Dsh 成员表单进入设置。成员模型可选择已配置的 Web/headless provider、model 和所声明的推理强度。明确选择后沿用该供应商在 Dsh 中的凭据；留空沿用全局默认认证。应用不保存登录令牌或 API Key。

## 开发

```bash
npm install
npm run tauri dev
```

测试：`npm test`（前端）、`cd src-tauri && cargo test`（后端）。

打包：Windows 执行 `npm run tauri build -- --bundles nsis`，macOS 执行 `npm run tauri build -- --bundles dmg`，产物在 `src-tauri/target/release/bundle/`。打包器首次运行会从 GitHub 下载 NSIS 工具链，网络不稳时会报 `timeout: global`，重试即可，已下载的组件会缓存。历史 Windows 包未签名；macOS 使用 ad-hoc 签完整应用及资源，产物须通过 `codesign --verify --deep --strict`。ad-hoc 不等于 Developer ID 签名或 Apple 公证，正式渠道签名、公证与上传尚未执行。若需要同时保留 `.app` 用于与 DMG 的 payload 比对，使用 `--bundles app,dmg`；只构建 DMG 时打包器会清理中间 `.app`。

## 文档

- [MVP 开发手册](docs/MVP开发手册.md)：产品定义、架构、协作流程、数据模型与里程碑
- [CLI 实测记录](docs/CLI实测记录.md)：各 CLI 无头模式的调用参数、事件格式与行为
- [本轮 macOS 修复验收](docs/验收记录-2026-11-17-macOS.md)：原生窗口、真实团队、失败与复验记录
- [安装与发布验收](docs/安装与发布验收.md)：产物来源、哈希、签名与未完成的安装门
- 实施计划：[M0 CLI 实测](docs/plans/2026-11-10-M0-CLI实测.md)、[M1 骨架](docs/plans/2026-11-10-M1-骨架.md)、[M2 单 Agent 对话](docs/plans/2026-11-10-M2-单Agent对话.md)、[M3 团队协作](docs/plans/2026-11-11-M3-团队协作.md)
- [多设备开发指南](docs/多设备开发指南.md)：Windows / Intel Mac / Apple Silicon Mac 之间切换开发的环境清单与协作约定

## Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
