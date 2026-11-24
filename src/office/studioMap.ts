import layout from "../assets/studio-diorama/layout.json";

export type RegionId = "command" | "pods" | "hub" | "cafe" | "terrace";
export interface OfficeRegion {
  id: RegionId;
  name: string;
  col: number;
  row: number;
  cols: number;
  rows: number;
  color: string;
}
export interface StudioSeat {
  col: number;
  row: number;
  facing: "up" | "down" | "left" | "right";
}
export interface FurnitureItem {
  kind: string;
  col: number;
  row: number;
  cols: number;
  rows: number;
}

export const OFFICE_REGIONS = layout.regions as OfficeRegion[];
export const WORK_SEATS = layout.workSeats as StudioSeat[];
export const MEETING_SEATS = layout.meetingSeats as StudioSeat[];
export const REST_SEATS = layout.restSeats as StudioSeat[];
export const FURNITURE = layout.furniture as FurnitureItem[];
export const DOORWAYS = layout.doorways;

/** 寻路与等轴测建筑共用同一份 32×32 坐标；分区隔断与立体家具阻挡行走，开放门廊通行。 */
const tiles = Array.from({ length: layout.cols * layout.rows }, (_, index) => {
  const col = index % layout.cols;
  const row = Math.floor(index / layout.cols);
  return col > 0 && row > 0 && col < layout.cols - 1 && row < layout.rows - 1 ? 1 : 0;
});

const block = (col: number, row: number) => {
  tiles[row * layout.cols + col] = 0;
};

for (const region of OFFICE_REGIONS) {
  for (let col = region.col; col < region.col + region.cols; col += 1) {
    block(col, region.row);
    block(col, region.row + region.rows - 1);
  }
  for (let row = region.row; row < region.row + region.rows; row += 1) {
    block(region.col, row);
    block(region.col + region.cols - 1, row);
  }
}

for (const item of FURNITURE) {
  for (let row = item.row; row < item.row + item.rows; row += 1) {
    for (let col = item.col; col < item.col + item.cols; col += 1) {
      block(col, row);
    }
  }
}

for (const door of DOORWAYS) {
  tiles[door.row * layout.cols + door.col] = 1;
}

export const STUDIO_ROOM = { cols: layout.cols, rows: layout.rows, cellSize: layout.cellSize, tiles };
