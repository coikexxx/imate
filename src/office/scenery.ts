import { CELL_H, CELL_W, SEAT_COLUMNS, type Scene } from "./layout";

/** 环境里的一个纯色块，坐标是逻辑像素。`tag` 标出它属于哪件东西。 */
export interface Rect { x: number; y: number; w: number; h: number; color: string; tag: string }

const C = {
  wall: "#2b3a55", wallSeam: "#263450", wallTrim: "#1c2740", baseboard: "#151c2c",
  floor: "#4b3a2f", floorLine: "#3d2f26", floorShade: "#33271f",
  rug: "#2f4858", rugEdge: "#3f6072",
  frame: "#64748b", sill: "#94a3b8", skyTop: "#60a5fa", skyLow: "#93c5fd", cloud: "#f8fafc",
  board: "#e2e8f0", ink: "#3b82f6", red: "#ef4444", green: "#4ade80", leaf: "#16a34a",
  yellow: "#facc15", pink: "#f472b6", purple: "#c084fc", white: "#f8fafc", dark: "#1e293b",
  wood: "#8b5a2b", woodDark: "#5c3d22", woodDeep: "#3a2616", pot: "#c2410c", potRim: "#ea580c",
  metal: "#94a3b8", metalLight: "#cbd5e1", metalDark: "#475569", water: "#7dd3fc", waterLight: "#bae6fd",
  door: "#7a4f2a", doorPanel: "#5c3d22",
} as const;
const BOOK_COLORS = [C.red, C.ink, C.yellow, C.green, C.purple, C.pink] as const;

type Put = (x: number, y: number, w: number, h: number, color: string) => void;
/** 立在地上的家具：`x` 是左边，`base` 是底边。 */
interface Furniture { w: number; draw: (put: Put, x: number, base: number, tick: number) => void }

const mod = (value: number, size: number) => ((value % size) + size) % size;

function drawWindow(put: Put, x: number, y: number, tick: number, seed: number) {
  put(x, y, 40, 20, C.frame);
  put(x + 2, y + 2, 36, 8, C.skyTop);
  put(x + 2, y + 10, 36, 8, C.skyLow);
  // 两朵云从左往右飘，出了玻璃的部分裁掉。
  const glassLeft = x + 2;
  const glassRight = x + 38;
  const cloud = (offset: number, top: number, speed: number) => {
    const left = glassLeft + mod(Math.floor(tick / speed) + offset, 36 + 12) - 12;
    for (const [dx, dy, w, h] of [[0, 1, 12, 2], [3, 0, 6, 1]] as const) {
      const from = Math.max(glassLeft, left + dx);
      const to = Math.min(glassRight, left + dx + w);
      if (to > from) put(from, top + dy, to - from, h, C.cloud);
    }
  };
  cloud(seed * 17, y + 4, 6);
  cloud(seed * 29 + 24, y + 11, 9);
  put(x + 19, y + 2, 2, 16, C.frame);
  put(x + 2, y + 9, 36, 1, C.frame);
  put(x - 1, y + 20, 42, 2, C.sill);
}

function drawClock(put: Put, x: number, y: number, tick: number) {
  put(x + 3, y, 8, 14, C.dark);
  put(x, y + 3, 14, 8, C.dark);
  put(x + 1, y + 1, 12, 12, C.dark);
  put(x + 3, y + 1, 8, 12, C.white);
  put(x + 1, y + 3, 12, 8, C.white);
  put(x + 2, y + 2, 10, 10, C.white);
  put(x + 4, y + 7, 3, 1, C.dark);
  put(x + 7, y + 3, 1, 4, C.dark);
  // 秒针每秒转一格。
  const hand = [[7, 2, 1, 5], [7, 7, 5, 1], [7, 7, 1, 5], [2, 7, 5, 1]][mod(Math.floor(tick / 4), 4)];
  put(x + hand[0], y + hand[1], hand[2], hand[3], C.red);
  put(x + 6, y + 6, 2, 2, C.dark);
}

function drawWhiteboard(put: Put, x: number, y: number) {
  put(x, y, 38, 20, C.sill);
  put(x + 1, y + 1, 36, 18, C.board);
  put(x + 4, y + 4, 14, 1, C.ink);
  put(x + 4, y + 7, 10, 1, C.ink);
  put(x + 4, y + 10, 16, 1, C.red);
  put(x + 24, y + 12, 2, 4, C.green);
  put(x + 27, y + 9, 2, 7, C.green);
  put(x + 30, y + 6, 2, 10, C.green);
  put(x + 5, y + 13, 4, 4, C.yellow);
  put(x + 11, y + 13, 4, 4, C.pink);
  put(x + 8, y + 20, 22, 2, C.frame);
}

