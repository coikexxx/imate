# imates

像管理团队一样管理多个 AI Agent 的 Tauri 2 桌面应用。每个项目有一组分工不同的 Agent，由本机已安装的 CLI（Claude Code、Codex、Grok、DeepSeek Harness）无头进程驱动，按“组长负责制”围绕目标讨论、拆解任务、执行并验收交付。

## 当前状态（2026-11-24）

- **核心协作与适配（M0–M5）**：M0、M1 已完成；Claude、Codex、Grok、Dsh 四套适配器与统一网络代理已接入（Pi 已采样但按决议暂不启用）。组长研判、并行讨论、串行派工、返工、运行中插话／点名干预、2.5D 等轴测微缩工作室沙盘、项目文件监听与规章同步均已落地。
- **权限、护栏与运行可观测性（M6 及后续修复）**：支持按成员设置 CLI 执行权限档位，受阻任务支持项目预授权自动单次提权（覆盖 Codex 与 Claude `permission_denied`）或人工单次授权；组长协议纠错享有每条指令前 3 次独立额度；系统设置提供成本与超时护栏（含触顶等待栏深链）、模型白名单／默认模型、成员消息模型角标及项目用量汇总。
- **界面与前端结构**：采用深松绿全局导航 + 暖白石色中性工作区 + 右侧团队现场（2.5D 工作室沙盘 + 轨迹／成员双视图）；前端组件按 `pages/`、`components/ui/`、`components/layout/`、`components/project/`、`components/settings/` 分层组织。
- **打包与验收边界**：Windows 与 macOS 核心路径验收记录见 [核心流程验收单](docs/核心流程验收单.md)；修复版 `9301e65` 已产出并校验 macOS arm64 DMG（ad-hoc 签名）与 Windows x64 NSIS（交叉构建），两平台干净机安装、Windows 原生打包及 Apple Developer ID 签名公证待执行，详见 [安装与发布验收](docs/安装与发布验收.md)。

### DeepSeek Harness（Dsh）说明

进入「系统设置 → 应用管理 → DeepSeek Harness」可配置默认认证与模型配置来源（Web / headless），也可从成员表单快捷打开。成员明确指定模型时沿用对应供应商在 Dsh 中的凭据与可选推理强度；留空时沿用全局默认认证。imates 不读取或保存登录令牌与 API Key 明文。

## 开发与验证

```bash
npm install                      # 安装前端依赖
npm run tauri dev                # 启动桌面开发模式
npm test                         # 运行前端单测（Vitest）
npm run build                    # 前端类型检查与生产构建
cd src-tauri && cargo test       # 运行后端单测（若报 frontendDist 不存在，先执行 npm run build）
cd src-tauri && cargo build      # 编译后端
```

### 桌面端打包

- **macOS**：`npm run tauri build -- --bundles dmg`（需同时保留 `.app` 比对 payload 时使用 `--bundles app,dmg`），产物须通过 `codesign --verify --deep --strict`。
- **Windows**：`npm run tauri build -- --bundles nsis`（首次构建若下载 NSIS 超时，直接重试即可复用缓存）。
- 产物路径：`src-tauri/target/release/bundle/`。

## 文档索引

- [MVP 开发手册](docs/MVP开发手册.md)：产品定义、架构、协作协议、数据模型、接口与里程碑的唯一权威依据
- [Agent 执行说明](AGENTS.md)：仓库开发约定、文档地图与各模块进度明细
- [CLI 实测记录](docs/CLI实测记录.md)：各 CLI 无头模式调用参数、JSON 事件流与权限行为样本
- [界面打磨记录](docs/界面打磨记录.md)：历次视觉设计、工作室重构、像素办公室与工作台打磨记录
- [核心流程验收单](docs/核心流程验收单.md) · [macOS 修复验收](docs/验收记录-2026-11-17-macOS.md) · [Windows 验收记录](docs/验收记录-2026-11-19-Windows.md)
- [安装与发布验收](docs/安装与发布验收.md) · [多设备开发指南](docs/多设备开发指南.md)
