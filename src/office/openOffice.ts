import room from "../assets/openoffice/room.json";
import type { Agent, AgentStatus } from "../types";
import { hashOf, MAX_SEATS } from "./layout";

export const ROOM = room;
export const ROOM_WIDTH = room.cols * room.cellSize;
export const ROOM_HEIGHT = room.rows * room.cellSize;
export const CHARACTER_SCALE = 2;
export const STATUS_TEXT: Record<AgentStatus, string> = {
  idle: "空闲", thinking: "思考中", talking: "发言中", working: "执行中", error: "出错",
};

export interface Cell { col: number; row: number }
export interface Position { x: number; y: number }
export type Facing = "down" | "up" | "right" | "left";
export interface OfficeSeat extends Cell { facing: Facing }

const desks = room.objects.filter((object) => object.type === "desk");
const chairs = room.objects.filter((object) => object.type === "chair");
const adjacentDesk = (chair: Cell) => desks.find((desk) => Math.abs(desk.col - chair.col) + Math.abs(desk.row - chair.row) === 1);

/** 沿用上游房间的原始座标，沙发不占用 imates 的工作工位。 */
export const WORK_SEATS: OfficeSeat[] = chairs.filter(adjacentDesk)
  .sort((a, b) => a.row - b.row || a.col - b.col).slice(0, MAX_SEATS)
  .map((chair) => {
    const desk = adjacentDesk(chair)!;
    const facing: Facing = desk.col < chair.col ? "left" : desk.col > chair.col ? "right" : desk.row < chair.row ? "up" : "down";
    return { col: chair.col, row: chair.row, facing };
  });
const REST_SEATS = chairs.filter((chair) => !adjacentDesk(chair));

export function cellPoint(cell: Cell): Position {
  return { x: (cell.col + 0.5) * room.cellSize, y: (cell.row + 0.5) * room.cellSize };
}

export function isWalkable(cell: Cell): boolean {
  if (!Number.isInteger(cell.col) || !Number.isInteger(cell.row)
    || cell.col < 0 || cell.row < 0 || cell.col >= room.cols || cell.row >= room.rows) return false;
  const tile = room.tiles[cell.row * room.cols + cell.col];
  return tile >= 1 && tile <= 7;
}

/** 广度优先寻路只经过原始房间的可通行网格；返回值不包含起点。 */
export function findRoomPath(from: Cell, to: Cell): Cell[] {
  if (!isWalkable(from) || !isWalkable(to)) return [];
  const start = from.row * room.cols + from.col;
  const end = to.row * room.cols + to.col;
  if (start === end) return [];
  const parents = new Int32Array(room.cols * room.rows).fill(-1);
  const queue = [start];
  parents[start] = start;
  for (let cursor = 0; cursor < queue.length && parents[end] === -1; cursor += 1) {
    const index = queue[cursor];
    const col = index % room.cols;
    const row = Math.floor(index / room.cols);
    for (const next of [{ col: col + 1, row }, { col: col - 1, row }, { col, row: row + 1 }, { col, row: row - 1 }]) {
      if (!isWalkable(next)) continue;
      const key = next.row * room.cols + next.col;
      if (parents[key] !== -1) continue;
      parents[key] = index;
      queue.push(key);
    }
  }
  if (parents[end] === -1) return [];
  const path: Cell[] = [];
  for (let index = end; index !== start; index = parents[index]) {
    path.push({ col: index % room.cols, row: Math.floor(index / room.cols) });
  }
  return path.reverse();
}

export interface OfficeActor extends Position {
  id: string;
  seatIndex: number;
  home: OfficeSeat;
  cell: Cell;
  path: Cell[];
  facing: Facing;
  status: AgentStatus;
  destination: "desk" | "rest";
  idleSeconds: number;
}

export function createActor(id: string, seatIndex: number): OfficeActor {
  const home = Number.isInteger(seatIndex) ? WORK_SEATS[seatIndex] : undefined;
  if (!home) throw new Error("工位编号无效");
  return {
    id, seatIndex, home, cell: { col: home.col, row: home.row }, ...cellPoint(home),
    path: [], facing: home.facing, status: "idle", destination: "desk", idleSeconds: 0,
  };
}

function routeActor(actor: OfficeActor, target: Cell, destination: OfficeActor["destination"]) {
  const path = findRoomPath(actor.cell, target);
  if (path.length === 0 && (actor.cell.col !== target.col || actor.cell.row !== target.row)) return;
  const origin = cellPoint(actor.cell);
  if (actor.x !== origin.x || actor.y !== origin.y) path.unshift({ ...actor.cell });
  actor.path = path;
  actor.destination = destination;
  actor.idleSeconds = 0;
}

