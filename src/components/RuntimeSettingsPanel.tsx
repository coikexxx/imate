import { useEffect, useReducer, useState } from "react";
import { api } from "../api";
import {
  assistantNotice, assistantOptions, DEFAULT_LIMITS, fromForm, initialPanel, isDirty, LIMIT_FIELDS, panelReducer, toForm,
} from "../lib/runtimeSettingsForm";
import type { CliConfig, CliInfo } from "../types";
import { Select } from "./Select";
import { Button, Field, inputClass } from "./ui";

/** 系统设置 → 运行设置：内置助理所用 CLI 与成本护栏。 */
export function RuntimeSettingsPanel() {
  const [state, dispatch] = useReducer(panelReducer, initialPanel);
  const [clis, setClis] = useState<CliInfo[]>([]);
  const [configs, setConfigs] = useState<CliConfig[] | null>(null);
  const [reloadTick, setReloadTick] = useState(0);

  useEffect(() => {
    let disposed = false;
    api.getRuntimeSettings()
      .then((settings) => { if (!disposed) dispatch({ type: "loaded", settings }); })
      .catch((failure) => { if (!disposed) dispatch({ type: "loadFailed", error: String(failure) }); });
    // 应用管理的配置已含安装状态；读不到时退回只探测安装状态。
    api.listCliConfigs()
      .then((list) => { if (!disposed) { setConfigs(list); setClis(list); } })
      .catch(() => api.listClis().then((list) => { if (!disposed) { setConfigs(null); setClis(list); } }).catch(() => {}));
    return () => { disposed = true; };
  }, [reloadTick]);

  async function save() {
    if (state.phase !== "ready" || state.saving) return;
    const parsed = fromForm(state.form);
    if (!parsed.ok) { dispatch({ type: "saveFailed", error: parsed.error }); return; }
    dispatch({ type: "saveStarted" });
    try { dispatch({ type: "saved", settings: await api.saveRuntimeSettings(parsed.settings) }); }
    catch (failure) { dispatch({ type: "saveFailed", error: String(failure) }); }
  }

  async function resetCorrupted() {
    try { dispatch({ type: "loaded", settings: await api.saveRuntimeSettings({ assistantCli: null, limits: DEFAULT_LIMITS }) }); }
    catch (failure) { dispatch({ type: "loadFailed", error: String(failure) }); }
  }

  if (state.phase === "loading") return <p className="text-sm text-slate-400">正在读取运行设置…</p>;
  if (state.phase === "load_failed") {
    return <div className="space-y-3">
      <p role="alert" className="text-sm text-red-300">{state.error}</p>
      <div className="flex flex-wrap gap-2">
        <Button onClick={() => setReloadTick((n) => n + 1)}>重试</Button>
        <Button variant="danger" onClick={() => void resetCorrupted()}>用默认值重新保存</Button>
      </div>
      <p className="text-xs text-slate-500">重新保存会把助理改回自动选择、护栏改回手册默认值；进行中的指令不受影响。</p>
    </div>;
  }

  const { form, saved, saving, error, savedTick } = state;
  const dirty = isDirty(saved, form);
  const notice = assistantNotice(clis, configs, form.assistantCli);
  const edit = (patch: Partial<typeof form>) => dispatch({ type: "edit", form: { ...form, ...patch } });

  return <div className="space-y-8">
    <section className="space-y-3">
      <h3 className="text-sm font-medium">内置助理</h3>
      <p className="text-sm text-slate-400">起草规章和人设时使用的 CLI。保存后从下一次起草生效，正在进行的起草不受影响。</p>
      <Field label="助理使用的 CLI">
        <Select aria-label="助理使用的 CLI" disabled={saving} value={form.assistantCli}
          options={assistantOptions(clis, configs, form.assistantCli)} onChange={(assistantCli) => edit({ assistantCli })} />
      </Field>
      <p role={notice.tone === "warning" ? "alert" : "status"} className={`text-xs ${notice.tone === "warning" ? "text-amber-300" : "text-slate-500"}`}>{notice.text}</p>
    </section>

    <section className="space-y-3">
      <h3 className="text-sm font-medium">成本护栏</h3>
      <p className="text-sm text-slate-400">团队指令在开始时冻结这些值，改动只对之后的新指令生效；点名和助理起草在每次启动时读取最新超时。</p>
      <div className="grid gap-4 md:grid-cols-2">
        {LIMIT_FIELDS.map((field) => <label key={field.key} className="block space-y-2">
          <span className="text-sm text-slate-400">{field.label}</span>
          <span className="flex items-center gap-2">
            <input className={inputClass} inputMode={field.integer ? "numeric" : "decimal"} aria-label={field.label} disabled={saving}
              value={form.limits[field.key]} onChange={(event) => edit({ limits: { ...form.limits, [field.key]: event.target.value } })} />
            <span className="shrink-0 text-xs text-slate-500">{field.unit}</span>
          </span>
          <span className="block text-xs leading-5 text-slate-500">{field.hint}</span>
        </label>)}
      </div>
    </section>

    {error && <p role="alert" className="text-sm text-red-300">{error}</p>}
    {!error && savedTick > 0 && !dirty && <p role="status" className="text-sm text-emerald-400">已保存。护栏从下一条新指令生效，助理从下一次起草生效。</p>}
    <div className="flex flex-wrap justify-end gap-2">
      <Button disabled={saving} onClick={() => edit({ limits: toForm({ assistantCli: null, limits: DEFAULT_LIMITS }).limits })}>恢复护栏默认值</Button>
      <Button variant="primary" disabled={saving || !dirty} onClick={() => void save()}>{saving ? "正在保存…" : "保存运行设置"}</Button>
    </div>
  </div>;
}
