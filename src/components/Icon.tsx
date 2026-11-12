import type { SVGProps } from "react";

const paths = {
  projects: "M3 7h7l2 2h9v11H3z M3 7V4h7l2 3",
  settings: "M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8 M9 3h6l1 3 3 1 2 5-2 5-3 1-1 3H9l-1-3-3-1-2-5 2-5 3-1z",
  plus: "M12 5v14 M5 12h14",
  arrow: "M5 12h14 M14 7l5 5-5 5",
  back: "M19 12H5 M10 7l-5 5 5 5",
  chat: "M4 4h16v12H9l-5 4z M8 8h8 M8 12h5",
  files: "M6 3h8l4 4v14H6z M14 3v5h5 M9 12h6 M9 16h6",
  rules: "M5 4h14v17l-7-3-7 3z M9 8h6 M9 12h6",
  team: "M9 12a4 4 0 1 0 0-8 4 4 0 0 0 0 8 M2 21v-3c0-3 14-3 14 0v3 M17 5a3 3 0 0 1 0 6 M19 15c3 0 3 2 3 5",
  connection: "M8 4v5 M16 4v5 M6 9h12v3a6 6 0 0 1-12 0z M12 18v4",
  close: "M6 6l12 12 M18 6 6 18",
  grid: "M3 3h7v7H3z M14 3h7v7h-7z M3 14h7v7H3z M14 14h7v7h-7z",
} as const;

export type IconName = keyof typeof paths;

/** 界面图标使用同一套线宽；CLI 的品牌图标独立保留。 */
export function Icon({ name, ...props }: SVGProps<SVGSVGElement> & { name: IconName }) {
  return <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" {...props}><path d={paths[name]} /></svg>;
}
