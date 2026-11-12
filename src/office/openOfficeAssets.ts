import backgroundUrl from "../assets/gba-office/background.png";
import { createGbaCharacters } from "./gbaCharacters";

export interface OfficeAssets { background: HTMLImageElement; characters: HTMLCanvasElement[] }

function loadImage(url: string, width: number, height: number): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const image = new Image();
    image.onload = () => image.naturalWidth === width && image.naturalHeight === height
      ? resolve(image) : reject(new Error("办公室素材尺寸不匹配"));
    image.onerror = () => reject(new Error("办公室素材加载失败"));
    image.src = url;
  });
}

let cached: Promise<OfficeAssets> | null = null;

/** 两个画布共享离线图片；失败后清空缓存，允许用户重新加载。 */
export function loadOfficeAssets(): Promise<OfficeAssets> {
  if (!cached) {
    cached = loadImage(backgroundUrl, 512, 512)
      .then((background) => ({ background, characters: createGbaCharacters() })).catch((error: unknown) => {
      cached = null;
      throw error;
    });
  }
  return cached;
}
