import type { AgentStatus } from "../types";
import { hashOf, idleAct, shirtColor } from "./layout";
import type { Facing } from "./officeScene";

const HAIR_PALETTE = [
  { main: "#2f2c2a", highlight: "#524c48", skin: "#f2cbb0", style: "glasses" },
  { main: "#5e4031", highlight: "#825b46", skin: "#ecc0a1", style: "headphones" },
  { main: "#3c434d", highlight: "#626d7a", skin: "#f4d1b8", style: "bun" },
  { main: "#78583a", highlight: "#9e7752", skin: "#e6b696", style: "parted" },
  { main: "#263238", highlight: "#485a63", skin: "#f0c7ab", style: "bob" },
  { main: "#8c6548", highlight: "#b58967", skin: "#f4d2ba", style: "wavy" },
] as const;

function shadeHex(hex: string, delta: number): string {
  const clean = hex.replace("#", "");
  const num = Number.parseInt(clean, 16);
  const clamp = (v: number) => Math.max(0, Math.min(255, v + delta));
  const r = clamp((num >> 16) & 255);
  const g = clamp((num >> 8) & 255);
  const b = clamp(num & 255);
  return `rgb(${r}, ${g}, ${b})`;
}

export interface DrawActorParams {
  ctx: CanvasRenderingContext2D;
  id: string;
  screenX: number;
  screenY: number;
  scale: number;
  facing: Facing;
  status: AgentStatus;
  moving: boolean;
  tick: number;
  motion: boolean;
  isLeader: boolean;
  hovered: boolean;
}

