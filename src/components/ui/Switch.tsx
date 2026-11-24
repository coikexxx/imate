interface Props { checked: boolean; disabled?: boolean; label: string; onChange: (next: boolean) => void }

/** 无障碍 role="switch" 开关，采用 transform 位移保证流畅度。 */
export function Switch({ checked, disabled = false, label, onChange }: Props) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={`relative h-5 w-9 shrink-0 rounded-full transition-colors duration-[140ms] ease-interface disabled:cursor-not-allowed disabled:opacity-40 ${
        checked ? "bg-accent" : "bg-line-strong"
      }`}
    >
      <span
        className={`absolute left-0.5 top-0.5 h-4 w-4 rounded-full bg-white shadow-[0_1px_2px_rgba(0,0,0,0.18)] transition-transform duration-[140ms] ease-interface ${
          checked ? "translate-x-4" : "translate-x-0"
        }`}
      />
    </button>
  );
}
