import type { Agent } from "../types";
import { idleAct, shirtColor } from "./layout";
import {
  ROOM_HEIGHT,
  ROOM_WIDTH,
  STATUS_TEXT,
  isoProject,
  type OfficeActor,
  type OfficeViewport,
} from "./officeScene";
import type { OfficeSceneAssets } from "./officeSceneAssets";
import { drawStudioActor } from "./studioActorRenderer";
import { FURNITURE, MEETING_SEATS, OFFICE_REGIONS, REST_SEATS, WORK_SEATS, type FurnitureItem } from "./studioMap";

export interface ActorHit {
  id: string;
  x: number;
  y: number;
  width: number;
  height: number;
}

interface DrawOptions {
  width: number;
  height: number;
  pixelRatio: number;
  tick: number;
  viewport: OfficeViewport;
  actors: Map<string, OfficeActor>;
  members: Agent[];
  assets: OfficeSceneAssets;
  hoveredId: string | null;
  motion: boolean;
}

function poly(
  ctx: CanvasRenderingContext2D,
  points: { x: number; y: number }[],
  fill: string,
  stroke?: string,
  lineWidth = 0.7,
) {
  ctx.beginPath();
  ctx.moveTo(points[0].x, points[0].y);
  for (let i = 1; i < points.length; i += 1) ctx.lineTo(points[i].x, points[i].y);
  ctx.closePath();
  ctx.fillStyle = fill;
  ctx.fill();
  if (stroke) {
    ctx.strokeStyle = stroke;
    ctx.lineWidth = lineWidth;
    ctx.stroke();
  }
}

/** 在等轴测世界坐标中绘制带顶面、西南面、东南面三面明暗的立体长方体。 */
function drawIsoBox(
  ctx: CanvasRenderingContext2D,
  col: number,
  row: number,
  cols: number,
  rows: number,
  z0: number,
  height: number,
  topColor: string,
  swColor: string,
  seColor: string,
  stroke = "rgba(32, 28, 24, 0.18)",
) {
  const z1 = z0 + height;
  const bE = isoProject(col + cols, row, z0);
  const bS = isoProject(col + cols, row + rows, z0);
  const bW = isoProject(col, row + rows, z0);

  const tN = isoProject(col, row, z1);
  const tE = isoProject(col + cols, row, z1);
  const tS = isoProject(col + cols, row + rows, z1);
  const tW = isoProject(col, row + rows, z1);

  poly(ctx, [bW, bS, tS, tW], swColor, stroke);
  poly(ctx, [bS, bE, tE, tS], seColor, stroke);
  poly(ctx, [tN, tE, tS, tW], topColor, stroke);
}

function drawFloorShadow(ctx: CanvasRenderingContext2D, col: number, row: number, cols: number, rows: number) {
  poly(
    ctx,
    [
      isoProject(col - 0.1, row - 0.1, 0.5),
      isoProject(col + cols + 0.25, row - 0.1, 0.5),
      isoProject(col + cols + 0.25, row + rows + 0.25, 0.5),
      isoProject(col - 0.1, row + rows + 0.25, 0.5),
    ],
    "rgba(34, 30, 24, 0.11)",
  );
}

function drawMonitor(ctx: CanvasRenderingContext2D, col: number, row: number, z: number, active: boolean, faceSE: boolean) {
  // 支架
  drawIsoBox(ctx, col - 0.08, row - 0.08, 0.16, 0.16, z, 2.5, "#5a5e5b", "#454846", "#353836");
  // 显示屏薄板
  if (faceSE) {
    drawIsoBox(ctx, col - 0.45, row - 0.06, 0.9, 0.12, z + 2.2, 6.2, "#3a3f3d", "#262b29", active ? "#275e45" : "#323937");
    if (active) {
      const p1 = isoProject(col - 0.36, row + 0.07, z + 3.2);
      const p2 = isoProject(col + 0.36, row + 0.07, z + 7.4);
      ctx.strokeStyle = "#8ce0b3";
      ctx.lineWidth = 1.1;
      ctx.beginPath();
      ctx.moveTo(p1.x, p1.y - 2);
      ctx.lineTo((p1.x + p2.x) / 2, (p1.y + p2.y) / 2 - 1);
      ctx.stroke();
    }
  } else {
    drawIsoBox(ctx, col - 0.45, row - 0.06, 0.9, 0.12, z + 2.2, 6.2, "#464b49", active ? "#275e45" : "#2f3533", "#242826");
  }
}