function drawShelf(put: Put, x: number, y: number) {
  put(x, y + 18, 34, 2, C.wood);
  put(x + 3, y + 20, 2, 2, C.woodDark);
  put(x + 29, y + 20, 2, 2, C.woodDark);
  for (let book = 0; book < 8; book += 1) {
    const height = 8 + mod(book * 5, 4) * 2;
    put(x + 2 + book * 3, y + 18 - height, 2, height, BOOK_COLORS[book % BOOK_COLORS.length]);
  }
  put(x + 27, y + 14, 5, 4, C.pot);
  put(x + 28, y + 9, 3, 5, C.leaf);
  put(x + 26, y + 11, 7, 2, C.green);
}

function drawDoor(put: Put, x: number, base: number) {
  put(x, base - 30, 20, 30, C.woodDeep);
  put(x + 2, base - 28, 16, 28, C.door);
  put(x + 4, base - 25, 12, 9, C.doorPanel);
  put(x + 4, base - 13, 12, 10, C.doorPanel);
  put(x + 15, base - 15, 2, 2, C.yellow);
}

const PLANT: Furniture = {
  w: 14,
  draw: (put, x, base) => {
    put(x + 4, base - 23, 6, 4, C.leaf);
    put(x + 5, base - 20, 4, 12, C.green);
    put(x + 2, base - 17, 10, 5, C.leaf);
    put(x, base - 14, 14, 3, C.green);
    put(x + 2, base - 9, 10, 2, C.potRim);
    put(x + 3, base - 7, 8, 7, C.pot);
  },
};
const COOLER: Furniture = {
  w: 12,
  draw: (put, x, base, tick) => {
    put(x + 2, base - 26, 8, 11, C.water);
    put(x + 3, base - 25, 2, 8, C.waterLight);
    put(x + 6, base - 18 - mod(tick, 3) * 2, 1, 1, C.white);
    put(x + 4, base - 15, 4, 1, C.ink);
    put(x, base - 14, 12, 14, C.metalLight);
    put(x + 1, base - 9, 10, 1, C.metal);
    put(x + 3, base - 12, 2, 2, C.ink);
    put(x + 7, base - 12, 2, 2, C.red);
  },
};
const PRINTER: Furniture = {
  w: 20,
  draw: (put, x, base, tick) => {
    put(x, base - 12, 20, 12, C.woodDark);
    put(x + 1, base - 11, 18, 1, C.wood);
    put(x + 10, base - 10, 1, 10, C.woodDeep);
    put(x + 4, base - 22, 12, 2, C.white);
    put(x + 2, base - 20, 16, 8, C.metal);
    put(x + 4, base - 16, 12, 2, C.metalDark);
    put(x + 15, base - 19, 1, 1, mod(tick, 8) < 4 ? C.green : C.leaf);
  },
};
const COFFEE: Furniture = {
  w: 24,
  draw: (put, x, base, tick) => {
    put(x + 1, base - 7, 2, 7, C.woodDark);
    put(x + 21, base - 7, 2, 7, C.woodDark);
    put(x, base - 10, 24, 3, C.wood);
    put(x + 3, base - 22, 10, 12, C.metalDark);
    put(x + 5, base - 20, 6, 3, C.dark);
    put(x + 11, base - 21, 1, 1, C.red);
    put(x + 6, base - 13, 3, 3, C.white);
    put(x + 7, base - 15 - mod(tick, 2), 1, 1, C.metalLight);
    put(x + 16, base - 13, 3, 3, C.yellow);
    put(x + 20, base - 13, 3, 3, C.pink);
  },
};
const BOOKCASE: Furniture = {
  w: 22,
  draw: (put, x, base) => {
    put(x, base - 34, 22, 34, C.woodDark);
    for (let shelf = 0; shelf < 3; shelf += 1) {
      const top = base - 32 + shelf * 10;
      put(x + 2, top, 18, 8, C.woodDeep);
      for (let book = 0; book < 5; book += 1) {
        const height = 5 + mod(book * 3 + shelf, 3);
        put(x + 3 + book * 3, top + 8 - height, 2, height, BOOK_COLORS[(book + shelf * 2) % BOOK_COLORS.length]);
      }
    }
  },
};

