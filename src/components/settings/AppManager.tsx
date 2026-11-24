import { useCallback, useEffect, useState } from "react";
import { api } from "../../api";
import { groupClis, toggleState, toInput } from "../../lib/cliConfig";
import type { CliConfig } from "../../types";
import { CliIcon } from "../ui/CliIcon";
import { Icon } from "../ui/Icon";
import { Switch } from "../ui/Switch";
import { Button } from "../ui/ui";
import { CliDetail } from "./CliDetail";

/** 系统设置 → 应用管理：列出预设 CLI，每个有开关，点进去是详情 */
export function AppManager() {
  const [configs, setConfigs] = useState<CliConfig[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [openId, setOpenId] = useState<string | null>(null);
  const [busyId, setBusyId] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      setConfigs(await api.listCliConfigs());
      setError(null);
    } catch (failure) {
      setError(String(failure));
    }
  }, []);
  useEffect(() => {
    void load();
  }, [load]);

  function replace(saved: CliConfig) {
    setConfigs((current) => current?.map((c) => (c.id === saved.id ? saved : c)) ?? current);
  }

  async function toggle(config: CliConfig, enabled: boolean) {
    setBusyId(config.id);
    setError(null);
    try {
      replace(await api.saveCliConfig(config.id, toInput(config, { enabled })));
    } catch (failure) {
      setError(String(failure));
    } finally {
      setBusyId(null);
    }
  }

  function openDetail(id: string) {
    setError(null);
    setOpenId(id);
  }
  function closeDetail() {
    setError(null);
    setOpenId(null);
  }

  const opened = configs?.find((c) => c.id === openId);
  if (opened) {
    return (
      <>
        {error && (
          <p role="alert" className="mb-3 rounded-lg border border-danger-border bg-danger-soft px-3.5 py-2 text-sm text-danger">
            {error}
          </p>
        )}
        <CliDetail config={opened} onBack={closeDetail} onSaved={replace} onRefresh={load} />
      </>
    );
  }
  if (!configs) {
    return error ? (
      <div className="flex items-center gap-3">
        <p role="alert" className="text-sm text-danger">{error}</p>
        <Button onClick={() => void load()}>重试</Button>
      </div>
    ) : (
      <p className="text-sm text-ink-secondary">正在检测本机已安装的 CLI…</p>
    );
  }

  const groups = groupClis(configs);
  const section = (title: string, items: CliConfig[]) =>
    items.length > 0 && (
      <section className="mb-7 last:mb-0">
        <h2 className="mb-3 flex items-center gap-2 text-xs font-medium text-ink-secondary">
          <span>{title}</span>
          <span className="rounded bg-surface-subtle px-1.5 py-0.5 font-mono text-[11px] tabular-nums text-ink-muted">
            {items.length}
          </span>
        </h2>
        <ul className={title === "暂未支持" ? "studio-app-unsupported" : "studio-app-grid"}>
          {items.map((config) => (
            <Row
              key={config.id}
              config={config}
              busy={busyId === config.id}
              onOpen={() => openDetail(config.id)}
              onToggle={(next) => void toggle(config, next)}
            />
          ))}
        </ul>
      </section>
    );

  return (
    <div>
      {error && (
        <p role="alert" className="mb-4 rounded-lg border border-danger-border bg-danger-soft px-3.5 py-2 text-sm text-danger">
          {error}
        </p>
      )}
      {section("已安装", groups.installed)}
      {section("未安装", groups.missing)}
      {section("暂未支持", groups.unsupported)}
    </div>
  );
}

function Row({
  config,
  busy,
  onOpen,
  onToggle,
}: {
  config: CliConfig;
  busy: boolean;
  onOpen: () => void;
  onToggle: (next: boolean) => void;
}) {
  const state = toggleState(config);
  return (
    <li className={`flex items-center gap-3 px-4 ${config.supported ? "min-h-16 py-3.5 hover:bg-paper" : "min-h-11 py-2"}`}>
      <button
        type="button"
        disabled={!config.supported}
        onClick={onOpen}
        className="flex min-w-0 flex-1 items-center gap-3 rounded-lg text-left disabled:cursor-not-allowed"
      >
        <CliIcon id={config.id} label={config.label} className={config.supported ? "" : "h-7 w-7 [&_img]:size-4"} />
        <span className="min-w-0 flex-1">
          <span className={`block truncate text-[13.5px] font-medium ${config.supported ? "text-ink" : "text-ink-secondary"}`}>
            {config.label}
          </span>
          {config.supported && (
            <span className="mt-0.5 block truncate font-mono text-[11px] text-ink-muted" title={config.path ?? undefined}>
              {config.path ?? state.hint ?? "未检测到"}
            </span>
          )}
        </span>
      </button>
      {config.supported && (
        <span className="hidden shrink-0 rounded bg-surface-subtle px-2 py-0.5 font-mono text-[11px] tabular-nums text-ink-secondary xl:block">
          {config.version ?? (config.installed ? "已安装" : "未安装")}
        </span>
      )}
      {config.supported && (
        <Switch checked={state.checked} disabled={state.disabled || busy} label={`${config.label} 开关`} onChange={onToggle} />
      )}
      {config.supported && (
        <button
          type="button"
          aria-hidden="true"
          tabIndex={-1}
          onClick={onOpen}
          className="shrink-0 text-ink-muted"
        >
          <Icon name="chevronRight" width="15" height="15" />
        </button>
      )}
    </li>
  );
}
