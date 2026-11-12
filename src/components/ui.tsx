import type { ButtonHTMLAttributes, ReactNode } from "react";

export const inputClass =
  "ui-input w-full min-w-0 rounded-lg border border-slate-700 bg-slate-900 px-3 py-2 text-sm leading-5 text-slate-100 focus:border-sky-400 disabled:cursor-not-allowed disabled:opacity-50";

type ButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: "primary" | "ghost" | "danger";
};

const BUTTON_STYLES = {
  primary: "border border-transparent bg-sky-600 text-white enabled:hover:bg-sky-500",
  ghost: "border border-slate-700 bg-slate-900 text-slate-200 enabled:hover:border-slate-500 enabled:hover:bg-slate-800",
  danger: "border border-red-400/30 text-red-300 enabled:hover:bg-red-950/60",
};

export function Button({ variant = "ghost", className = "", ...rest }: ButtonProps) {
  return (
    <button
      type="button"
      className={`inline-flex min-h-9 shrink-0 items-center justify-center gap-2 rounded-lg px-3 py-1.5 text-sm font-medium leading-5 disabled:cursor-not-allowed disabled:opacity-40 ${BUTTON_STYLES[variant]} ${className}`}
      {...rest}
    />
  );
}

export function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <label className="block space-y-2">
      <span className="text-sm text-slate-400">{label}</span>
      {children}
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
    <div className="fixed inset-0 z-10 flex items-center justify-center bg-black/60 p-6">
      <div role="dialog" aria-modal="true" aria-label={title} className="flex max-h-full w-full max-w-2xl flex-col rounded-xl border border-slate-700 bg-slate-900">
        <div className="flex items-center justify-between border-b border-slate-800 px-4 py-3">
          <h2 className="text-sm font-semibold">{title}</h2>
          <button
            type="button"
            aria-label="关闭"
            className="grid size-8 place-items-center rounded-lg text-slate-400 hover:bg-slate-800 hover:text-slate-100"
            onClick={onClose}
          >
            ✕
          </button>
        </div>
        <div className="overflow-y-auto p-6">{children}</div>
      </div>
    </div>
  );
}