/** 两侧空地的家具：外层按离工位由近到远的列，内层是前后两排。 */
const SIDE_FURNITURE: readonly (readonly (readonly Furniture[])[])[] = [
  [[PLANT, COOLER], [BOOKCASE, COFFEE], [PRINTER, PLANT]],
  [[PRINTER, PLANT], [COFFEE, BOOKCASE], [PLANT, COOLER]],
];

/** 一帧里办公室环境的全部色块，按从后到前的顺序：墙、地板、墙上挂件、地毯、两侧家具。 */
export function sceneryRects(scene: Scene, tick: number): Rect[] {
  const rects: Rect[] = [];
  const tagged = (tag: string): Put => (x, y, w, h, color) => { rects.push({ x, y, w, h, color, tag }); };
  const { width, height, wallH, seats } = scene;

  // 后墙：整面墙、顶线、竖向接缝、踢脚线。
  const wall = tagged("wall");
  wall(0, 0, width, wallH, C.wall);
  wall(0, 0, width, 2, C.wallTrim);
  for (let x = 31; x < width; x += 32) wall(x, 2, 1, wallH - 5, C.wallSeam);
  wall(0, wallH - 3, width, 3, C.baseboard);

  // 地板：整块木地板、墙根阴影、错缝的木板线。
  const floor = tagged("floor");
  const floorH = height - wallH;
  floor(0, wallH, width, floorH, C.floor);
  floor(0, wallH, width, 2, C.floorShade);
  for (let row = 0; row * 8 < floorH; row += 1) {
    const top = wallH + row * 8;
    if (top + 7 < height) floor(0, top + 7, width, 1, C.floorLine);
    const plankH = Math.min(7, height - top);
    for (let x = row % 2 === 0 ? 12 : 0; x < width; x += 24) floor(x, top, 1, plankH, C.floorLine);
  }

  // 墙上挂件：把墙分成若干段，每段居中挂一件，窗户优先。
  const segments = Math.max(1, Math.floor(width / 72));
  const hangTop = Math.max(2, Math.floor((wallH - 4 - 22) / 2));
  const others = ["whiteboard", "clock", "shelf"] as const;
  for (let index = 0; index < segments; index += 1) {
    const center = Math.floor(((index + 0.5) * width) / segments);
    if (segments >= 6 && index === segments - 1) {
      drawDoor(tagged("door"), center - 10, wallH);
    } else if (index % 2 === 0) {
      drawWindow(tagged("window"), center - 20, hangTop, tick, index);
    } else {
      const kind = others[Math.floor(index / 2) % others.length];
      if (kind === "whiteboard") drawWhiteboard(tagged(kind), center - 19, hangTop);
      else if (kind === "clock") drawClock(tagged(kind), center - 7, hangTop + 4, tick);
      else drawShelf(tagged(kind), center - 17, hangTop);
    }
  }

  // 工位区地毯。
  const rug = tagged("rug");
  const blockLeft = seats[0].x;
  const blockRight = seats[SEAT_COLUMNS - 1].x + CELL_W;
  const rugLeft = Math.max(0, blockLeft - 6);
  const rugRight = Math.min(width, blockRight + 6);
  const rugTop = seats[0].y + 20;
  const rugBottom = Math.min(height, seats[seats.length - 1].y + CELL_H);
  rug(rugLeft, rugTop, rugRight - rugLeft, rugBottom - rugTop, C.rugEdge);
  if (rugRight - rugLeft > 4 && rugBottom - rugTop > 4) {
    rug(rugLeft + 2, rugTop + 2, rugRight - rugLeft - 4, rugBottom - rugTop - 4, C.rug);
  }

  // 两侧空地的家具：空地够宽才摆，底边对齐前后两排桌子。
  const furniture = tagged("furniture");
  const margins = [blockLeft, width - blockRight];
  const bases = [seats[0].y + 40, seats[SEAT_COLUMNS].y + 40];
  margins.forEach((margin, side) => {
    if (margin < 22) return;
    const columns = Math.max(1, Math.min(3, Math.floor(margin / 28)));
    const columnW = margin / columns;
    for (let column = 0; column < columns; column += 1) {
      // column 0 紧挨工位区。
      const center = side === 0 ? blockLeft - (column + 0.5) * columnW : blockRight + (column + 0.5) * columnW;
      SIDE_FURNITURE[side][column].forEach((item, row) => {
        if (item.w + 2 > columnW) return;
        item.draw(furniture, Math.floor(center - item.w / 2), bases[row], tick);
      });
    }
  });

  return rects;
}