function drawPlant(ctx: CanvasRenderingContext2D, col: number, row: number, cols = 1, rows = 1) {
  drawFloorShadow(ctx, col + 0.1, row + 0.1, cols - 0.2, rows - 0.2);
  drawIsoBox(ctx, col + 0.15, row + 0.15, cols - 0.3, rows - 0.3, 0, 6.5, "#dfd6c7", "#c9baa3", "#b3a28a");
  drawIsoBox(ctx, col + 0.22, row + 0.22, cols - 0.44, rows - 0.44, 6.2, 0.8, "#6e553e", "#5e4733", "#4f3b2a");
  const center = isoProject(col + cols / 2, row + rows / 2, 11);
  const leaves = [
    { dx: -5, dy: -2, rx: 5.2, ry: 3.2, rot: -0.4, color: "#4f805b" },
    { dx: 5, dy: -1, rx: 5.0, ry: 3.0, rot: 0.45, color: "#3f6e4c" },
    { dx: 0, dy: -5, rx: 5.5, ry: 3.6, rot: 0.1, color: "#64996e" },
    { dx: -2, dy: 2, rx: 4.4, ry: 2.8, rot: -0.2, color: "#5a8c65" },
    { dx: 3, dy: 2, rx: 4.2, ry: 2.6, rot: 0.25, color: "#477553" },
  ];
  for (const leaf of leaves) {
    ctx.beginPath();
    ctx.ellipse(center.x + leaf.dx, center.y + leaf.dy, leaf.rx, leaf.ry, leaf.rot, 0, Math.PI * 2);
    ctx.fillStyle = leaf.color;
    ctx.fill();
  }
}

function drawChair(ctx: CanvasRenderingContext2D, col: number, row: number, facing: "up" | "down" | "left" | "right", tone = "#5c6962") {
  const c = col + 0.5;
  const r = row + 0.5;
  const base = isoProject(c, r, 0);
  ctx.beginPath();
  ctx.ellipse(base.x, base.y, 4.6, 2.5, 0, 0, Math.PI * 2);
  ctx.fillStyle = "rgba(30, 28, 24, 0.13)";
  ctx.fill();

  // 椅柱
  const stemBot = isoProject(c, r, 0.5);
  const stemTop = isoProject(c, r, 4.2);
  ctx.strokeStyle = "#484a47";
  ctx.lineWidth = 1.5;
  ctx.beginPath();
  ctx.moveTo(stemBot.x, stemBot.y);
  ctx.lineTo(stemTop.x, stemTop.y);
  ctx.stroke();

  // 坐垫
  drawIsoBox(ctx, col + 0.24, row + 0.24, 0.52, 0.52, 4.0, 1.8, tone, "#4a554f", "#3d4742");

  // 靠背随朝向放置在对侧边缘
  if (facing === "up") {
    drawIsoBox(ctx, col + 0.24, row + 0.66, 0.52, 0.12, 5.6, 5.2, tone, "#4a554f", "#3d4742");
  } else if (facing === "down") {
    drawIsoBox(ctx, col + 0.24, row + 0.22, 0.52, 0.12, 5.6, 5.2, tone, "#4a554f", "#3d4742");
  } else if (facing === "left") {
    drawIsoBox(ctx, col + 0.66, row + 0.24, 0.12, 0.52, 5.6, 5.2, tone, "#4a554f", "#3d4742");
  } else {
    drawIsoBox(ctx, col + 0.22, row + 0.24, 0.12, 0.52, 5.6, 5.2, tone, "#4a554f", "#3d4742");
  }
}

