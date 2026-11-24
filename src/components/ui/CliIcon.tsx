import { cliIconInfo } from "../../lib/cliIcons";

interface Props {
  id: string;
  /** 没有图标时取它的首字母做徽标 */
  label: string;
  className?: string;
}

/** CLI 的 logo。装饰性图片：名称已经写在旁边的文字里，所以不给 alt；Pi 没有对应 logo，用「π」 */
export function CliIcon({ id, label, className = "" }: Props) {
  const icon = cliIconInfo(id);
  const box = `grid h-9 w-9 shrink-0 place-items-center rounded-lg border border-line bg-surface ${className}`;
  if (icon) {
    return (
      <span className={box}>
        <img src={icon.src} alt="" aria-hidden="true" className="h-5 w-5" />
      </span>
    );
  }
  return <span className={`${box} text-sm font-medium text-ink-secondary`}>{id === "pi" ? "π" : label.slice(0, 1)}</span>;
}
