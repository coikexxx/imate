/** 原创 16×24 像素人物，放入兼容既有接口的 16×32 帧格；不使用游戏原始素材。 */
const LOOKS = [
  { hair: "#4e4c61", light: "#77718a", shirt: "#5c98ba", trim: "#a6d0d6", skin: "#efc499", cap: false },
  { hair: "#735745", light: "#a67c52", shirt: "#bc787e", trim: "#efb2a1", skin: "#e4b789", cap: true },
  { hair: "#bb9a5b", light: "#e4ca83", shirt: "#72a17a", trim: "#b4cd8b", skin: "#efc499", cap: false },
  { hair: "#665875", light: "#9984a1", shirt: "#b5965f", trim: "#ead28f", skin: "#bb8569", cap: false },
  { hair: "#435b64", light: "#698a8b", shirt: "#6e9e9a", trim: "#b0d2bf", skin: "#e4b789", cap: true },
  { hair: "#927b65", light: "#c0a17f", shirt: "#8b82a6", trim: "#c8b7c8", skin: "#efc499", cap: false },
] as const;

export function createGbaCharacters(): HTMLCanvasElement[] {
  return LOOKS.map((look) => {
    const canvas = document.createElement("canvas");
    canvas.width = 112; canvas.height = 96;
    const ctx = canvas.getContext("2d");
    if (!ctx) throw new Error("无法绘制办公室角色");
    const r = (x: number, y: number, w: number, h: number, color: string) => { ctx.fillStyle = color; ctx.fillRect(x, y, w, h); };
    const outline = "#3b4d62";
    for (let row = 0; row < 3; row += 1) {
      for (let frame = 0; frame < 7; frame += 1) {
        const walking = frame < 3 && frame !== 1;
        const bob = walking ? 1 : 0;
        ctx.save();
        ctx.translate(frame * 16, row * 32 + 8 - bob);
        // 衣服、背包与脚：短身大头比例，步行帧交替伸腿。
        r(4, 13, 8, 8, outline); r(5, 13, 6, 6, look.shirt);
        r(3, 14, 1, 5, outline); r(12, 14, 1, 5, outline);
        r(4, 14, 1, 4, look.trim); r(11, 14, 1, 4, look.shirt);
        r(4, 18, 1, 2, look.skin); r(11, 18, 1, 2, look.skin);
        r(5, 20, 2, walking && frame === 0 ? 4 : 3, outline);
        r(9, 20, 2, walking && frame === 2 ? 4 : 3, outline);
        r(5, 20, 2, 1, "#7e91a1"); r(9, 20, 2, 1, "#7e91a1");
        if (row === 1) { r(5, 14, 6, 5, look.trim); r(6, 15, 4, 4, look.shirt); r(5, 16, 1, 3, outline); }
        else { r(6, 14, 1, 5, look.trim); r(8, 14, 2, 1, look.trim); }

        // 头发与脸，向上帧保留完整后脑，向侧帧带鼻子轮廓。
        r(5, 0, 6, 1, outline); r(3, 1, 10, 2, outline); r(2, 3, 12, 7, outline); r(3, 10, 10, 2, outline);
        r(4, 2, 8, 8, look.hair); r(3, 4, 10, 5, look.hair); r(5, 1, 6, 2, look.light); r(4, 3, 3, 2, look.light);
        if (row !== 1) {
          r(row === 2 ? 7 : 4, 5, row === 2 ? 6 : 8, 5, look.skin);
          r(5, 10, 6, 2, look.skin); r(6, 12, 4, 1, look.skin);
          r(4, 4, 8, 2, look.hair); r(4, 5, 2, 2, look.hair);
          if (row === 0) { r(5, 7, 1, 2, outline); r(10, 7, 1, 2, outline); r(4, 9, 1, 1, "#d7967d"); r(11, 9, 1, 1, "#d7967d"); }
          else { r(10, 7, 1, 2, outline); r(13, 8, 1, 2, look.skin); r(4, 6, 4, 4, look.hair); }
        } else { r(5, 10, 6, 2, look.hair); r(6, 12, 4, 1, look.skin); }
        if (look.cap) { r(3, 2, 10, 3, outline); r(4, 1, 8, 3, look.shirt); r(5, 1, 5, 1, look.trim); r(row === 1 ? 2 : 7, 4, row === 1 ? 12 : 8, 1, look.trim); }
        // 打字帧与阅读帧由真实状态选择，不额外驱动任何任务。
        if (frame === 3 || frame === 4) { const armY = frame === 3 ? 13 : 14; r(3, armY, 2, 2, look.skin); r(11, 15 - (frame % 2), 2, 2, look.skin); }
        if (frame >= 5) { r(4, 16, 8, 4, outline); r(5, 16, 3, 3, "#ead9a4"); r(8, 16, 3, 3, frame === 5 ? "#a4c7be" : "#bed7c6"); r(3, 17, 2, 2, look.skin); r(11, 17, 2, 2, look.skin); }
        ctx.restore();
      }
    }
    return canvas;
  });
}
