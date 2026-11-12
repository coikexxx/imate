import { useEffect, useId, useLayoutEffect, useRef, useState, type CSSProperties, type KeyboardEvent } from "react";
import { createPortal } from "react-dom";

export interface SelectOption { value: string; label: string; disabled?: boolean }
interface Props {
  value: string;
  options: SelectOption[];
  onChange: (value: string) => void;
  disabled?: boolean;
  placeholder?: string;
  "aria-label"?: string;
  size?: "default" | "compact";
}

/** 自绘菜单通过 portal 脱离滚动面板，保留选择、禁用及键盘操作语义。 */
export function Select({ value, options, onChange, disabled = false, placeholder = "请选择", "aria-label": label, size = "default" }: Props) {
  const id = useId();
  const trigger = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(-1);
  const [position, setPosition] = useState<CSSProperties>({});
  const search = useRef({ text: "", time: 0 });
  const selected = options.find((option) => option.value === value);
  const enabled = options.map((option, index) => option.disabled ? -1 : index).filter((index) => index >= 0);
  const isOpen = open && !disabled;

  function close(restoreFocus = false) {
    setOpen(false);
    if (restoreFocus) trigger.current?.focus();
  }
  function show(last = false) {
    if (disabled) return;
    const selectedIndex = options.findIndex((option) => option.value === value && !option.disabled);
    setActive(selectedIndex >= 0 ? selectedIndex : (last ? enabled[enabled.length - 1] : enabled[0]) ?? -1);
    setOpen(true);
  }
  function choose(index: number) {
    const option = options[index];
    if (!option || option.disabled) return;
    onChange(option.value);
    close(true);
  }
  function move(direction: number) {
    const current = enabled.indexOf(active);
    const next = current < 0 ? (direction > 0 ? 0 : enabled.length - 1) : (current + direction + enabled.length) % enabled.length;
    setActive(enabled[next] ?? -1);
  }
  function keyDown(event: KeyboardEvent<HTMLButtonElement>) {
    if (event.key === "Tab") { close(); return; }
    if (event.key === "Escape") { if (isOpen) { event.preventDefault(); event.stopPropagation(); close(true); } return; }
    if (["ArrowDown", "ArrowUp", "Home", "End", "Enter", " "].includes(event.key)) {
      event.preventDefault();
      if (!isOpen) { show(event.key === "ArrowUp" || event.key === "End"); return; }
      if (event.key === "ArrowDown") move(1);
      else if (event.key === "ArrowUp") move(-1);
      else if (event.key === "Home") setActive(enabled[0] ?? -1);
      else if (event.key === "End") setActive(enabled[enabled.length - 1] ?? -1);
      else choose(active);
      return;
    }
    if (event.key.length === 1 && !event.ctrlKey && !event.metaKey && !event.altKey) {
      const now = Date.now();
      search.current = { text: (now - search.current.time < 700 ? search.current.text : "") + event.key.toLocaleLowerCase(), time: now };
      const match = options.findIndex((option) => !option.disabled && option.label.toLocaleLowerCase().startsWith(search.current.text));
      if (match >= 0) { setActive(match); setOpen(true); }
    }
  }

  useLayoutEffect(() => {
    if (!isOpen) return;
    function place() {
      const rect = trigger.current?.getBoundingClientRect();
      if (!rect) return;
      const below = window.innerHeight - rect.bottom - 12;
      const above = rect.top - 12;
      const upward = below < 220 && above > below;
      setPosition({ position: "fixed", left: Math.max(8, Math.min(rect.left, window.innerWidth - rect.width - 8)), width: Math.min(rect.width, window.innerWidth - 16), maxHeight: Math.max(80, Math.min(300, upward ? above : below)), ...(upward ? { bottom: window.innerHeight - rect.top + 6 } : { top: rect.bottom + 6 }) });
    }
    place();
    const observer = new ResizeObserver(place);
    if (trigger.current) observer.observe(trigger.current);
    window.addEventListener("resize", place);
    window.addEventListener("scroll", place, true);
    return () => { observer.disconnect(); window.removeEventListener("resize", place); window.removeEventListener("scroll", place, true); };
  }, [isOpen]);

  useEffect(() => {
    if (!isOpen) return;
    function outside(event: PointerEvent) {
      if (!trigger.current?.contains(event.target as Node) && !menu.current?.contains(event.target as Node)) close();
    }
    document.addEventListener("pointerdown", outside);
    return () => document.removeEventListener("pointerdown", outside);
  }, [isOpen]);

  useEffect(() => {
    if (isOpen && active >= 0) menu.current?.querySelector(`[data-index="${active}"]`)?.scrollIntoView({ block: "nearest" });
  }, [isOpen, active]);

  return <>
    <button ref={trigger} type="button" role="combobox" aria-label={label} aria-expanded={isOpen} aria-haspopup="listbox" aria-controls={isOpen ? id : undefined} aria-activedescendant={isOpen && active >= 0 ? `${id}-${active}` : undefined} disabled={disabled} className={`studio-select ${size === "compact" ? "studio-select-compact" : ""} ${isOpen ? "is-open" : ""}`} onClick={() => isOpen ? close() : show()} onKeyDown={keyDown} onBlur={() => close()}>
      <span className={`studio-select-value ${selected ? "" : "is-placeholder"}`} title={selected?.label}>{selected?.label ?? placeholder}</span>
      <svg className="studio-select-chevron" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" aria-hidden="true"><path d="m7 10 5 5 5-5" /></svg>
    </button>
    {isOpen && createPortal(<div ref={menu} id={id} role="listbox" aria-label={label ?? "可选项"} className="studio-select-menu" style={position} onPointerDown={(event) => event.preventDefault()} onClick={(event) => { event.preventDefault(); event.stopPropagation(); }}>
      {options.length === 0 && <p className="studio-select-empty">暂无可选项</p>}
      {options.map((option, index) => <div key={option.value} id={`${id}-${index}`} role="option" aria-selected={option.value === value} aria-disabled={option.disabled || undefined} data-index={index} data-active={active === index} className="studio-select-option" onPointerMove={() => { if (!option.disabled) setActive(index); }} onClick={() => choose(index)}>
        <span className="studio-select-option-label">{option.label}</span><span className="studio-select-check">{option.value === value && <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" aria-hidden="true"><path d="m5 12 4 4L19 6" /></svg>}</span>
      </div>)}
    </div>, document.body)}
  </>;
}
