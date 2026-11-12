import backgroundUrl from "../assets/openoffice/background.jpeg";
import char0 from "../assets/openoffice/characters/char_0.png";
import char1 from "../assets/openoffice/characters/char_1.png";
import char2 from "../assets/openoffice/characters/char_2.png";
import char3 from "../assets/openoffice/characters/char_3.png";
import char4 from "../assets/openoffice/characters/char_4.png";
import char5 from "../assets/openoffice/characters/char_5.png";

export interface OfficeAssets { background: HTMLImageElement; characters: HTMLImageElement[] }

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
    cached = Promise.all([
      loadImage(backgroundUrl, 1024, 1024),
      Promise.all([char0, char1, char2, char3, char4, char5].map((url) => loadImage(url, 112, 96))),
    ]).then(([background, characters]) => ({ background, characters })).catch((error: unknown) => {
      cached = null;
      throw error;
    });
  }
  return cached;
}
