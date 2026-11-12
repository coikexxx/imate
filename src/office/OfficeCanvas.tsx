import { useEffect, useRef } from "react";
import type { Agent, AgentStatus } from "../types";
import { MAX_SEATS, deskItem, idleAct, sceneLayout, shirtColor } from "./layout";
import { sceneryRects } from "./scenery";
import { DESK_SHADOW, NAME_OFFSET, NAME_PLATE_H, PALETTE, emptySeatLayers, seatLayers, type Layer } from "./sprites";

interface Props { agents: Agent[]; statuses: Record<string, AgentStatus> }

const STATUS_TEXT: Record<AgentStatus, string> = {
  idle: "空闲", thinking: "思考中", talking: "发言中", working: "干活中", error: "出错",
};
/** 每秒约 4 帧。 */
const TICK_MS = 250;

function paint(ctx: CanvasRenderingContext2D, layer: Layer, x: number, y: number, scale: number, shirt: string) {
  layer.sprite.forEach((row, rowIndex) => {
    for (let column = 0; column < row.length; column += 1) {
      const char = row[column];
      if (char === ".") continue;
      ctx.fillStyle = char === "C" ? shirt : PALETTE[char];
      ctx.fillRect((x + layer.dx + column) * scale, (y + layer.dy + rowIndex) * scale, scale, scale);
    }
  });
}

/** 像素办公室：小人固定在自己的工位上，动画只表达状态。 */
export function OfficeCanvas({ agents, statuses }: Props) {
  const wrap = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  // 动画循环只建一次，每帧从这里读最新的成员和状态。
  const latest = useRef({ agents, statuses });
  latest.current = { agents, statuses };

  useEffect(() => {
    const host = wrap.current;
    const node = canvas.current;
    if (!host || !node) return;
    let tick = 0;
    let scene = sceneLayout(host.clientWidth, host.clientHeight);

    const draw = () => {
      const ctx = node.getContext("2d");
      if (!ctx) return;
      const { scale } = scene;
      ctx.imageSmoothingEnabled = false;
      for (const rect of sceneryRects(scene, tick)) {
        ctx.fillStyle = rect.color;
        ctx.fillRect(rect.x * scale, rect.y * scale, rect.w * scale, rect.h * scale);
      }
      const { agents: members, statuses: current } = latest.current;
      const now = Date.now();
      for (let seat = 0; seat < MAX_SEATS; seat += 1) {
        const origin = scene.seats[seat];
        const member = members.find((agent) => agent.seatIndex === seat);
        // 桌子在地上的投影
        ctx.fillStyle = "rgba(2, 6, 23, 0.35)";
        ctx.fillRect((origin.x + DESK_SHADOW.dx) * scale, (origin.y + DESK_SHADOW.dy) * scale, DESK_SHADOW.w * scale, DESK_SHADOW.h * scale);
        const layers = member
          ? seatLayers(current[member.id] ?? "idle", idleAct(member.id, now), tick, deskItem(member.id))
          : emptySeatLayers();
        const shirt = member ? shirtColor(member.id) : "#475569";
        for (const layer of layers) paint(ctx, layer, origin.x, origin.y, scale, shirt);
        if (member) {
          // 桌前名牌：深色底、左侧一条成员配色、名字居中。
          const name = member.name.length > 6 ? `${member.name.slice(0, 5)}…` : member.name;
          ctx.font = `${Math.max(9, 6 * scale)}px ui-sans-serif, system-ui, sans-serif`;
          const plateW = Math.min(44, Math.ceil(ctx.measureText(name).width / scale) + 8);
          const plateX = origin.x + NAME_OFFSET.x - Math.floor(plateW / 2);
          ctx.fillStyle = "#0f172a";
          ctx.fillRect(plateX * scale, (origin.y + NAME_OFFSET.y) * scale, plateW * scale, NAME_PLATE_H * scale);
          ctx.fillStyle = shirt;
          ctx.fillRect(plateX * scale, (origin.y + NAME_OFFSET.y) * scale, 2 * scale, NAME_PLATE_H * scale);
          ctx.fillStyle = "#e2e8f0";
          ctx.textAlign = "center";
          ctx.textBaseline = "middle";
          ctx.fillText(name, (origin.x + NAME_OFFSET.x + 1) * scale, (origin.y + NAME_OFFSET.y + NAME_PLATE_H / 2) * scale, (plateW - 4) * scale);
        }
      }
    };
    // 画布的像素尺寸等于容器尺寸，场景按整数倍放大后铺满，超出的边缘被裁掉。
    const resize = () => {
      scene = sceneLayout(host.clientWidth, host.clientHeight);
      node.width = Math.max(1, host.clientWidth);
      node.height = Math.max(1, host.clientHeight);
      draw();
    };
    const observer = new ResizeObserver(resize);
    observer.observe(host);
    resize();
    const timer = window.setInterval(() => { tick += 1; draw(); }, TICK_MS);
    return () => { observer.disconnect(); window.clearInterval(timer); };
  }, []);

  const summary = agents.length === 0
    ? "像素办公室：还没有成员"
    : `像素办公室：${agents.map((agent) => `${agent.name}${STATUS_TEXT[statuses[agent.id] ?? "idle"]}`).join("，")}`;
  return (
    <div ref={wrap} className="relative h-full min-h-0 overflow-hidden rounded border border-slate-800 bg-slate-950">
      <canvas ref={canvas} role="img" aria-label={summary} className="absolute inset-0 block h-full w-full" style={{ imageRendering: "pixelated" }} />
    </div>
  );
}
