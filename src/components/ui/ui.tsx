import type { ButtonHTMLAttributes, ReactNode } from "react";
import { Icon } from "./Icon";

export const inputClass =
  "ui-input w-full min-w-0 rounded-lg border border-line-strong bg-surface px-3 py-1.5 text-sm leading-5 text-ink hover:border-[#9e9e96] focus:border-accent disabled:cursor-not-allowed disabled:bg-surface-subtle disabled:text-ink-muted";

type ButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: "primary" | "ghost" | "danger";
};

const BUTTON_STYLES = {
  primary:
    "border border-transparent bg-accent text-white shadow-[0_1px_2px_rgba(24,25,23,0.08)] enabled:hover:bg-accent-hover enabled:active:scale-[0.98]",
  ghost:
    "border border-line-strong bg-surface text-ink enabled:hover:border-[#a8a8a0] enabled:hover:bg-surface-subtle enabled:active:scale-[0.98]",
  danger:
    "border border-danger-border bg-surface text-danger enabled:hover:bg-danger-soft enabled:hover:border-[#f4b4ae] enabled:active:scale-[0.98]",
};

/** 与 Button 同一套外观；链接跳转（如去设置页）复用，避免复制样式。 */
export function buttonClass(variant: keyof typeof BUTTON_STYLES = "ghost", className = "") {
  return `inline-flex min-h-9 shrink-0 items-center justify-center gap-2 rounded-lg px-3.5 py-1.5 text-[13px] font-medium leading-5 ${BUTTON_STYLES[variant]} ${className}`;
}

export function Button({ variant = "ghost", className = "", ...rest }: ButtonProps) {
  return (
    <button
      type="button"
      className={`${buttonClass(variant, className)} disabled:cursor-not-allowed disabled:opacity-40`}
      {...rest}
    />
  );
}

export function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: ReactNode;
  children: ReactNode;
}) {
  return (
    <label className="block space-y-1.5">
      <span className="block text-[13px] font-medium text-ink-secondary">{label}</span>
      {children}
      {hint && <span className="block text-xs leading-5 text-ink-muted">{hint}</span>}
    </label>
  );
}

export function Modal({
  title,
  onClose,
  children,
}: {
  title: string;
  onClose: () => void;
  children: ReactNode;
}) {
  return (
    <div className="fixed inset-0 z-20 flex items-center justify-center bg-ink/40 p-6 backdrop-blur-[1px]">
      <div
        role="dialog"
        aria-modal="true"
        aria-label={title}
        className="flex max-h-full w-full max-w-2xl flex-col rounded-2xl border border-line bg-surface shadow-[0_20px_48px_rgba(24,25,23,0.18)]"
      >
        <div className="flex items-center justify-between border-b border-line px-5 py-3.5">
          <h2 className="text-[15px] font-medium text-ink">{title}</h2>
          <button
            type="button"
            aria-label="关闭"
            className="grid size-8 place-items-center rounded-lg text-ink-muted hover:bg-surface-subtle hover:text-ink"
            onClick={onClose}
          >
            <Icon name="close" width="16" height="16" />
          </button>
        </div>
        <div className="overflow-y-auto p-5">{children}</div>
      </div>
    </div>
  );
}
