import type { Agent } from "../types";
import { hashOf, idleAct, shirtColor } from "./layout";
import { CHARACTER_SCALE, officeFrame, ROOM_HEIGHT, ROOM_WIDTH, STATUS_TEXT, type OfficeActor, type OfficeViewport } from "./officeScene";
import type { OfficeSceneAssets } from "./officeSceneAssets";
import { OFFICE_REGIONS } from "./gbaOfficeMap";

export interface ActorHit { id: string; x: number; y: number; width: number; height: number }
interface DrawOptions {
  width: number; height: number; pixelRatio: number; tick: number;
  viewport: OfficeViewport; actors: Map<string, OfficeActor>; members: Agent[];
  assets: OfficeSceneAssets; hoveredId: string | null; motion: boolean;
}

/** 图片放在房间坐标中；名牌和状态气泡保持屏幕字号，缩略图也能辨认成员。 */
export function drawOfficeScene(ctx: CanvasRenderingContext2D, options: DrawOptions): ActorHit[] {
  const { width, height, pixelRatio, viewport, actors, members, assets, tick, hoveredId, motion } = options;
  const { scale, offsetX, offsetY } = viewport;
  ctx.setTransform(pixelRatio, 0, 0, pixelRatio, 0, 0);
  ctx.imageSmoothingEnabled = false;
  ctx.clearRect(0, 0, width, height);
  ctx.fillStyle = "#9ab18c";
  ctx.fillRect(0, 0, width, height);
  ctx.drawImage(assets.background, offsetX, offsetY, ROOM_WIDTH * scale, ROOM_HEIGHT * scale);
  // 区域牌保持屏幕字号，确保全景缩略图也能看清五个区域。
  for (const region of OFFICE_REGIONS) {
    const font = scale < 0.75 ? 9 : 11;
    ctx.font = `600 ${font}px ui-sans-serif, system-ui, sans-serif`;
    const signW = ctx.measureText(region.name).width + 18;
    const signH = font + 8;
    const sx = Math.round(offsetX + (region.col + 0.5) * 16 * scale);
    const sy = Math.round(offsetY + (region.row + 0.1) * 16 * scale);
    ctx.fillStyle = "#3d6264"; ctx.fillRect(sx, sy, signW, signH);
    ctx.fillStyle = "#fbf5dcef"; ctx.fillRect(sx + 1, sy + 1, signW - 2, signH - 2);
    ctx.fillStyle = region.color; ctx.fillRect(sx + 4, sy + 5, 3, signH - 10);
    ctx.fillStyle = "#375258"; ctx.textAlign = "left"; ctx.textBaseline = "middle";
    ctx.fillText(region.name, sx + 11, sy + signH / 2 + 0.5);
  }
  const people = new Map(members.map((member) => [member.id, member]));
  const hits: ActorHit[] = [];
  const ordered = [...actors.values()].sort((a, b) => a.y - b.y);

  for (const actor of ordered) {
    const member = people.get(actor.id);
    if (!member) continue;
    const moving = actor.path.length > 0;
    const frame = officeFrame(actor.status, tick, actor.facing, moving);
    const w = frame.width * CHARACTER_SCALE * scale;
    const h = frame.height * CHARACTER_SCALE * scale;
    const x = Math.round(offsetX + actor.x * scale);
    const y = Math.round(offsetY + (actor.y + (moving ? 0 : 6)) * scale);
    if (x + w < 0 || x - w > width || y < 0 || y - h > height) continue;
    ctx.fillStyle = "#271b314d";
    ctx.fillRect(x - w * 0.35, y - 3 * scale, w * 0.7, 4 * scale);
    if (actor.id === hoveredId) {
      ctx.strokeStyle = "#f8efcd";
      ctx.lineWidth = 2;
      ctx.strokeRect(x - w / 2 - 2, y - h - 2, w + 4, h + 4);
    }
    ctx.save();
    ctx.translate(x, y);
    if (frame.flip) ctx.scale(-1, 1);
    ctx.drawImage(assets.characters[hashOf(actor.id) % assets.characters.length], frame.sx, frame.sy, frame.width, frame.height, -w / 2, -h, w, h);
    ctx.restore();

    if (actor.status === "talking" && actor.facing === "down" && !moving && motion) {
      ctx.fillStyle = "#603f38";
      ctx.fillRect(x - scale, y - h + 19 * scale, 2 * scale, (tick % 2 + 1) * scale);
    }

    const name = Array.from(member.name);
    const text = `${member.isLeader ? "组长 · " : ""}${name.length > 6 ? `${name.slice(0, 5).join("")}…` : member.name}`;
    const showName = scale >= 0.75 || actor.id === hoveredId;
    ctx.font = "500 10px ui-sans-serif, system-ui, sans-serif";
    const plateW = Math.ceil(ctx.measureText(text).width) + 12;
    const plateX = Math.max(2, Math.min(width - plateW - 2, Math.round(x - plateW / 2)));
    const plateY = Math.max(2, Math.min(height - 18, y + 3));
    if (showName) {
      ctx.fillStyle = actor.status === "error" ? "#fff0eb" : "#fff8e9f2";
      ctx.fillRect(plateX, plateY, plateW, 16);
      ctx.fillStyle = shirtColor(actor.id); ctx.fillRect(plateX, plateY, 3, 16);
    } else {
      ctx.fillStyle = "#3b4d62"; ctx.fillRect(x - 3, y + 2, 6, 4);
      ctx.fillStyle = shirtColor(actor.id); ctx.fillRect(x - 2, y + 3, 4, 2);
    }
    ctx.fillStyle = "#473a37";
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    if (showName) ctx.fillText(text, plateX + plateW / 2 + 1, plateY + 8);

    if (actor.status !== "idle") {
      const compact = scale < 0.75;
      const status = actor.status === "thinking" ? (compact ? "·" : ["·", "··", "···"][tick % 3])
        : actor.status === "talking" ? compact ? "聊" : "讨论" : actor.status === "working" ? compact ? "工" : "工作" : "!";
      ctx.font = `600 ${compact ? 8 : 10}px ui-sans-serif, system-ui, sans-serif`;
      const bubbleH = compact ? 12 : 16;
      const bubbleW = Math.max(compact ? 12 : 20, ctx.measureText(status).width + (compact ? 6 : 12));
      const bubbleX = Math.max(2, Math.min(width - bubbleW - 2, x - bubbleW / 2));
      const bubbleY = Math.max(2, y - 24 * scale - bubbleH - 4);
      ctx.fillStyle = actor.status === "error" ? "#b6493c" : actor.status === "working" ? "#3c6e51" : "#fff8e9";
      ctx.fillRect(bubbleX, bubbleY, bubbleW, bubbleH);
      ctx.fillRect(x - 1, bubbleY + bubbleH, 2, 2);
      ctx.fillStyle = actor.status === "error" || actor.status === "working" ? "#fff8e9" : "#473a37";
      ctx.fillText(status, bubbleX + bubbleW / 2, bubbleY + bubbleH / 2);
    } else if (!moving && motion && scale >= 0.75) {
      const act = idleAct(actor.id, tick * 250);
      const left = x + w / 2 + 2;
      const top = y - h / 2;
      if (act === "nap") {
        ctx.fillStyle = "#f8efcd";
        ctx.font = "bold 10px monospace";
        ctx.fillText(tick % 2 ? "z" : "Z", left, top);
      } else {
        ctx.fillStyle = act === "coffee" ? "#ecd6a5" : "#37474c";
        ctx.fillRect(left, top, Math.max(3, 5 * scale), Math.max(4, 7 * scale));
        ctx.fillStyle = act === "coffee" ? "#825a46" : tick % 2 ? "#9ed9c1" : "#a5bfdc";
        ctx.fillRect(left + 1, top + 1, Math.max(1, 3 * scale), Math.max(1, 3 * scale));
      }
    }
    const hitLeft = showName ? Math.min(plateX, x - w / 2) : x - Math.max(6, w / 2);
    const hitRight = showName ? Math.max(plateX + plateW, x + w / 2) : x + Math.max(6, w / 2);
    hits.push({ id: actor.id, x: hitLeft, y: y - h - 20, width: hitRight - hitLeft, height: h + 40 });
  }

  if (hoveredId && people.has(hoveredId)) {
    const person = people.get(hoveredId)!;
    const status = actors.get(hoveredId)?.status ?? "idle";
    ctx.font = "12px ui-sans-serif, system-ui, sans-serif";
    const text = `${person.name} · ${STATUS_TEXT[status]}`;
    const w = Math.min(width - 16, ctx.measureText(text).width + 20);
    ctx.fillStyle = "#fff8e9f5";
    ctx.fillRect(8, height - 30, w, 22);
    ctx.fillStyle = "#473a37";
    ctx.textAlign = "left";
    ctx.fillText(text, 18, height - 19, Math.max(1, w - 20));
  }
  return hits;
}
