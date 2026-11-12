import { describe, expect, it } from "vitest";
import type { AgentStatus } from "../types";
import { CELL_H, CELL_W, DESK_ITEMS } from "./layout";
import { ALL_SPRITES, PALETTE, emptySeatLayers, seatLayers } from "./sprites";

const STATUSES: AgentStatus[] = ["idle", "thinking", "talking", "working", "error"];
const ACTS = ["coffee", "nap", "phone"] as const;

describe("像素图", () => {
  it("每张图都是矩形，只用调色板里的字符", () => {
    expect(ALL_SPRITES.length).toBeGreaterThan(20);
    for (const sprite of ALL_SPRITES) {
      expect(sprite.length).toBeGreaterThan(0);
      for (const row of sprite) {
        expect(row.length).toBe(sprite[0].length);
        for (const char of row) expect(char === "." || char === "C" || char in PALETTE).toBe(true);
      }
    }
  });
  it("每种状态的动画是 2 到 4 帧，图层不超出工位", () => {
    for (const status of STATUSES) {
      for (const act of ACTS) {
        for (const item of DESK_ITEMS) {
          const frames = new Set<string>();
          for (let tick = 0; tick < 12; tick += 1) {
            const layers = seatLayers(status, act, tick, item);
            frames.add(JSON.stringify(layers));
            for (const layer of layers) {
              expect(layer.dx).toBeGreaterThanOrEqual(0);
              expect(layer.dy).toBeGreaterThanOrEqual(0);
              expect(layer.dx + layer.sprite[0].length).toBeLessThanOrEqual(CELL_W);
              expect(layer.dy + layer.sprite.length).toBeLessThanOrEqual(CELL_H);
            }
          }
          expect(frames.size, `${status}/${act}/${item}`).toBeGreaterThanOrEqual(2);
          expect(frames.size, `${status}/${act}/${item}`).toBeLessThanOrEqual(4);
        }
      }
    }
  });
  it("动画按 tick 循环，负数和很大的 tick 也有图", () => {
    expect(seatLayers("thinking", "coffee", 0, "plant")).toEqual(seatLayers("thinking", "coffee", 3, "plant"));
    expect(seatLayers("working", "coffee", -1, "plant").length).toBeGreaterThan(0);
    expect(seatLayers("talking", "coffee", 1_000_001, "plant").length).toBeGreaterThan(0);
  });
  it("三种摸鱼动作画面不同，四种桌面小物画面不同", () => {
    const acts = ACTS.map((act) => JSON.stringify(seatLayers("idle", act, 0, "plant")));
    expect(new Set(acts).size).toBe(3);
    const items = DESK_ITEMS.map((item) => JSON.stringify(seatLayers("working", "coffee", 0, item)));
    expect(new Set(items).size).toBe(DESK_ITEMS.length);
  });
  it("有人的工位包含隔板、椅子、桌子、电脑和键盘；空工位没有小人和桌面小物", () => {
    const occupied = seatLayers("working", "coffee", 0, "plant");
    expect(occupied.length).toBeGreaterThanOrEqual(7);
    const empty = emptySeatLayers();
    expect(empty).toHaveLength(5);
    expect(JSON.stringify(empty)).not.toContain("C");
    expect(emptySeatLayers()).toEqual(empty);
  });
});
