import { describe, expect, it } from "vitest";
import { sceneLayout } from "./layout";
import { sceneryRects } from "./scenery";

const SIZES = [[676, 280], [1316, 390], [900, 640], [264, 124]] as const;

describe("办公室环境", () => {
  it("墙面和地板盖住场景里的每一个像素，不留白", () => {
    for (const [width, height] of SIZES) {
      const scene = sceneLayout(width, height);
      const covered = new Uint8Array(scene.width * scene.height);
      for (const rect of sceneryRects(scene, 0)) {
        for (let y = Math.max(0, rect.y); y < Math.min(scene.height, rect.y + rect.h); y += 1) {
          for (let x = Math.max(0, rect.x); x < Math.min(scene.width, rect.x + rect.w); x += 1) {
            covered[y * scene.width + x] = 1;
          }
        }
      }
      expect(covered.every((value) => value === 1), `${width}x${height}`).toBe(true);
    }
  });
  it("每个色块坐标为整数、面积为正、颜色是十六进制色值", () => {
    for (const [width, height] of SIZES) {
      for (const tick of [0, 1, 7, 1_000_001]) {
        for (const rect of sceneryRects(sceneLayout(width, height), tick)) {
          expect([rect.x, rect.y, rect.w, rect.h].every(Number.isInteger)).toBe(true);
          expect(rect.w).toBeGreaterThan(0);
          expect(rect.h).toBeGreaterThan(0);
          expect(rect.color).toMatch(/^#[0-9a-f]{6}$/);
        }
      }
    }
  });
  it("同一帧结果稳定，窗外的云会随时间移动", () => {
    const scene = sceneLayout(676, 280);
    expect(sceneryRects(scene, 5)).toEqual(sceneryRects(scene, 5));
    expect(sceneryRects(scene, 0)).not.toEqual(sceneryRects(scene, 240));
  });
  it("宽场景两侧摆放家具，窄场景不摆也不挤占工位", () => {
    const base = (width: number, height: number) => {
      const scene = sceneLayout(width, height);
      return sceneryRects(scene, 0).filter((rect) => rect.tag === "furniture").length;
    };
    expect(base(264, 124)).toBe(0);
    expect(base(676, 280)).toBeGreaterThan(0);
    expect(base(1316, 390)).toBeGreaterThan(base(676, 280));
  });
  it("墙上至少有一扇窗，宽场景还有挂钟和白板", () => {
    const tags = (width: number, height: number) =>
      new Set(sceneryRects(sceneLayout(width, height), 0).map((rect) => rect.tag));
    expect(tags(264, 124).has("window")).toBe(true);
    const wide = tags(676, 280);
    for (const tag of ["window", "clock", "whiteboard"]) expect(wide.has(tag), tag).toBe(true);
  });
});