function drawFurnitureItem(
  ctx: CanvasRenderingContext2D,
  item: FurnitureItem,
  activeSeats: Set<number>,
  anyTalking: boolean,
  tick: number,
  motion: boolean,
) {
  const { kind, col, row, cols, rows } = item;

  if (kind === "plant" || kind === "planterBox") {
    drawPlant(ctx, col, row, cols, rows);
    return;
  }

  drawFloorShadow(ctx, col, row, cols, rows);

  if (kind === "leaderDesk") {
    // 组长胡桃木主控台：双侧钢架支腿 + 厚桌面 + 双联曲面屏 + 黄铜台灯
    drawIsoBox(ctx, col + 0.15, row + 0.2, 0.3, rows - 0.4, 0, 7.5, "#424644", "#363a38", "#2a2d2b");
    drawIsoBox(ctx, col + cols - 0.45, row + 0.2, 0.3, rows - 0.4, 0, 7.5, "#424644", "#363a38", "#2a2d2b");
    drawIsoBox(ctx, col, row, cols, rows, 7.5, 2.2, "#9c6f4c", "#825a3c", "#6b4930");
    const leaderBusy = activeSeats.has(0);
    drawMonitor(ctx, col + 1.4, row + 0.7, 9.7, leaderBusy, true);
    drawMonitor(ctx, col + 2.5, row + 0.7, 9.7, leaderBusy, true);
    // 黄铜建筑师台灯
    const lampBase = isoProject(col + 3.4, row + 0.6, 9.8);
    ctx.strokeStyle = "#c99f52";
    ctx.lineWidth = 1.3;
    ctx.beginPath();
    ctx.moveTo(lampBase.x, lampBase.y);
    ctx.lineTo(lampBase.x + 2, lampBase.y - 7);
    ctx.lineTo(lampBase.x - 3, lampBase.y - 9);
    ctx.stroke();
    ctx.fillStyle = "#e6be6e";
    ctx.beginPath();
    ctx.arc(lampBase.x - 3.5, lampBase.y - 8.5, 2.2, 0, Math.PI * 2);
    ctx.fill();
  } else if (kind === "deskIsland") {
    // 开放式双人／三人创客协作岛：原木桌面 + 中央毛毡屏风 + 对应工位显示器
    drawIsoBox(ctx, col + 0.2, row + 0.2, 0.28, rows - 0.4, 0, 7.2, "#545854", "#434744", "#353835");
    drawIsoBox(ctx, col + cols - 0.48, row + 0.2, 0.28, rows - 0.4, 0, 7.2, "#545854", "#434744", "#353835");
    drawIsoBox(ctx, col, row, cols, rows, 7.2, 2.0, "#e3c59b", "#c8a97e", "#b09268");
    // 中央低矮吸音屏风
    drawIsoBox(ctx, col + 0.25, row + rows / 2 - 0.07, cols - 0.5, 0.14, 9.2, 3.6, "#8fa495", "#768b7c", "#627568");

    if (cols <= 4) {
      const isWestIsland = col < 8;
      const topSeat = isWestIsland ? 1 : 3;
      const botSeat = isWestIsland ? 2 : 4;
      drawMonitor(ctx, col + 1.3, row + 0.65, 9.2, activeSeats.has(topSeat), false);
      drawMonitor(ctx, col + 2.6, row + 1.35, 9.2, activeSeats.has(botSeat), true);
    } else {
      // 南侧三人长岛
      drawMonitor(ctx, col + 1.5, row + 0.65, 9.2, activeSeats.has(5), false);
      drawMonitor(ctx, col + 4.0, row + 1.35, 9.2, activeSeats.has(6), true);
      drawMonitor(ctx, col + 6.4, row + 0.65, 9.2, activeSeats.has(7), false);
    }
  } else if (kind === "roundTable") {
    // 大型研讨圆桌：中央圆柱底座 + 椭圆胡桃木桌面 + 发光蓝图触摸板
    const cx = col + cols / 2;
    const cy = row + rows / 2;
    const pedBot = isoProject(cx, cy, 0);
    const pedTop = isoProject(cx, cy, 7.8);
    ctx.fillStyle = "rgba(30, 28, 24, 0.15)";
    ctx.beginPath();
    ctx.ellipse(pedBot.x, pedBot.y + 1, 34, 18.5, 0, 0, Math.PI * 2);
    ctx.fill();

    // 圆柱支座
    ctx.fillStyle = "#484540";
    ctx.fillRect(pedBot.x - 8, pedTop.y, 16, pedBot.y - pedTop.y);
    ctx.beginPath();
    ctx.ellipse(pedBot.x, pedBot.y, 11, 6, 0, 0, Math.PI * 2);
    ctx.fill();

    // 桌面厚度与顶面
    ctx.fillStyle = "#755138";
    ctx.beginPath();
    ctx.ellipse(pedTop.x, pedTop.y + 2.6, 35, 19, 0, 0, Math.PI * 2);
    ctx.fill();

    ctx.fillStyle = "#9e7252";
    ctx.strokeStyle = "#694831";
    ctx.lineWidth = 0.9;
    ctx.beginPath();
    ctx.ellipse(pedTop.x, pedTop.y, 35, 19, 0, 0, Math.PI * 2);
    ctx.fill();
    ctx.stroke();

    // 桌面中央协作平板／蓝图光环
    ctx.fillStyle = anyTalking ? "#d9f2e4" : "#ece8de";
    ctx.strokeStyle = anyTalking ? "#3b8760" : "#b8b0a0";
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.ellipse(pedTop.x, pedTop.y, 12, 6.5, 0, 0, Math.PI * 2);
    ctx.fill();
    ctx.stroke();

    if (anyTalking && motion) {
      const pulse = 14 + (tick % 3) * 2.5;
      ctx.strokeStyle = "rgba(59, 135, 96, 0.35)";
      ctx.lineWidth = 1.1;
      ctx.beginPath();
      ctx.ellipse(pedTop.x, pedTop.y, pulse, pulse * 0.54, 0, 0, Math.PI * 2);
      ctx.stroke();
    }
  } else if (kind === "espressoBar") {
    // 手冲咖啡吧台：竖纹木柜体 + 石英石台面 + 意式咖啡机与蒸汽
    drawIsoBox(ctx, col, row, cols, rows, 0, 10.5, "#f4f1ea", "#a67954", "#8a6242");
    drawIsoBox(ctx, col - 0.05, row - 0.05, cols + 0.1, rows + 0.1, 10.5, 1.5, "#f7f5ef", "#dedad0", "#cbc6ba");
    // 意式浓缩咖啡机
    drawIsoBox(ctx, col + 0.4, row + 0.4, 1.1, 0.9, 12, 6.5, "#dce0de", "#b8beba", "#969c98");
    // 手冲分享壶
    const pot = isoProject(col + 2.2, row + 0.9, 13);
    ctx.fillStyle = "#d99b52";
    ctx.beginPath();
    ctx.arc(pot.x, pot.y, 2.6, 0, Math.PI * 2);
    ctx.fill();
    if (motion) {
      const wispY = (tick % 4) * 1.6;
      ctx.strokeStyle = "rgba(255, 255, 255, 0.72)";
      ctx.lineWidth = 1.1;
      ctx.beginPath();
      ctx.moveTo(pot.x - 4, pot.y - 5 - wispY);
      ctx.quadraticCurveTo(pot.x - 6, pot.y - 9 - wispY, pot.x - 3, pot.y - 12 - wispY);
      ctx.stroke();
    }
  } else if (kind === "libraryShelf") {
    // 满墙原木书架与彩色书脊
    drawIsoBox(ctx, col, row, cols, rows, 0, 26, "#c99f72", "#b3885c", "#966f48");
    const bookColors = ["#4d7c60", "#c46d58", "#5b7d9c", "#d4a656", "#ede6d6"];
    for (let shelf = 0; shelf < 3; shelf += 1) {
      const z = 5 + shelf * 7;
      for (let i = 0; i < 5; i += 1) {
        const bookColor = bookColors[(shelf * 3 + i) % bookColors.length];
        drawIsoBox(ctx, col + 0.1, row + 0.3 + i * 0.68, 0.65, 0.5, z, 5.2, bookColor, bookColor, bookColor, undefined);
      }
    }
  } else if (kind === "sofa" || kind === "galleryBench" || kind === "gardenBench") {
    const toneTop = kind === "gardenBench" ? "#c99b68" : kind === "galleryBench" ? "#d6cfc2" : "#7c9486";
    const toneSW = kind === "gardenBench" ? "#ad8152" : kind === "galleryBench" ? "#bdb5a6" : "#657d6f";
    const toneSE = kind === "gardenBench" ? "#916a41" : kind === "galleryBench" ? "#a69e8f" : "#53685c";
    drawIsoBox(ctx, col, row, cols, rows, 0, 5.2, toneTop, toneSW, toneSE);
    if (kind === "sofa") {
      drawIsoBox(ctx, col, row, cols, 0.28, 5.2, 5.5, toneTop, toneSW, toneSE);
    }
  } else if (kind === "bistroTable") {
    const cx = col + cols / 2;
    const cy = row + rows / 2;
    const bot = isoProject(cx, cy, 0);
    const top = isoProject(cx, cy, 8.2);
    ctx.strokeStyle = "#5c554c";
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.moveTo(bot.x, bot.y);
    ctx.lineTo(top.x, top.y);
    ctx.stroke();
    ctx.fillStyle = "#f2ede2";
    ctx.strokeStyle = "#b8b0a0";
    ctx.lineWidth = 0.9;
    ctx.beginPath();
    ctx.ellipse(top.x, top.y, 15, 8.5, 0, 0, Math.PI * 2);
    ctx.fill();
    ctx.stroke();
    // 桌面微型盆景
    ctx.fillStyle = "#5c8c64";
    ctx.beginPath();
    ctx.arc(top.x, top.y - 2.5, 3.2, 0, Math.PI * 2);
    ctx.fill();
  } else if (kind === "serverRack") {
    drawIsoBox(ctx, col + 0.1, row + 0.1, cols - 0.2, rows - 0.2, 0, 24, "#454a48", "#343836", "#262928");
    for (let slot = 0; slot < 4; slot += 1) {
      const led = isoProject(col + 0.1, row + 0.6, 6 + slot * 4.2);
      ctx.fillStyle = slot === 1 && motion && tick % 3 === 0 ? "#e0a448" : "#56c288";
      ctx.beginPath();
      ctx.arc(led.x + 2, led.y, 1.1, 0, Math.PI * 2);
      ctx.fill();
    }
  } else if (kind === "wallScreen" || kind === "kanbanBoard") {
    const isKanban = kind === "kanbanBoard";
    drawIsoBox(
      ctx,
      col,
      row - 0.7,
      cols,
      0.22,
      13,
      14,
      "#525754",
      "#3f4441",
      isKanban ? "#f7f3e8" : "#253830",
    );
    if (isKanban) {
      const notes = ["#e8b86d", "#8cc4a2", "#90b4ce", "#e39a86"];
      for (let i = 0; i < 4; i += 1) {
        const p = isoProject(col + 0.45 + i * 0.62, row - 0.45, 22 - (i % 2) * 3.5);
        ctx.fillStyle = notes[i];
        ctx.fillRect(p.x - 2, p.y - 2, 4.2, 3.6);
      }
    } else {
      const p = isoProject(col + cols / 2, row - 0.45, 20);
      ctx.strokeStyle = "#7fd1a6";
      ctx.lineWidth = 1.1;
      ctx.strokeRect(p.x - 10, p.y - 4, 20, 8);
    }
  }
}

