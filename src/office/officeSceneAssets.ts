import { DOORWAYS, OFFICE_REGIONS, STUDIO_ROOM } from "./studioMap";
import { ISO_TILE_H, ISO_TILE_W, ROOM_HEIGHT, ROOM_WIDTH, isoProject } from "./officeScene";

export interface OfficeSceneAssets {
  baseCanvas: HTMLCanvasElement;
}

function poly(ctx: CanvasRenderingContext2D, points: { x: number; y: number }[], fill: string, stroke?: string, lineWidth = 1) {
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

function isoQuad(col: number, row: number, cols: number, rows: number, z = 0) {
  return [
    isoProject(col, row, z),
    isoProject(col + cols, row, z),
    isoProject(col + cols, row + rows, z),
    isoProject(col, row + rows, z),
  ];
}

/** 预渲染 2× Retina 的 2.5D 等轴测建筑沙盘静物底图（底座剖面、五区地面材质、天窗剖面墙与阳光投射）。 */
function createDioramaBase(): HTMLCanvasElement {
  const dpr = 2;
  const canvas = document.createElement("canvas");
  canvas.width = ROOM_WIDTH * dpr;
  canvas.height = ROOM_HEIGHT * dpr;
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("无法创建工作室沙盘画布");
  ctx.scale(dpr, dpr);

  // 1. 制图桌亚麻暖底与柔和暗角
  const bgGrad = ctx.createRadialGradient(ROOM_WIDTH / 2, ROOM_HEIGHT * 0.48, 80, ROOM_WIDTH / 2, ROOM_HEIGHT * 0.5, 460);
  bgGrad.addColorStop(0, "#f5f3ec");
  bgGrad.addColorStop(1, "#eae6dc");
  ctx.fillStyle = bgGrad;
  ctx.fillRect(0, 0, ROOM_WIDTH, ROOM_HEIGHT);

  const cols = STUDIO_ROOM.cols;
  const rows = STUDIO_ROOM.rows;
  const slabDepth = 15;

  // 2. 悬浮沙盘模型投影
  ctx.save();
  ctx.filter = "blur(10px)";
  poly(
    ctx,
    [
      { x: isoProject(0, 0, 0).x, y: isoProject(0, 0, 0).y + slabDepth + 6 },
      { x: isoProject(cols, 0, 0).x + 6, y: isoProject(cols, 0, 0).y + slabDepth + 8 },
      { x: isoProject(cols, rows, 0).x, y: isoProject(cols, rows, 0).y + slabDepth + 12 },
      { x: isoProject(0, rows, 0).x - 6, y: isoProject(0, rows, 0).y + slabDepth + 8 },
    ],
    "rgba(32, 30, 25, 0.16)",
  );
  ctx.restore();

  // 3. 立体建筑底座厚度（西南暖灰混凝土面 + 东南背光面 + 原木托边）
  const pN = isoProject(0, 0, 0);
  const pE = isoProject(cols, 0, 0);
  const pS = isoProject(cols, rows, 0);
  const pW = isoProject(0, rows, 0);

  poly(
    ctx,
    [pW, pS, { x: pS.x, y: pS.y + slabDepth }, { x: pW.x, y: pW.y + slabDepth }],
    "#cbc3b3",
    "#b8b09f",
    0.8,
  );
  poly(
    ctx,
    [pS, pE, { x: pE.x, y: pE.y + slabDepth }, { x: pS.x, y: pS.y + slabDepth }],
    "#b7ae9d",
    "#a69d8c",
    0.8,
  );
  // 底座底部深胡桃木细托线
  poly(
    ctx,
    [
      { x: pW.x, y: pW.y + slabDepth - 3 },
      { x: pS.x, y: pS.y + slabDepth - 3 },
      { x: pS.x, y: pS.y + slabDepth },
      { x: pW.x, y: pW.y + slabDepth },
    ],
    "#8f7960",
  );
  poly(
    ctx,
    [
      { x: pS.x, y: pS.y + slabDepth - 3 },
      { x: pE.x, y: pE.y + slabDepth - 3 },
      { x: pE.x, y: pE.y + slabDepth },
      { x: pS.x, y: pS.y + slabDepth },
    ],
    "#78644e",
  );

  // 4. 整层微水泥走廊基底与浅分缝
  poly(ctx, [pN, pE, pS, pW], "#ebe6dc", "#d6d0c3", 1);
  ctx.strokeStyle = "rgba(160, 152, 138, 0.22)";
  ctx.lineWidth = 0.6;
  for (let i = 2; i < cols; i += 2) {
    const a = isoProject(i, 0, 0);
    const b = isoProject(i, rows, 0);
    ctx.beginPath();
    ctx.moveTo(a.x, a.y);
    ctx.lineTo(b.x, b.y);
    ctx.stroke();
    const c = isoProject(0, i, 0);
    const d = isoProject(cols, i, 0);
    ctx.beginPath();
    ctx.moveTo(c.x, c.y);
    ctx.lineTo(d.x, d.y);
    ctx.stroke();
  }

  // 5. 五大功能区独立地面材质
  for (const region of OFFICE_REGIONS) {
    const { id, col, row, cols: rCols, rows: rRows } = region;
    if (id === "command") {
      // 主控门厅：温润洞石地砖 + 松绿迎宾织毯
      poly(ctx, isoQuad(col, row, rCols, rRows, 0.2), "#f3efe7", "#dcd6c8", 0.9);
      poly(ctx, isoQuad(col + 3.5, row + 7.5, 4, 6, 0.4), "#dce6dd", "#adc2b2", 0.8);
    } else if (id === "pods") {
      // 协作工位：暖橡木地板与木纹分缝
      poly(ctx, isoQuad(col, row, rCols, rRows, 0.2), "#e7d3b0", "#ceb58e", 0.9);
      ctx.strokeStyle = "rgba(176, 142, 96, 0.28)";
      ctx.lineWidth = 0.65;
      for (let r = row + 1; r < row + rRows; r += 1) {
        const s = isoProject(col, r, 0.3);
        const e = isoProject(col + rCols, r, 0.3);
        ctx.beginPath();
        ctx.moveTo(s.x, s.y);
        ctx.lineTo(e.x, e.y);
        ctx.stroke();
      }
      // 协作岛下方静音编织地垫
      poly(ctx, isoQuad(3.2, 18.2, 5.6, 3.6, 0.4), "#dedad0", "#c9c4b8", 0.6);
      poly(ctx, isoQuad(10.2, 18.2, 5.6, 3.6, 0.4), "#dedad0", "#c9c4b8", 0.6);
      poly(ctx, isoQuad(4.4, 24.2, 9.2, 3.6, 0.4), "#dedad0", "#c9c4b8", 0.6);
    } else if (id === "hub") {
      // 研讨圆桌：鼠尾草灰绿声学地毯与内嵌描边
      poly(ctx, isoQuad(col, row, rCols, rRows, 0.2), "#eef0eb", "#d8ddd4", 0.9);
      poly(ctx, isoQuad(col + 1.2, row + 3.2, rCols - 2.4, rRows - 4.4, 0.5), "#d4dfd3", "#a7bba6", 0.9);
    } else if (id === "cafe") {
      // 咖啡图书角：赤陶与奶油拼花砖
      poly(ctx, isoQuad(col, row, rCols, rRows, 0.2), "#f2e8dc", "#dbcab8", 0.9);
      for (let r = row; r < row + rRows; r += 1) {
        for (let c = col; c < col + rCols; c += 1) {
          if ((c + r) % 2 === 0) {
            poly(ctx, isoQuad(c + 0.06, r + 0.06, 0.88, 0.88, 0.3), "rgba(214, 173, 142, 0.34)");
          }
        }
      }
    } else if (id === "terrace") {
      // 露台花园：户外柚木防腐木甲板 + 鹅卵石边带
      poly(ctx, isoQuad(col, row, rCols, rRows, 0.2), "#d6cea8", "#bfb698", 0.8);
      poly(ctx, isoQuad(col + 0.5, row + 0.5, rCols - 1, rRows - 1, 0.5), "#d5ae80", "#b88e60", 0.9);
      ctx.strokeStyle = "rgba(130, 92, 52, 0.36)";
      ctx.lineWidth = 0.75;
      for (let c = col + 1; c < col + rCols - 0.5; c += 0.8) {
        const s = isoProject(c, row + 0.5, 0.6);
        const e = isoProject(c, row + rRows - 0.5, 0.6);
        ctx.beginPath();
        ctx.moveTo(s.x, s.y);
        ctx.lineTo(e.x, e.y);
        ctx.stroke();
      }
    }
  }

  // 6. 西北落地天窗投射到室内的温暖日光光斑
  for (const [rStart, rSpan] of [
    [3.5, 4.5],
    [10, 4],
    [18, 5],
    [25, 4],
  ]) {
    poly(
      ctx,
      [
        isoProject(1.2, rStart, 0.6),
        isoProject(7.5, rStart + 1.2, 0.6),
        isoProject(7.5, rStart + rSpan + 1.2, 0.6),
        isoProject(1.2, rStart + rSpan, 0.6),
      ],
      "rgba(255, 251, 235, 0.34)",
    );
  }

  // 7. 两面立体剖面背景墙（NW 工业风落地窗墙 + NE 原木格栅墙）
  const wallH = 44;
  const wallThick = 0.65;

  // NW 墙内立面 (col = 0.35 .. 1, row = 0.35 .. 31)
  poly(
    ctx,
    [
      isoProject(1, 0.35, 0),
      isoProject(1, 31, 0),
      isoProject(1, 31, wallH),
      isoProject(1, 0.35, wallH),
    ],
    "#f6f3eb",
    "#d8d2c4",
    0.9,
  );
  // NW 墙外侧端头面与顶部剖面盖板
  poly(
    ctx,
    [
      isoProject(1 - wallThick, 31, 0),
      isoProject(1, 31, 0),
      isoProject(1, 31, wallH),
      isoProject(1 - wallThick, 31, wallH),
    ],
    "#d6cfc0",
  );
  poly(
    ctx,
    [
      isoProject(1 - wallThick, 0.35, wallH),
      isoProject(1, 0.35, wallH),
      isoProject(1, 31, wallH),
      isoProject(1 - wallThick, 31, wallH),
    ],
    "#e5dfd2",
    "#ccc5b5",
    0.7,
  );

  // NW 墙上的四扇钢框玻璃高窗
  for (const [winRow, winLen] of [
    [3.2, 4.4],
    [9.6, 3.8],
    [17.6, 4.8],
    [24.4, 4.0],
  ]) {
    const zBot = 10;
    const zTop = 37;
    poly(
      ctx,
      [
        isoProject(1.02, winRow, zBot),
        isoProject(1.02, winRow + winLen, zBot),
        isoProject(1.02, winRow + winLen, zTop),
        isoProject(1.02, winRow, zTop),
      ],
      "#d7e8ec",
      "#50625c",
      1.2,
    );
    // 玻璃高光与窗格横竖梃
    const midR = winRow + winLen / 2;
    const midA = isoProject(1.03, midR, zBot);
    const midB = isoProject(1.03, midR, zTop);
    ctx.strokeStyle = "#5b6d67";
    ctx.lineWidth = 0.9;
    ctx.beginPath();
    ctx.moveTo(midA.x, midA.y);
    ctx.lineTo(midB.x, midB.y);
    ctx.stroke();
    const hzA = isoProject(1.03, winRow, zBot + (zTop - zBot) * 0.62);
    const hzB = isoProject(1.03, winRow + winLen, zBot + (zTop - zBot) * 0.62);
    ctx.beginPath();
    ctx.moveTo(hzA.x, hzA.y);
    ctx.lineTo(hzB.x, hzB.y);
    ctx.stroke();
  }

  // NE 墙内立面 (row = 1, col = 1 .. 31)
  poly(
    ctx,
    [
      isoProject(1, 1, 0),
      isoProject(31, 1, 0),
      isoProject(31, 1, wallH),
      isoProject(1, 1, wallH),
    ],
    "#ede8dc",
    "#cfc8b8",
    0.9,
  );
  poly(
    ctx,
    [
      isoProject(31, 1, 0),
      isoProject(31, 1 - wallThick, 0),
      isoProject(31, 1 - wallThick, wallH),
      isoProject(31, 1, wallH),
    ],
    "#bfb7a6",
  );
  poly(
    ctx,
    [
      isoProject(1 - wallThick, 1 - wallThick, wallH),
      isoProject(31, 1 - wallThick, wallH),
      isoProject(31, 1, wallH),
      isoProject(1 - wallThick, 1, wallH),
    ],
    "#e2dcd0",
    "#c9c2b2",
    0.7,
  );

  // NE 墙原木声学格栅装饰段（研讨区与咖啡区背景）
  for (let c = 13.5; c <= 29.5; c += 0.65) {
    if (c > 22.2 && c < 24.2) continue;
    const b = isoProject(c, 1.02, 4);
    const t = isoProject(c, 1.02, wallH - 5);
    ctx.strokeStyle = c > 24 ? "#c49a6c" : "#cbb089";
    ctx.lineWidth = 1.4;
    ctx.beginPath();
    ctx.moveTo(b.x, b.y);
    ctx.lineTo(t.x, t.y);
    ctx.stroke();
  }

  // 墙脚踢脚线
  ctx.strokeStyle = "#bfb6a4";
  ctx.lineWidth = 1.5;
  ctx.beginPath();
  ctx.moveTo(isoProject(1, 31, 1.5).x, isoProject(1, 31, 1.5).y);
  ctx.lineTo(isoProject(1, 1, 1.5).x, isoProject(1, 1, 1.5).y);
  ctx.lineTo(isoProject(31, 1, 1.5).x, isoProject(31, 1, 1.5).y);
  ctx.stroke();

  // 8. 室内半高玻璃轻隔断（仅在非门洞的分区分界段绘制低矮通透隔断，保持开放感）
  const doorSet = new Set(DOORWAYS.map((d) => `${d.col},${d.row}`));
  const drawGlassRail = (c1: number, r1: number, c2: number, r2: number, h = 13) => {
    poly(
      ctx,
      [isoProject(c1, r1, 0), isoProject(c2, r2, 0), isoProject(c2, r2, h), isoProject(c1, r1, h)],
      "rgba(205, 224, 222, 0.36)",
      "rgba(116, 138, 130, 0.55)",
      0.9,
    );
    const topA = isoProject(c1, r1, h);
    const topB = isoProject(c2, r2, h);
    ctx.strokeStyle = "#9b7b56";
    ctx.lineWidth = 1.6;
    ctx.beginPath();
    ctx.moveTo(topA.x, topA.y);
    ctx.lineTo(topB.x, topB.y);
    ctx.stroke();
  };

  // 主控门厅与研讨圆桌之间的半高玻璃屏风段
  for (let r = 2; r <= 12; r += 1) {
    if (!doorSet.has(`11,${r}`) && !doorSet.has(`13,${r}`) && (r <= 5 || r >= 10)) {
      drawGlassRail(12, r, 12, r + 0.9, 14);
    }
  }
  // 研讨圆桌与咖啡角之间的半高玻璃屏风段
  for (let r = 2; r <= 12; r += 1) {
    if (!doorSet.has(`22,${r}`) && (r <= 6 || r >= 10)) {
      drawGlassRail(23, r, 23, r + 0.9, 14);
    }
  }
  // 露台花园外侧东南与西南低矮木栏杆
  drawGlassRail(20, 30.2, 30.2, 30.2, 10);
  drawGlassRail(30.2, 16, 30.2, 30.2, 10);

  // 抑制未使用常量告警并保留网格比例引用
  void ISO_TILE_W;
  void ISO_TILE_H;

  return canvas;
}

let cached: Promise<OfficeSceneAssets> | null = null;

/** 缩略图与展开图共享同一份 2× 等轴测底图缓存；失败时清空缓存以便重试。 */
export function loadOfficeSceneAssets(): Promise<OfficeSceneAssets> {
  if (!cached) {
    cached = Promise.resolve()
      .then(() => ({ baseCanvas: createDioramaBase() }))
      .catch((error: unknown) => {
        cached = null;
        throw error;
      });
  }
  return cached;
}
