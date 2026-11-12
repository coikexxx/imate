# OpenOffice 像素办公室场景接入

## 目标与授权

2026-11-12 用户指定参考 `git@github.com:longyangxi/OpenOffice.git`，并使用该项目的办公室场景。此次授权取代手册中「不引入外部素材」的旧限制。以当前团队工作室界面为基线，在 `codex/openoffice-scene` 实施。

采用 OpenOffice 默认的 Retro（`nostalgic.zip`）场景：原始背景、房间网格、工位位置及六张人物帧图。上游固定为 `5b0246c396aed041c5262ab0132623bf2b8b067b`，保留 MIT 许可与素材来源。源码当前使用 Canvas 2D，直接适配现有画布，不增加运行依赖。

## 架构与边界

- 只替换像素办公室的场景与展示，沿用 imates 的成员、seatIndex 和五种真实状态；不引入上游编排、数据库、网络服务或 CLI 调用。
- 房间的八个工作座位从原始 chair / desk 标记匹配，按行列固定排序；休息区仅用于空闲角色的表现，走动不代表实际任务。
- 使用原始障碍网格寻路，不穿墙；忙碌后返回自己的工位。减少动态效果时停止自主走动和循环帧。
- 缩略场景展示完整房间，支持放大、缩小、复位、拖动画面与展开；展开后继续接收相同实时状态。
- 场景与角色资源随应用离线打包；加载失败有中文提示与重试。
- 不修改真实用户数据库，不推送或合并分支。

## 文件范围与接口

| 文件 | 用途 |
|---|---|
| `src/assets/openoffice/` | 背景、room.json、六张人物帧图、来源说明与 MIT 许可 |
| `src/office/openOffice.ts`、`openOffice.test.ts` | 房间座位、帧图映射、寻路、角色状态和相机纯逻辑 |
| `src/office/openOfficeAssets.ts`、`openOfficeRenderer.ts` | 图片加载、背景与角色绘制 |
| `src/office/OfficeCanvas.tsx` | 保留 agents / statuses 接口，接入新场景与展开交互 |
| `src/index.css` | 场景容器、工具条、展开视图与窄窗口适配 |
| `docs/MVP开发手册.md`、`docs/界面打磨记录.md`、`AGENTS.md` | 回填本次场景约定、实施与验证记录 |

## Task 1：场景资源与状态逻辑

Interfaces：输入 Agent 的 id / seatIndex 和 AgentStatus；输出固定工位、角色帧、可通行路径与相机变换。不依赖 DOM。

- [x] 先写测试：八个工位与原图坐标一致、列表重排不换座位、五状态帧映射、路径不穿墙、状态切回工作时回到原工位、相机适配与边界。
- [x] 运行 `npm test -- src/office/openOffice.test.ts`，确认新接口缺失而失败。
- [x] 导入固定版本素材与许可，实现房间、帧图、寻路和角色状态逻辑，确认 9 项测试通过；补测并修正走到半格时改道需先返回网格中心。
- [x] 提交：`feat: 接入 OpenOffice 复古场景资源与角色状态逻辑`。

## Task 2：画布与展开交互

Interfaces：保留 `OfficeCanvas({ agents, statuses })`。图片加载与画布循环由组件管理，卸载清理 observer / RAF / 监听。

- [ ] 实现背景与原人物帧图绘制，状态气泡、成员配色名牌和组长标记。
- [ ] 接入加载失败重试、清晰缩放、拖动、适应窗口与原生 dialog 展开；展开期间暂停缩略画布。
- [ ] 浏览器临时数据验证空团队、八成员、实时状态变化、展开/关闭、缩放/拖动、窄窗口与加载失败重试。
- [ ] 提交：`feat: 用 OpenOffice 场景展示团队现场并支持展开查看`。

## Task 3：验证与文档

- [ ] 运行前端完整测试、TypeScript / Vite 构建与 `git diff --check`；验证发布产物包含全部场景资源。
- [ ] 更新手册、界面记录和进度表，明确已执行与未执行的验收边界。
- [ ] 提交：`docs: 记录 OpenOffice 场景接入与验证结果`。
