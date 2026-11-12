import { useCallback, useEffect, useState } from "react";
import { api } from "../api";
import { groupClis, toggleState, toInput } from "../lib/cliConfig";
import type { CliConfig } from "../types";
import { CliDetail } from "./CliDetail";
import { CliIcon } from "./CliIcon";
import { Switch } from "./Switch";
import { Button } from "./ui";

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

  function openDetail(id: string) { setError(null); setOpenId(id); }
  function closeDetail() { setError(null); setOpenId(null); }

  const opened = configs?.find((c) => c.id === openId);
  if (opened) {
    return (
      <>
        {error && <p role="alert" className="mb-3 text-sm text-red-300">{error}</p>}
        <CliDetail config={opened} onBack={closeDetail} onSaved={replace} onRefresh={load} />
      </>
    );
  }
  if (!configs) {
    return error
      ? (
        <div className="flex items-center gap-3">
          <p role="alert" className="text-sm text-red-300">{error}</p>
          <Button onClick={() => void load()}>重试</Button>
        </div>
      )
      : <p className="text-sm text-slate-400">正在检测本机已安装的 CLI…</p>;
  }

  const groups = groupClis(configs);
  const section = (title: string, items: CliConfig[]) => items.length > 0 && (
    <section className="mb-8">
      <h3 className="mb-3 flex items-center gap-2 text-sm font-medium text-slate-400">{title}<span className="font-mono text-xs tabular-nums text-slate-500">{items.length}</span></h3>
      <ul className={title === "暂未支持" ? "studio-app-unsupported" : "studio-app-grid"}>
        {items.map((config) => <Row key={config.id} config={config} busy={busyId === config.id} onOpen={() => openDetail(config.id)} onToggle={(next) => void toggle(config, next)} />)}
      </ul>
    </section>
  );
  return (
    <div>
      <p className="mb-6 max-w-prose text-sm leading-6 text-slate-400">启用后，可为新成员和内置助理选用该 CLI。关闭不影响已有成员。</p>
      {error && <p role="alert" className="mb-3 text-sm text-red-300">{error}</p>}
      {section("已安装", groups.installed)}
      {section("未安装", groups.missing)}
      {section("暂未支持", groups.unsupported)}
    </div>
  );
}

function Row({ config, busy, onOpen, onToggle }: { config: CliConfig; busy: boolean; onOpen: () => void; onToggle: (next: boolean) => void }) {
  const state = toggleState(config);
  return (
    <li className={`flex items-center gap-3 px-4 ${config.supported ? "min-h-18 py-4 hover:bg-slate-800/40" : "min-h-12 py-2"}`}>
      <button type="button" disabled={!config.supported} onClick={onOpen}
        className="flex min-w-0 flex-1 items-center gap-3 rounded-lg text-left disabled:cursor-not-allowed">
        <CliIcon id={config.id} label={config.label} className={config.supported ? "" : "!h-8 !w-8 [&_img]:size-5"} />
        <span className="min-w-0">
          <span className={`block truncate font-medium ${config.supported ? "text-slate-100" : "text-slate-400"}`}>{config.label}</span>
          {config.supported && <span className="mt-1 block truncate text-xs text-slate-500" title={config.path ?? undefined}>{config.path ?? state.hint ?? "未检测到"}</span>}
        </span>
      </button>
      {config.supported && <span className="hidden shrink-0 text-right font-mono text-[11px] tabular-nums text-slate-500 xl:block">{config.version ?? (config.installed ? "" : "未安装")}</span>}
      {config.supported
        ? <Switch checked={state.checked} disabled={state.disabled || busy} label={`${config.label} 开关`} onChange={onToggle} />
        : <span aria-hidden className="h-6 w-11 shrink-0" />}
      {config.supported && <span aria-hidden className="shrink-0 text-slate-500">›</span>}
    </li>
  );
}
