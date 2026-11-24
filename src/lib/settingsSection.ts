/** 设置页的分类；支持从 HashRouter 的 ?tab= 直接落到某一类。 */
export type SettingsSection = "apps" | "runtime" | "proxy";

const SECTIONS: SettingsSection[] = ["apps", "runtime", "proxy"];

/** 无法识别或没有参数时回到第一类，避免深链把页面顶到空白状态。 */
export function settingsSectionFromParam(value: string | null | undefined): SettingsSection {
  return SECTIONS.includes(value as SettingsSection) ? value as SettingsSection : "apps";
}
