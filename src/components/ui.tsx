import type { ButtonHTMLAttributes, ReactNode } from "react";

export const inputClass =
  "w-full rounded border border-slate-700 bg-slate-900 px-3 py-2 text-sm text-slate-100 outline-none focus:border-sky-500";

type ButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: "primary" | "ghost" | "danger";
};

const BUTTON_STYLES = {
  primary: "bg-sky-600 text-white hover:bg-sky-500",
  ghost: "border border-slate-700 text-slate-200 hover:bg-slate-800",
  danger: "border border-red-800 text-red-300 hover:bg-red-950",
};

export function Button({ variant = "ghost", className = "", ...rest }: ButtonProps) {
  return (
    <button
      type="button"
      className={`rounded px-3 py-1.5 text-sm disabled:cursor-not-allowed disabled:opacity-40 ${BUTTON_STYLES[variant]} ${className}`}
      {...rest}
    />
  );
}

export function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <label className="block space-y-1">
      <span className="text-xs text-slate-400">{label}</span>
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
      <div className="flex max-h-full w-full max-w-2xl flex-col rounded-lg border border-slate-700 bg-slate-900">
        <div className="flex items-center justify-between border-b border-slate-800 px-4 py-3">
          <h2 className="text-sm font-semibold">{title}</h2>
          <button
            type="button"
            aria-label="关闭"
            className="text-slate-400 hover:text-slate-100"
            onClick={onClose}
          >
            ✕
          </button>
        </div>
        <div className="overflow-y-auto p-4">{children}</div>
      </div>
    </div>
  );
}
