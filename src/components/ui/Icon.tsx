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
  info: "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18 M12 11v5 M12 8h.01",
  grid: "M3 3h7v7H3z M14 3h7v7h-7z M3 14h7v7H3z M14 14h7v7h-7z",
  usage: "M4 20h16 M7 16V10 M12 16V5 M17 16v-3",
  trajectory: "M7.5 6a1.5 1.5 0 1 0-3 0 1.5 1.5 0 0 0 3 0 M7.5 12a1.5 1.5 0 1 0-3 0 1.5 1.5 0 0 0 3 0 M7.5 18a1.5 1.5 0 1 0-3 0 1.5 1.5 0 0 0 3 0 M6 7.5v3 M6 13.5v3 M10 6h7 M10 12h5 M10 18h8",
  folder: "M3 6.5A1.5 1.5 0 0 1 4.5 5h4.3l2 2H19.5A1.5 1.5 0 0 1 21 8.5v9a1.5 1.5 0 0 1-1.5 1.5h-15A1.5 1.5 0 0 1 3 17.5z",
  folderOpen: "M3 6.5A1.5 1.5 0 0 1 4.5 5h4.3l2 2H19.5A1.5 1.5 0 0 1 21 8.5V10H6.2L3 18V6.5z M6.2 10H21l-2.4 8.5a1.5 1.5 0 0 1-1.4.5H3z",
  file: "M6 3.5h7.5L18 8v12.5H6z M13.5 3.5V8H18",
  chevronRight: "m9 6 6 6-6 6",
  chevronDown: "m6 9 6 6 6-6",
  check: "m5 12 4.5 4.5L19 7",
  at: "M16 12a4 4 0 1 1-8 0 4 4 0 0 1 8 0Zm0 0v1.5a2.5 2.5 0 0 0 5 0V12a9 9 0 1 0-5.5 8.3",
  refresh: "M20 11a8 8 0 1 0 2 5.3 M20 4v7h-7",
} as const;

export type IconName = keyof typeof paths;

/** 界面图标使用同一套 1.5px 线宽；CLI 的品牌图标独立保留。 */
export function Icon({ name, ...props }: SVGProps<SVGSVGElement> & { name: IconName }) {
  return (
    <svg
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      {...props}
    >
      <path d={paths[name]} />
    </svg>
  );
}
