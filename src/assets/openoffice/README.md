# OpenOffice 场景素材

来源：<https://github.com/longyangxi/OpenOffice>

固定版本：`5b0246c396aed041c5262ab0132623bf2b8b067b`。上游以 MIT 许可发布，完整版权与许可声明见同目录 `LICENSE`。

| 本地文件 | 上游路径 |
|---|---|
| `background.jpeg`、`room.json` | `apps/web/public/offices/nostalgic.zip` 内的同名文件 |
| `characters/char_0.png` 至 `char_5.png` | `apps/web/public/assets/characters/` |

背景与人物帧图保留原始内容。`room.json` 的 64×64 障碍网格及桌椅标记用于定位工位和寻路。上游 README 对像素办公室注明参考了 `pablodelucca/pixel-agents`；本次资源直接取自上述 OpenOffice 版本。

imates 使用自己的 Canvas 2D 适配层，只消费成员及真实状态；没有接入上游的服务端、编排器或 CLI 启动逻辑。帧图为 112×96，单帧 16×32：三行依次向下、向上、向右，前 3 列为走动，随后两列为打字、最后两列为阅读；向左采用镜像。
