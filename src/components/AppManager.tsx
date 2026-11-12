import { useCallback, useEffect, useState } from "react";
import { api } from "../api";
import { groupClis, toggleState, toInput } from "../lib/cliConfig";
import type { CliConfig } from "../types";
import { CliDetail } from "./CliDetail";
import { Switch } from "./Switch";

/** 系统设置 → 应用管理：列出预设 CLI，每个有开关，点进去是详情 */
export function AppManager() {
  const [configs, setConfigs] = useState<CliConfig[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [openId, setOpenId] = useState<string | null>(null);
  const [busyId, setBusyId] = useState<string | null>(null);

  const load = useCallback(async () => {
    try { setConfigs(await api.listCliConfigs()); setError(null); }
    catch (failure) { setError(String(failure)); }
  }, []);
  useEffect(() => { void load(); }, [load]);

  function replace(saved: CliConfig) {
    setConfigs((current) => current?.map((c) => (c.id === saved.id ? saved : c)) ?? current);
  }

  async function toggle(config: CliConfig, enabled: boolean) {
    setBusyId(config.id); setError(null);
    try { replace(await api.saveCliConfig(config.id, toInput(config, { enabled }))); }
    catch (failure) { setError(String(failure)); }
    finally { setBusyId(null); }
  }

  const opened = configs?.find((c) => c.id === openId);
  if (opened) {
    return <CliDetail config={opened} onBack={() => setOpenId(null)} onSaved={replace} onRefresh={load} />;
  }
  if (!configs) {
    return error
      ? <p role="alert" className="text-sm text-red-300">{error}</p>
      : <p className="text-sm text-slate-400">正在检测本机已安装的 CLI…</p>;
  }

  const groups = groupClis(configs);
  const section = (title: string, items: CliConfig[]) => items.length > 0 && (
    <section className="mb-6">
      <h3 className="mb-2 text-sm text-slate-400">{title} · {items.length}</h3>
      <ul className="divide-y divide-slate-800 rounded-lg border border-slate-800 bg-slate-900/60">
        {items.map((config) => <Row key={config.id} config={config} busy={busyId === config.id} onOpen={() => setOpenId(config.id)} onToggle={(next) => void toggle(config, next)} />)}
      </ul>
    </section>
  );
  return (
    <div>
      <p className="mb-4 text-sm text-slate-400">开关决定该 CLI 能否被新选：关闭后不再出现在成员表单，内置助理也不会自动选用；已有成员不受影响。</p>
      {error && <p role="alert" className="mb-3 text-sm text-red-300">{error}</p>}
      {section("已安装", groups.installed)}
      {section("未安装", groups.missing)}
    </div>
  );
}

function Row({ config, busy, onOpen, onToggle }: { config: CliConfig; busy: boolean; onOpen: () => void; onToggle: (next: boolean) => void }) {
  const state = toggleState(config);
  return (
    <li className={`flex items-center gap-3 px-4 py-3 ${config.supported ? "" : "opacity-60"}`}>
      <button type="button" disabled={!config.supported} onClick={onOpen}
        className="flex min-w-0 flex-1 items-center gap-3 text-left disabled:cursor-not-allowed">
        <span className="grid h-9 w-9 shrink-0 place-items-center rounded bg-slate-800 text-sm font-semibold">{config.label.slice(0, 1)}</span>
        <span className="min-w-0">
          <span className="block truncate font-medium">{config.label}</span>
          <span className="block truncate text-xs text-slate-500" title={config.path ?? undefined}>{config.path ?? state.hint ?? "未检测到"}</span>
        </span>
      </button>
      <span className="w-32 shrink-0 text-right font-mono text-xs text-slate-400">{config.version ?? (config.installed ? "" : "未安装")}</span>
      <Switch checked={state.checked} disabled={state.disabled || busy} label={`${config.label} 开关`} onChange={onToggle} />
      {config.supported && <span aria-hidden className="shrink-0 text-slate-500">›</span>}
    </li>
  );
}