/** 绘制原创 2.5D 等轴测微缩工作室人偶（圆润建筑模型比例，无像素锯齿、无游戏棒球帽特征）。 */
export function drawStudioActor(params: DrawActorParams) {
  const { ctx, id, screenX, screenY, scale, facing, status, moving, tick, motion, isLeader, hovered } = params;
  const look = HAIR_PALETTE[hashOf(id) % HAIR_PALETTE.length];
  const baseShirt = shirtColor(id);
  const jacket = shadeHex(baseShirt, -18);
  const jacketDark = shadeHex(baseShirt, -46);
  const jacketLight = shadeHex(baseShirt, 18);

  // 等轴测屏幕朝向：grid up(row-) -> 右上(NE)，down(row+) -> 左下(SW)，right(col+) -> 右下(SE)，left(col-) -> 左上(NW)
  const facingBack = facing === "up" || facing === "left";
  const facingRight = facing === "up" || facing === "right";
  const dirX = facingRight ? 1 : -1;

  const stride = moving && motion ? Math.sin((tick % 4) * (Math.PI / 2)) : 0;
  const bob = moving && motion ? Math.abs(stride) * 1.6 : status === "working" && motion ? (tick % 2) * 0.6 : 0;

  ctx.save();
  ctx.translate(screenX, screenY);
  ctx.scale(scale, scale);

  // 1. 地面柔和椭圆投影与悬停光环
  ctx.beginPath();
  ctx.ellipse(0, 0, 7.5, 4.2, 0, 0, Math.PI * 2);
  ctx.fillStyle = "rgba(28, 26, 22, 0.22)";
  ctx.fill();

  if (hovered) {
    ctx.beginPath();
    ctx.ellipse(0, 0, 10, 5.6, 0, 0, Math.PI * 2);
    ctx.strokeStyle = "#255c42";
    ctx.lineWidth = 1.4;
    ctx.stroke();
  }

  ctx.translate(0, -bob);

  // 2. 双腿（深灰休闲西裤，行走时前后迈步）
  const leftLegY = stride * 1.8;
  const rightLegY = -stride * 1.8;
  ctx.lineCap = "round";
  ctx.strokeStyle = "#3a3d3b";
  ctx.lineWidth = 2.8;
  ctx.beginPath();
  ctx.moveTo(-2.4, -7.5);
  ctx.lineTo(-2.6 + stride * 1.2 * dirX, -1.5 + leftLegY * 0.4);
  ctx.moveTo(2.4, -7.5);
  ctx.lineTo(2.6 - stride * 1.2 * dirX, -1.5 + rightLegY * 0.4);
  ctx.stroke();

  // 鞋尖
  ctx.fillStyle = "#262826";
  ctx.beginPath();
  ctx.ellipse(-2.6 + stride * 1.2 * dirX, -1.1 + leftLegY * 0.4, 1.9, 1.1, 0, 0, Math.PI * 2);
  ctx.ellipse(2.6 - stride * 1.2 * dirX, -1.1 + rightLegY * 0.4, 1.9, 1.1, 0, 0, Math.PI * 2);
  ctx.fill();

  // 3. 立体圆锥微缩躯干（双面明暗渐变表现 2.5D 体积感）
  const torsoGrad = ctx.createLinearGradient(-6, -18, 6, -7);
  torsoGrad.addColorStop(0, jacketLight);
  torsoGrad.addColorStop(0.55, jacket);
  torsoGrad.addColorStop(1, jacketDark);

  ctx.beginPath();
  ctx.moveTo(-4.6, -17.2);
  ctx.quadraticCurveTo(0, -18.8, 4.6, -17.2);
  ctx.lineTo(5.6, -7.5);
  ctx.quadraticCurveTo(0, -5.8, -5.6, -7.5);
  ctx.closePath();
  ctx.fillStyle = torsoGrad;
  ctx.fill();
  ctx.strokeStyle = "rgba(24, 25, 23, 0.24)";
  ctx.lineWidth = 0.8;
  ctx.stroke();

  // 领口与组长工牌挂绳细节
  if (!facingBack) {
    ctx.fillStyle = "#f7f5ef";
    ctx.beginPath();
    ctx.moveTo(-2.1, -17.4);
    ctx.lineTo(0, -14.2);
    ctx.lineTo(2.1, -17.4);
    ctx.closePath();
    ctx.fill();
    if (isLeader) {
      ctx.strokeStyle = "#e8c468";
      ctx.lineWidth = 0.9;
      ctx.beginPath();
      ctx.moveTo(-1.6, -16.8);
      ctx.lineTo(0, -11.5);
      ctx.lineTo(1.6, -16.8);
      ctx.stroke();
      ctx.fillStyle = "#f5df9d";
      ctx.fillRect(-1.3, -11.8, 2.6, 3);
    }
  }

  // 4. 手臂与状态道具（打字、思考手稿、讨论手势、手冲咖啡）
  const armWave = status === "talking" && motion ? (tick % 2 === 0 ? -2.2 : 0.8) : 0;
  const typeShift = status === "working" && motion ? (tick % 2 === 0 ? -0.9 : 0.9) : 0;

  ctx.strokeStyle = jacketDark;
  ctx.lineWidth = 2.4;
  ctx.beginPath();
  ctx.moveTo(-4.6, -15.8);
  ctx.lineTo(-6.2 + (status === "working" ? 1.8 * dirX : 0), -10.4 + typeShift);
  ctx.moveTo(4.6, -15.8);
  ctx.lineTo(6.2 + (status === "talking" ? 1.2 * dirX : 0), -10.4 + armWave - typeShift);
  ctx.stroke();

  // 手部肤色圆点
  ctx.fillStyle = look.skin;
  ctx.beginPath();
  ctx.arc(-6.2 + (status === "working" ? 1.8 * dirX : 0), -10.1 + typeShift, 1.3, 0, Math.PI * 2);
  ctx.arc(6.2 + (status === "talking" ? 1.2 * dirX : 0), -10.1 + armWave - typeShift, 1.3, 0, Math.PI * 2);
  ctx.fill();

  if (status === "thinking" && !moving) {
    // 夹板手稿本
    ctx.fillStyle = "#f7f1df";
    ctx.strokeStyle = "#8a7960";
    ctx.lineWidth = 0.7;
    ctx.beginPath();
    ctx.roundRect(3.2 * dirX - 2.5, -13.8, 5.2, 6.2, 1);
    ctx.fill();
    ctx.stroke();
  } else if (status === "idle" && !moving && motion) {
    const act = idleAct(id, tick * 250);
    if (act === "coffee") {
      ctx.fillStyle = "#fffaf0";
      ctx.strokeStyle = "#8b6d52";
      ctx.lineWidth = 0.7;
      ctx.beginPath();
      ctx.roundRect(5.2 * dirX - 1.6, -12.6, 3.4, 4.2, 0.8);
      ctx.fill();
      ctx.stroke();
    } else if (act === "phone") {
      ctx.fillStyle = "#2c3539";
      ctx.beginPath();
      ctx.roundRect(5.0 * dirX - 1.4, -12.8, 2.8, 4.4, 0.7);
      ctx.fill();
      ctx.fillStyle = "#9fd8cb";
      ctx.fillRect(5.0 * dirX - 1.0, -12.2, 2.0, 3.0);
    }
  }

  // 5. 立体头部与发型配饰
  ctx.beginPath();
  ctx.arc(0, -22.5, 5.3, 0, Math.PI * 2);
  ctx.fillStyle = look.skin;
  ctx.fill();
  ctx.strokeStyle = "rgba(40, 32, 26, 0.2)";
  ctx.lineWidth = 0.7;
  ctx.stroke();

  if (facingBack) {
    // 背向时完整后脑头发包裹
    ctx.beginPath();
    ctx.arc(0, -22.8, 5.5, Math.PI * 0.85, Math.PI * 2.15);
    ctx.quadraticCurveTo(5.4, -19.2, 0, -19.0);
    ctx.quadraticCurveTo(-5.4, -19.2, -5.0, -21.5);
    ctx.closePath();
    ctx.fillStyle = look.main;
    ctx.fill();
  } else {
    // 3/4 正向柔和五官与刘海
    const eyeOffsetX = dirX * 1.4;
    ctx.fillStyle = "#2b2927";
    ctx.beginPath();
    ctx.arc(-1.7 + eyeOffsetX, -22.0, 0.65, 0, Math.PI * 2);
    ctx.arc(1.9 + eyeOffsetX, -22.0, 0.65, 0, Math.PI * 2);
    ctx.fill();

    // 温和腮红
    ctx.fillStyle = "rgba(222, 138, 118, 0.38)";
    ctx.beginPath();
    ctx.arc(-2.8 + eyeOffsetX * 0.7, -20.6, 0.9, 0, Math.PI * 2);
    ctx.arc(3.0 + eyeOffsetX * 0.7, -20.6, 0.9, 0, Math.PI * 2);
    ctx.fill();

    if (status === "talking" && motion && tick % 2 === 0) {
      ctx.fillStyle = "#7a443c";
      ctx.beginPath();
      ctx.ellipse(0.3 + eyeOffsetX, -19.8, 1.0, 0.75, 0, 0, Math.PI * 2);
      ctx.fill();
    }

    // 顶部与侧边发型轮廓
    ctx.fillStyle = look.main;
    ctx.beginPath();
    ctx.arc(0, -23.6, 5.4, Math.PI * 0.95, Math.PI * 2.05);
    ctx.quadraticCurveTo(5.2 * -dirX, -21.0, -4.6 * dirX, -23.2);
    ctx.closePath();
    ctx.fill();

    // 发丝高光
    ctx.strokeStyle = look.highlight;
    ctx.lineWidth = 1.1;
    ctx.beginPath();
    ctx.arc(-0.8, -25.2, 2.8, Math.PI * 1.15, Math.PI * 1.75);
    ctx.stroke();

    if (look.style === "glasses") {
      ctx.strokeStyle = "#47433e";
      ctx.lineWidth = 0.75;
      ctx.beginPath();
      ctx.arc(-1.7 + eyeOffsetX, -22.0, 1.35, 0, Math.PI * 2);
      ctx.moveTo(-0.35 + eyeOffsetX, -22.0);
      ctx.lineTo(0.55 + eyeOffsetX, -22.0);
      ctx.arc(1.9 + eyeOffsetX, -22.0, 1.35, 0, Math.PI * 2);
      ctx.stroke();
    }
  }

  // 发髻或头戴式监听耳机等工作室特征配饰
  if (look.style === "bun") {
    ctx.fillStyle = look.main;
    ctx.beginPath();
    ctx.arc(-dirX * 2.2, -28.4, 2.3, 0, Math.PI * 2);
    ctx.fill();
  } else if (look.style === "headphones") {
    ctx.strokeStyle = "#4b5952";
    ctx.lineWidth = 1.3;
    ctx.beginPath();
    ctx.arc(0, -23.2, 5.8, Math.PI * 1.08, Math.PI * 1.92);
    ctx.stroke();
    ctx.fillStyle = "#5c7065";
    ctx.beginPath();
    ctx.ellipse(-5.5, -22.4, 1.3, 2.1, 0, 0, Math.PI * 2);
    ctx.ellipse(5.5, -22.4, 1.3, 2.1, 0, 0, Math.PI * 2);
    ctx.fill();
  }

  ctx.restore();
}
