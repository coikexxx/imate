import { useEffect, useRef, useState, type PointerEvent } from "react";
import { createPortal } from "react-dom";
import { Icon } from "../components/ui/Icon";
import type { Agent, AgentStatus } from "../types";
import {
  ROOM_HEIGHT,
  ROOM_WIDTH,
  STATUS_TEXT,
  getOfficeViewport,
  getRegionCamera,
  stepActor,
  syncActors,
  type OfficeActor,
  type Position,
} from "./officeScene";
import { loadOfficeSceneAssets, type OfficeSceneAssets } from "./officeSceneAssets";
import { drawOfficeScene, type ActorHit } from "./officeSceneRenderer";
import { OFFICE_REGIONS, type OfficeRegion, type RegionId } from "./studioMap";

interface Props {
  agents: Agent[];
  statuses: Record<string, AgentStatus>;
}
interface OfficeSimulation {
  actors: Map<string, OfficeActor>;
  elapsed: number;
}

function OfficeView({
  agents,
  statuses,
  simulation,
  paused = false,
  expanded = false,
  onExpand,
  onClose,
}: Props & {
  simulation: OfficeSimulation;
  paused?: boolean;
  expanded?: boolean;
  onExpand?: () => void;
  onClose?: () => void;
}) {
  const host = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const latest = useRef({ agents, statuses, paused });
  latest.current = { agents, statuses, paused };
  const camera = useRef({ zoom: 1, center: { x: ROOM_WIDTH / 2, y: ROOM_HEIGHT / 2 } });
  const hits = useRef<ActorHit[]>([]);
  const hovered = useRef<string | null>(null);
  const drag = useRef<{ pointer: number; x: number; y: number; center: Position } | null>(null);
  const [zoom, setZoom] = useState(1);
  const [regionId, setRegionId] = useState<RegionId | null>(null);
  const [attempt, setAttempt] = useState(0);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(false);

  useEffect(() => {
    const node = canvas.current;
    const wrap = host.current;
    if (!node || !wrap) return;
    const ctx = node.getContext("2d");
    if (!ctx) {
      setLoading(false);
      setError(true);
      return;
    }
    let disposed = false;
    let assets: OfficeSceneAssets | null = null;
    let width = 1;
    let height = 1;
    let pixelRatio = 1;
    let last = 0;
    let request = 0;
    const actors = simulation.actors;
    const reduced = window.matchMedia("(prefers-reduced-motion: reduce)");
    const resize = () => {
      width = Math.max(1, wrap.clientWidth);
      height = Math.max(1, wrap.clientHeight);
      pixelRatio = Math.min(2, window.devicePixelRatio || 1);
      node.width = Math.round(width * pixelRatio);
      node.height = Math.round(height * pixelRatio);
    };
    const observer = new ResizeObserver(resize);
    observer.observe(wrap);
    resize();
    setLoading(true);
    setError(false);
    loadOfficeSceneAssets()
      .then((loaded) => {
        if (disposed) return;
        assets = loaded;
        setLoading(false);
      })
      .catch(() => {
        if (!disposed) {
          setLoading(false);
          setError(true);
        }
      });

    const draw = (now: number) => {
      request = window.requestAnimationFrame(draw);
      if (!assets || latest.current.paused || document.hidden) {
        last = now;
        return;
      }
      if (last && now - last < 1000 / 30) return;
      const delta = last ? Math.min(0.1, (now - last) / 1000) : 0;
      last = now;
      simulation.elapsed += delta;
      syncActors(actors, latest.current.agents, latest.current.statuses);
      for (const actor of actors.values()) stepActor(actor, delta, !reduced.matches);
      const viewport = getOfficeViewport(width, height, camera.current.zoom, camera.current.center);
      camera.current.center = viewport.center;
      hits.current = drawOfficeScene(ctx, {
        width,
        height,
        pixelRatio,
        assets,
        actors,
        viewport,
        members: latest.current.agents,
        tick: reduced.matches ? 0 : Math.floor(simulation.elapsed * 4),
        hoveredId: hovered.current,
        motion: !reduced.matches,
      });
    };
    request = window.requestAnimationFrame(draw);
    return () => {
      disposed = true;
      observer.disconnect();
      window.cancelAnimationFrame(request);
    };
  }, [attempt, simulation]);

  function changeZoom(next: number) {
    const value = Math.min(8, Math.max(1, next));
    camera.current.zoom = value;
    if (value === 1) camera.current.center = { x: ROOM_WIDTH / 2, y: ROOM_HEIGHT / 2 };
    setZoom(value);
    setRegionId(null);
  }

  function focusRegion(region: OfficeRegion) {
    if (!host.current) return;
    const next = getRegionCamera(host.current.clientWidth, host.current.clientHeight, region);
    camera.current = next;
    setZoom(next.zoom);
    setRegionId(region.id);
  }

  function move(event: PointerEvent<HTMLCanvasElement>) {
    const bounds = event.currentTarget.getBoundingClientRect();
    const x = event.clientX - bounds.left;
    const y = event.clientY - bounds.top;
    if (drag.current?.pointer === event.pointerId) {
      const view = getOfficeViewport(bounds.width, bounds.height, camera.current.zoom, drag.current.center);
      camera.current.center = {
        x: drag.current.center.x - (event.clientX - drag.current.x) / view.scale,
        y: drag.current.center.y - (event.clientY - drag.current.y) / view.scale,
      };
      return;
    }
    hovered.current =
      [...hits.current]
        .reverse()
        .find((hit) => x >= hit.x && x <= hit.x + hit.width && y >= hit.y && y <= hit.y + hit.height)?.id ?? null;
  }

  function startDrag(event: PointerEvent<HTMLCanvasElement>) {
    if (zoom === 1 || event.button !== 0) return;
    setRegionId(null);
    drag.current = { pointer: event.pointerId, x: event.clientX, y: event.clientY, center: { ...camera.current.center } };
    event.currentTarget.setPointerCapture(event.pointerId);
    hovered.current = null;
  }

  const activeCount = agents.filter((agent) => {
    const status = statuses[agent.id] ?? "idle";
    return status !== "idle" && status !== "error";
  }).length;
  const errorCount = agents.filter((agent) => statuses[agent.id] === "error").length;
  const activity =
    activeCount > 0
      ? `${activeCount} 位正在协作`
      : errorCount > 0
        ? `${errorCount} 位执行出错`
        : agents.length > 0
          ? "全员待命"
          : "等待成员加入";
  const summary = `2.5D 等轴测微缩工作室沙盘，包含主控门厅、协作工位、研讨圆桌、咖啡图书角和露台花园。${
    agents.length === 0
      ? "还没有成员"
      : agents.map((agent) => `${agent.name}${STATUS_TEXT[statuses[agent.id] ?? "idle"]}`).join("，")
  }`;

  return (
    <div className={`office-scene${expanded ? " office-scene-expanded" : ""}`}>
      <div className="office-toolbar">
        <span className="office-title">
          <i className="office-title-mark" aria-hidden="true" />
          工作室沙盘<span>{agents.length} 位成员</span>
        </span>
        <div className="office-tools">
          <button
            type="button"
            aria-label="缩小沙盘"
            title="缩小"
            disabled={zoom === 1}
            onClick={() => changeZoom(zoom / 1.5)}
          >
            −
          </button>
          <button type="button" aria-label="适应窗口" title="复位全景" onClick={() => changeZoom(1)}>
            <span className="office-fit-icon" aria-hidden="true" />
          </button>
          <button
            type="button"
            aria-label="放大沙盘"
            title="放大"
            disabled={zoom === 8}
            onClick={() => changeZoom(zoom * 1.5)}
          >
            +
          </button>
          {onExpand && (
            <button
              type="button"
              className="office-expand"
              aria-label="展开工作室沙盘"
              title="展开沙盘"
              onClick={onExpand}
            >
              <svg width="13" height="13" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.3" aria-hidden="true">
                <path d="M6 2H2v4m8-4h4v4M2 10v4h4m8-4v4h-4" />
              </svg>
            </button>
          )}
          {onClose && (
            <button
              type="button"
              className="office-expand"
              aria-label="收起工作室沙盘"
              title="收起沙盘（Esc）"
              onClick={onClose}
            >
              <Icon name="close" width="14" height="14" />
            </button>
          )}
        </div>
      </div>

      <div ref={host} className={`office-viewport${zoom > 1 ? " office-can-pan" : ""}`}>
        <canvas
          ref={canvas}
          role="img"
          aria-label={summary}
          onPointerDown={startDrag}
          onPointerMove={move}
          onLostPointerCapture={() => {
            drag.current = null;
          }}
          onPointerUp={(event) => {
            drag.current = null;
            if (event.currentTarget.hasPointerCapture(event.pointerId)) {
              event.currentTarget.releasePointerCapture(event.pointerId);
            }
          }}
          onPointerCancel={() => {
            drag.current = null;
          }}
          onPointerLeave={() => {
            hovered.current = null;
          }}
        />
        {loading && (
          <div className="office-feedback" role="status">
            <p>正在构建工作室沙盘…</p>
          </div>
        )}
        {error && (
          <div className="office-feedback" role="alert">
            <p>工作室沙盘初始化失败</p>
            <button type="button" onClick={() => setAttempt((value) => value + 1)}>
              重新构建
            </button>
          </div>
        )}
        {!loading && !error && agents.length === 0 && (
          <div className="office-empty">添加成员后，微缩工作室将实时映射团队动态</div>
        )}
      </div>

      <div className="office-caption">
        <span>{activity}</span>
        <span>{zoom > 1 ? "拖动画幅 · 点击方框复位" : expanded ? "点击下方分区聚焦局部" : "展开查看区域导览"}</span>
      </div>

      {expanded && (
        <div className="office-bottom">
          <nav className="office-region-nav" aria-label="工作室分区">
            <button type="button" aria-pressed={regionId === null && zoom === 1} onClick={() => changeZoom(1)}>
              全景
            </button>
            {OFFICE_REGIONS.map((region) => (
              <button
                key={region.id}
                type="button"
                aria-pressed={regionId === region.id}
                onClick={() => focusRegion(region)}
              >
                <span style={{ backgroundColor: region.color }} aria-hidden="true" />
                {region.name}
              </button>
            ))}
          </nav>
          <div className="office-legend">
            <span><i className="office-dot-idle" />空闲</span>
            <span><i className="office-dot-thinking" />思考 / 讨论</span>
            <span><i className="office-dot-working" />执行</span>
            <span><i className="office-dot-error" />出错</span>
          </div>
        </div>
      )}
    </div>
  );
}

/** 缩略图与展开图消费相同的成员和实时状态，展开不触发任何 Agent 调用。 */
export function OfficeCanvas({ agents, statuses }: Props) {
  const [expanded, setExpanded] = useState(false);
  const dialog = useRef<HTMLDialogElement>(null);
  const simulation = useRef<OfficeSimulation>({ actors: new Map(), elapsed: 0 });
  useEffect(() => {
    if (expanded && dialog.current && !dialog.current.open) dialog.current.showModal();
  }, [expanded]);
  return (
    <>
      <OfficeView
        agents={agents}
        statuses={statuses}
        simulation={simulation.current}
        paused={expanded}
        onExpand={() => setExpanded(true)}
      />
      {expanded &&
        createPortal(
          <dialog
            ref={dialog}
            className="office-dialog"
            aria-label="微缩工作室沙盘"
            onClose={() => setExpanded(false)}
            onClick={(event) => {
              if (event.target === event.currentTarget) dialog.current?.close();
            }}
          >
            <OfficeView
              agents={agents}
              statuses={statuses}
              simulation={simulation.current}
              expanded
              onClose={() => dialog.current?.close()}
            />
          </dialog>,
          document.body,
        )}
    </>
  );
}