/** 以 ID 保留角色、以 seatIndex 固定工位，成员数组的排序不参与分座。 */
export function syncActors(actors: Map<string, OfficeActor>, members: Pick<Agent, "id" | "seatIndex">[], statuses: Record<string, AgentStatus>) {
  const present = new Set<string>();
  for (const member of members) {
    if (!Number.isInteger(member.seatIndex) || !WORK_SEATS[member.seatIndex]) continue;
    present.add(member.id);
    let actor = actors.get(member.id);
    if (!actor || actor.seatIndex !== member.seatIndex) {
      actor = createActor(member.id, member.seatIndex);
      actors.set(member.id, actor);
    }
    const status = statuses[member.id] ?? "idle";
    if (actor.status !== status) {
      actor.status = status;
      actor.idleSeconds = 0;
      if (status !== "idle") routeActor(actor, actor.home, "desk");
    }
  }
  for (const id of actors.keys()) if (!present.has(id)) actors.delete(id);
}

/** 每帧有界推进；休息是空闲动画，不会改变真实 Agent 状态。 */
export function stepActor(actor: OfficeActor, deltaSeconds: number, motion: boolean) {
  if (!motion) {
    actor.path = [];
    actor.cell = { col: actor.home.col, row: actor.home.row };
    Object.assign(actor, cellPoint(actor.home));
    actor.facing = actor.home.facing;
    actor.destination = "desk";
    actor.idleSeconds = 0;
    return;
  }
  const dt = Math.min(0.1, Math.max(0, deltaSeconds));
  if (actor.path.length === 0) {
    if (actor.status === "idle") {
      actor.idleSeconds += dt;
      // 五个休息座位一人一个；其余成员留在工位，避免挤在同一沙发上。
      const rest = REST_SEATS[actor.seatIndex];
      if (rest && actor.idleSeconds > 8 + hashOf(actor.id) % 10) {
        routeActor(actor, actor.destination === "desk" ? rest : actor.home, actor.destination === "desk" ? "rest" : "desk");
      }
    }
    return;
  }
  let distance = dt * 80;
  while (distance > 0 && actor.path.length > 0) {
    const next = actor.path[0];
    const target = cellPoint(next);
    const dx = target.x - actor.x;
    const dy = target.y - actor.y;
    actor.facing = Math.abs(dx) > Math.abs(dy) ? dx > 0 ? "right" : "left" : dy > 0 ? "down" : "up";
    const length = Math.hypot(dx, dy);
    if (length <= distance) {
      actor.x = target.x;
      actor.y = target.y;
      actor.cell = next;
      actor.path.shift();
      distance -= length;
    } else {
      actor.x += dx / length * distance;
      actor.y += dy / length * distance;
      distance = 0;
    }
  }
  if (actor.path.length === 0) actor.facing = actor.destination === "desk" ? actor.home.facing : "down";
}

/** 帧排列与 OpenOffice 的 assetLoader / spriteData 一致。 */
export function officeFrame(status: AgentStatus, tick: number, facing: Facing, moving: boolean) {
  const frame = Math.max(0, Math.floor(tick));
  const column = moving ? [0, 1, 2, 1][frame % 4]
    : status === "working" ? 3 + frame % 2
    : status === "thinking" ? 5 + frame % 2 : 1;
  const row = facing === "up" ? 1 : facing === "left" || facing === "right" ? 2 : 0;
  return { sx: column * 16, sy: row * 32, width: 16, height: 32, flip: facing === "left" };
}

export interface OfficeViewport {
  scale: number; offsetX: number; offsetY: number; center: Position;
}
const clamp = (value: number, min: number, max: number) => Math.min(max, Math.max(min, value));

/** 小窗完整展示房间；放大后把相机限制在房间范围内。坐标均为 CSS 像素。 */
export function getOfficeViewport(width: number, height: number, zoom: number, center: Position = { x: ROOM_WIDTH / 2, y: ROOM_HEIGHT / 2 }): OfficeViewport {
  const w = Math.max(1, Number.isFinite(width) ? width : 1);
  const h = Math.max(1, Number.isFinite(height) ? height : 1);
  const scale = Math.min(w / ROOM_WIDTH, h / ROOM_HEIGHT) * clamp(Number.isFinite(zoom) ? zoom : 1, 1, 8);
  const halfW = w / (2 * scale);
  const halfH = h / (2 * scale);
  const x = halfW >= ROOM_WIDTH / 2 ? ROOM_WIDTH / 2 : clamp(center.x, halfW, ROOM_WIDTH - halfW);
  const y = halfH >= ROOM_HEIGHT / 2 ? ROOM_HEIGHT / 2 : clamp(center.y, halfH, ROOM_HEIGHT - halfH);
  return { scale, offsetX: w / 2 - x * scale, offsetY: h / 2 - y * scale, center: { x, y } };
}
