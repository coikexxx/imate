import { useEffect, useRef } from "react";
import type { Agent, AgentStatus } from "../types";
import { CELL, LOGICAL_H, LOGICAL_W, MAX_SEATS, idleAct, integerScale, seatOrigin, shirtColor } from "./layout";
import { NAME_OFFSET, PALETTE, emptySeatLayers, seatLayers, type Layer } from "./sprites";

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
    let scale = 1;

    const draw = () => {
      const ctx = node.getContext("2d");
      if (!ctx) return;
      ctx.imageSmoothingEnabled = false;
      ctx.fillStyle = "#0f172a";
      ctx.fillRect(0, 0, node.width, node.height);
      const { agents: members, statuses: current } = latest.current;
      const now = Date.now();
      for (let seat = 0; seat < MAX_SEATS; seat += 1) {
        const origin = seatOrigin(seat);
        if (!origin) continue;
        // 地板分隔线
        ctx.fillStyle = "#1e293b";
        ctx.fillRect(origin.x * scale, (origin.y + CELL - 1) * scale, CELL * scale, scale);
        const member = members.find((agent) => agent.seatIndex === seat);
        const layers = member ? seatLayers(current[member.id] ?? "idle", idleAct(member.id, now), tick) : emptySeatLayers();
        const shirt = member ? shirtColor(member.id) : "#475569";
        for (const layer of layers) paint(ctx, layer, origin.x, origin.y, scale, shirt);
        if (member) {
          const name = member.name.length > 6 ? `${member.name.slice(0, 5)}…` : member.name;
          ctx.fillStyle = "#cbd5e1";
          ctx.font = `${Math.max(10, 6 * scale)}px ui-sans-serif, system-ui, sans-serif`;
          ctx.textAlign = "center";
          ctx.textBaseline = "top";
          ctx.fillText(name, (origin.x + NAME_OFFSET.x) * scale, (origin.y + NAME_OFFSET.y) * scale);
        }
      }
    };
    const resize = () => {
      scale = integerScale(host.clientWidth, host.clientHeight);
      node.width = LOGICAL_W * scale;
      node.height = LOGICAL_H * scale;
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
    <div ref={wrap} className="flex h-full min-h-0 items-center justify-center overflow-hidden rounded border border-slate-800 bg-slate-950">
      <canvas ref={canvas} role="img" aria-label={summary} style={{ imageRendering: "pixelated" }} />
    </div>
  );
}
