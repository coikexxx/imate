interface Props { checked: boolean; disabled?: boolean; label: string; onChange: (next: boolean) => void }

/** 与 CC Switch 类似的开关。无障碍上是 role="switch" 的按钮 */
export function Switch({ checked, disabled = false, label, onChange }: Props) {
  return (
    <button type="button" role="switch" aria-checked={checked} aria-label={label} disabled={disabled}
      onClick={() => onChange(!checked)}
      className={`relative h-6 w-11 shrink-0 rounded-full transition-colors duration-[140ms] ease-interface disabled:cursor-not-allowed disabled:opacity-40 ${checked ? "bg-sky-600" : "bg-slate-700"}`}>
      <span className={`absolute left-0.5 top-0.5 h-5 w-5 rounded-full bg-white transition-transform duration-[140ms] ease-interface ${checked ? "translate-x-5" : "translate-x-0"}`} />
    </button>
  );
}