function drawPill(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  w: number,
  h: number,
  r: number,
  fill: string,
  stroke?: string,
) {
  ctx.beginPath();
  ctx.roundRect(x, y, w, h, r);
  ctx.fillStyle = fill;
  ctx.fill();
  if (stroke) {
    ctx.strokeStyle = stroke;
    ctx.lineWidth = 0.85;
    ctx.stroke();
  }
}

/** 渲染完整 2.5D 等轴测微缩工作室：静物底图 + 深度排序立体家具与角色 + 屏幕空间清晰铭牌。 */
export function drawOfficeScene(ctx: CanvasRenderingContext2D, options: DrawOptions): ActorHit[] {
  const { width, height, pixelRatio, viewport, actors, members, assets, tick, hoveredId, motion } = options;
  const { scale, offsetX, offsetY } = viewport;

  ctx.setTransform(pixelRatio, 0, 0, pixelRatio, 0, 0);
  ctx.imageSmoothingEnabled = true;
  ctx.imageSmoothingQuality = "high";
  ctx.clearRect(0, 0, width, height);

  ctx.fillStyle = "#ece8de";
  ctx.fillRect(0, 0, width, height);

  // 1. 绘制 2× 预渲染建筑沙盘底座与地面材质
  ctx.drawImage(assets.baseCanvas, offsetX, offsetY, ROOM_WIDTH * scale, ROOM_HEIGHT * scale);

  // 2. 在等轴测世界变换下，按 (col + row) 画家算法深度排序绘制全部立体座椅、家具与角色
  const people = new Map(members.map((member) => [member.id, member]));
  const activeSeats = new Set<number>();
  let anyTalking = false;
  for (const actor of actors.values()) {
    if (actor.status === "working" && actor.path.length === 0) activeSeats.add(actor.seatIndex);
    if (actor.status === "talking") anyTalking = true;
  }

  type Renderable = { depth: number; draw: () => void };
  const sceneItems: Renderable[] = [];

  for (const seat of WORK_SEATS) {
    sceneItems.push({
      depth: seat.col + seat.row + 0.85,
      draw: () => drawChair(ctx, seat.col, seat.row, seat.facing, "#596860"),
    });
  }
  for (const seat of MEETING_SEATS) {
    sceneItems.push({
      depth: seat.col + seat.row + 0.85,
      draw: () => drawChair(ctx, seat.col, seat.row, seat.facing, "#687882"),
    });
  }
  for (let i = 0; i < 2; i += 1) {
    const stool = REST_SEATS[i];
    sceneItems.push({
      depth: stool.col + stool.row + 0.85,
      draw: () => drawChair(ctx, stool.col, stool.row, stool.facing, "#9c7254"),
    });
  }
  for (let i = 4; i < 6; i += 1) {
    const chair = REST_SEATS[i];
    sceneItems.push({
      depth: chair.col + chair.row + 0.85,
      draw: () => drawChair(ctx, chair.col, chair.row, chair.facing, "#7d8c74"),
    });
  }

  for (const item of FURNITURE) {
    const depth = item.col + item.cols * 0.55 + item.row + item.rows * 0.55;
    sceneItems.push({
      depth,
      draw: () => drawFurnitureItem(ctx, item, activeSeats, anyTalking, tick, motion),
    });
  }

  for (const actor of actors.values()) {
    const member = people.get(actor.id);
    if (!member) continue;
    const gx = actor.x / 16;
    const gy = actor.y / 16;
    const pos = isoProject(gx, gy, 0);
    sceneItems.push({
      depth: gx + gy + 0.12,
      draw: () =>
        drawStudioActor({
          ctx,
          id: actor.id,
          screenX: pos.x,
          screenY: pos.y,
          scale: 1,
          facing: actor.facing,
          status: actor.status,
          moving: actor.path.length > 0,
          tick,
          motion,
          isLeader: member.isLeader,
          hovered: actor.id === hoveredId,
        }),
    });
  }

  sceneItems.sort((a, b) => a.depth - b.depth);

  ctx.save();
  ctx.translate(offsetX, offsetY);
  ctx.scale(scale, scale);
  for (const entry of sceneItems) entry.draw();
  ctx.restore();

  // 3. 展开或放大视图下绘制屏幕空间清晰功能区导览牌；侧栏缩略全景保持沙盘纯净不遮挡角色
  if (scale >= 0.55) {
    for (const region of OFFICE_REGIONS) {
      const anchor = isoProject(region.col + region.cols * 0.25, region.row + 0.9, 12);
      const sx = Math.round(offsetX + anchor.x * scale);
      const sy = Math.round(offsetY + anchor.y * scale);
      ctx.font = `500 11px -apple-system, BlinkMacSystemFont, "PingFang SC", sans-serif`;
      const textW = Math.ceil(ctx.measureText(region.name).width);
      const pillW = textW + 18;
      const pillH = 19;
      const px = Math.max(4, Math.min(width - pillW - 4, sx - pillW / 2));
      const py = Math.max(4, Math.min(height - pillH - 4, sy - pillH / 2));

      drawPill(ctx, px, py, pillW, pillH, pillH / 2, "rgba(252, 250, 245, 0.9)", "rgba(186, 179, 165, 0.8)");
      ctx.fillStyle = region.color;
      ctx.beginPath();
      ctx.arc(px + 8, py + pillH / 2, 2.6, 0, Math.PI * 2);
      ctx.fill();

      ctx.fillStyle = "#343633";
      ctx.textAlign = "left";
      ctx.textBaseline = "middle";
      ctx.fillText(region.name, px + 14, py + pillH / 2 + 0.5);
    }
  }

  // 4. 屏幕空间角色名牌、实时状态气泡与命中区
  const hits: ActorHit[] = [];
  const orderedActors = [...actors.values()].sort((a, b) => a.x + a.y - (b.x + b.y));

  for (const actor of orderedActors) {
    const member = people.get(actor.id);
    if (!member) continue;
    const gx = actor.x / 16;
    const gy = actor.y / 16;
    const footWorld = isoProject(gx, gy, 0);
    const headWorld = isoProject(gx, gy, 30);

    const x = Math.round(offsetX + footWorld.x * scale);
    const y = Math.round(offsetY + footWorld.y * scale);
    const headY = Math.round(offsetY + headWorld.y * scale);
    const bodyW = Math.max(12, 16 * scale);
    const bodyH = Math.max(18, y - headY);

    if (x + bodyW < 0 || x - bodyW > width || y < 0 || headY > height) continue;

    const chars = Array.from(member.name);
    const label = `${member.isLeader ? "组长 · " : ""}${chars.length > 6 ? `${chars.slice(0, 5).join("")}…` : member.name}`;
    const showName = scale >= 0.58 || actor.id === hoveredId;

    ctx.font = `500 10px -apple-system, BlinkMacSystemFont, "PingFang SC", sans-serif`;
    const plateW = Math.ceil(ctx.measureText(label).width) + 14;
    const plateX = Math.max(3, Math.min(width - plateW - 3, Math.round(x - plateW / 2)));
    const plateY = Math.max(3, Math.min(height - 19, y + 4));

    if (showName) {
      drawPill(
        ctx,
        plateX,
        plateY,
        plateW,
        16,
        5,
        actor.status === "error" ? "rgba(254, 243, 242, 0.95)" : "rgba(255, 255, 255, 0.93)",
        "rgba(205, 202, 194, 0.85)",
      );
      ctx.fillStyle = shirtColor(actor.id);
      ctx.beginPath();
      ctx.roundRect(plateX + 2, plateY + 3, 3, 10, 1.5);
      ctx.fill();
      ctx.fillStyle = "#222421";
      ctx.textAlign = "center";
      ctx.textBaseline = "middle";
      ctx.fillText(label, plateX + plateW / 2 + 2, plateY + 8.5);
    } else {
      drawPill(ctx, x - 5, y + 2, 10, 5, 2.5, "rgba(255,255,255,0.92)", "rgba(180,176,168,0.8)");
      ctx.fillStyle = shirtColor(actor.id);
      ctx.beginPath();
      ctx.roundRect(x - 3.5, y + 3.2, 7, 2.6, 1.2);
      ctx.fill();
    }

    if (actor.status !== "idle") {
      const compact = scale < 0.58;
      const status =
        actor.status === "thinking"
          ? compact
            ? "···"
            : ["·", "··", "···"][tick % 3]
          : actor.status === "talking"
            ? compact
              ? "聊"
              : "讨论"
            : actor.status === "working"
              ? compact
                ? "工"
                : "执行"
              : "!";
      ctx.font = `600 ${compact ? 8.5 : 10}px -apple-system, BlinkMacSystemFont, "PingFang SC", sans-serif`;
      const bubbleH = compact ? 14 : 17;
      const bubbleW = Math.max(compact ? 15 : 24, Math.ceil(ctx.measureText(status).width) + (compact ? 8 : 12));
      const bubbleX = Math.max(3, Math.min(width - bubbleW - 3, Math.round(x - bubbleW / 2)));
      const bubbleY = Math.max(3, headY - bubbleH - 4);
      const fill =
        actor.status === "error"
          ? "#b42318"
          : actor.status === "working"
            ? "#255c42"
            : actor.status === "talking"
              ? "#356888"
              : "#ffffff";
      const stroke = actor.status === "thinking" ? "rgba(185, 180, 170, 0.9)" : undefined;
      drawPill(ctx, bubbleX, bubbleY, bubbleW, bubbleH, bubbleH / 2, fill, stroke);
      ctx.fillStyle = actor.status === "thinking" ? "#3b3d39" : "#ffffff";
      ctx.textAlign = "center";
      ctx.textBaseline = "middle";
      ctx.fillText(status, bubbleX + bubbleW / 2, bubbleY + bubbleH / 2 + 0.5);
    } else if (actor.path.length === 0 && motion && scale >= 0.58 && idleAct(actor.id, tick * 250) === "nap") {
      ctx.fillStyle = "#6c736b";
      ctx.font = "600 9.5px ui-monospace, SFMono-Regular, monospace";
      ctx.textAlign = "left";
      ctx.fillText(tick % 2 ? "z" : "zZ", x + bodyW * 0.45, headY - 2);
    }

    const hitLeft = showName ? Math.min(plateX, x - bodyW / 2) : x - Math.max(8, bodyW / 2);
    const hitRight = showName ? Math.max(plateX + plateW, x + bodyW / 2) : x + Math.max(8, bodyW / 2);
    hits.push({
      id: actor.id,
      x: hitLeft,
      y: headY - 18,
      width: hitRight - hitLeft,
      height: bodyH + 38,
    });
  }

  if (hoveredId && people.has(hoveredId)) {
    const person = people.get(hoveredId)!;
    const status = actors.get(hoveredId)?.status ?? "idle";
    ctx.font = `500 11.5px -apple-system, BlinkMacSystemFont, "PingFang SC", sans-serif`;
    const text = `${person.name}（${person.role}）· ${STATUS_TEXT[status]}`;
    const w = Math.min(width - 16, Math.ceil(ctx.measureText(text).width) + 22);
    drawPill(ctx, 8, height - 29, w, 22, 6, "rgba(255, 255, 255, 0.94)", "rgba(198, 194, 184, 0.9)");
    ctx.fillStyle = "#20221f";
    ctx.textAlign = "left";
    ctx.textBaseline = "middle";
    ctx.fillText(text, 18, height - 17.5, Math.max(1, w - 20));
  }

  return hits;
}
