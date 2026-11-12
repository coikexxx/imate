import { confirm } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "../api";
import {
  addCustomModel, isDetailDirty, mergeModels, providerHint, reconcileDefault, selectedModels, supportsProviderQuery, toInput,
  type ModelRow,
} from "../lib/cliConfig";
import { dshModelOptions } from "../lib/dsh";
import type { CliConfig, CliModel } from "../types";
import { CliIcon } from "./CliIcon";
import { DshSettingsPanel } from "./DshSettings";
import { Switch } from "./Switch";
import { Button, Field, inputClass } from "./ui";

interface Props {
  config: CliConfig;
  onBack: () => void;
  /** 保存成功后用后端返回的最新视图替换列表里的这一项 */
  onSaved: (saved: CliConfig) => void;
  /** 重新检测所有 CLI */
  onRefresh: () => Promise<void>;
}

/** 单个 CLI 的详情：接入（路径、检测）与模型（勾选可用模型、选默认） */
export function CliDetail({ config, onBack, onSaved, onRefresh }: Props) {
  const [enabled, setEnabled] = useState(config.enabled);
  const [customPath, setCustomPath] = useState(config.customPath ?? "");
  const [rows, setRows] = useState<ModelRow[]>(() => mergeModels(config.models, []));
  const [defaultModel, setDefaultModel] = useState<string | null>(config.defaultModel);
  const [custom, setCustom] = useState("");
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  // 候选请求序号：只有最新一次请求的结果才写入状态，卸载时自增使在途请求作废
  const requestSeq = useRef(0);
  const [candidateError, setCandidateError] = useState<string | null>(null);
  // 供应商主机名（后端上一次返回），用来告诉用户密钥会发给谁
  const [providerHost, setProviderHost] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);

  const loadCandidates = useCallback(async (fromProvider = false) => {
    const seq = ++requestSeq.current;
    const latest = () => requestSeq.current === seq;
    setLoading(true); setCandidateError(null);
    try {
      let candidates: CliModel[];
      let failureText: string | null = null;
      let host: string | null = null;
      if (config.id === "dsh") {
        const settings = await api.getDshSettings();
        const catalog = await api.listDshModels(settings.profile);
        candidates = dshModelOptions(catalog, "").map((o) => ({ id: o.value, label: o.label, custom: false }));
        failureText = catalog.failures.map((f) => `${f.provider}：${f.message}`).join("；") || null;
      } else {
        const catalog = await api.listCliModels(config.id, fromProvider);
        candidates = catalog.models;
        failureText = catalog.error;
        host = catalog.providerHost;
      }
      if (!latest()) return;
      setCandidateError(failureText);
      // 从供应商读取时以本次结果为准（包括 null）；普通刷新保持原值
      if (fromProvider) setProviderHost(host);
      setRows((current) => mergeModels(selectedModels(current), candidates));
    } catch (failure) { if (latest()) setCandidateError(String(failure)); }
    finally { if (latest()) setLoading(false); }
  }, [config.id]);
  useEffect(() => {
    void loadCandidates();
    return () => { requestSeq.current += 1; };
  }, [loadCandidates]);

  async function redetect() {
    setRefreshing(true);
    try { await onRefresh(); }
    finally { setRefreshing(false); }
  }

  const checkedRows = rows.filter((r) => r.checked);
  const effectiveDefault = reconcileDefault(rows, defaultModel);
  const dirty = isDetailDirty(config, { enabled, customPath, models: selectedModels(rows), defaultModel: effectiveDefault });

  // 任何字段被编辑后，上一次的「已保存」提示就不再成立
  function edited<T>(setter: (value: T) => void) {
    return (value: T) => { setSaved(false); setter(value); };
  }
  const editEnabled = edited(setEnabled);
  const editCustomPath = edited(setCustomPath);
  const editDefaultModel = edited(setDefaultModel);
  function toggleRow(id: string, checked: boolean) {
    setSaved(false);
    setRows((current) => current.map((r) => (r.id === id ? { ...r, checked } : r)));
  }
  function addCustom() {
    if (custom.trim() === "") return;
    setSaved(false);
    setRows((current) => addCustomModel(current, custom));
    setCustom("");
  }
  async function back() {
    if (dirty && !(await confirm("放弃未保存的修改？", { title: "未保存的修改", kind: "warning" }))) return;
    onBack();
  }

  async function save() {
    setSaving(true); setError(null); setSaved(false);
    try {
      const next = await api.saveCliConfig(config.id, toInput(config, {
        enabled,
        customPath: customPath.trim() || null,
        models: selectedModels(rows),
        defaultModel: effectiveDefault,
      }));
      onSaved(next);
      setDefaultModel(next.defaultModel);
      setSaved(true);
    } catch (failure) { setError(String(failure)); }
    finally { setSaving(false); }
  }

  return (
    <div className="space-y-8">
      <div className="space-y-4">
        <button type="button" className="rounded text-sm text-slate-400 hover:text-slate-100" onClick={() => void back()}>← 应用管理</button>
        <div className="flex items-center gap-3">
          <CliIcon id={config.id} label={config.label} />
          <h3 className="text-lg font-medium">{config.label}</h3>
          <span className="flex-1" />
          <span className="text-sm text-slate-400">{enabled && config.installed ? "已启用" : "未启用"}</span>
          <Switch checked={enabled && config.installed} disabled={!config.installed} label={`${config.label} 开关`} onChange={editEnabled} />
        </div>
      </div>

      <section className="space-y-4">
        <h4 className="text-base font-medium">接入</h4>
        <div className="rounded-xl border border-slate-800 bg-slate-900/60 p-4 text-sm text-slate-400">
          <p>{config.installed ? `已检测到${config.version ? `，版本 ${config.version}` : ""}` : "未检测到。请先安装，或在下面手动指定可执行文件的完整路径。"}</p>
          {config.path && <p className="mt-1 break-all font-mono text-slate-500">{config.path}</p>}
        </div>
        <Field label="手动指定路径">
          <input className={inputClass} value={customPath} placeholder="可执行文件的绝对路径" onChange={(event) => editCustomPath(event.target.value)} />
        </Field>
        <p className="text-sm leading-5 text-slate-500">留空自动检测。保存后按新路径重新检测。</p>
        <div className="flex gap-2">
          <Button disabled={saving || refreshing} onClick={() => void redetect()}>{refreshing ? "正在检测…" : "重新检测"}</Button>
        </div>
        {config.id === "dsh" && (
          <div className="rounded border border-slate-800 p-3">
            <p className="mb-3 text-sm font-medium">Dsh 认证与配置来源</p>
            <DshSettingsPanel embedded onClose={() => undefined} onSaved={() => { void loadCandidates(); void onRefresh(); }} />
          </div>
        )}
      </section>

      <section className="space-y-4 border-t border-slate-800 pt-6">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <h4 className="text-base font-medium">可用模型</h4>
          <div className="flex flex-wrap items-center gap-2">
            {supportsProviderQuery(config.id) && (
              <Button disabled={loading} title={providerHint(providerHost)} onClick={() => void loadCandidates(true)}>从供应商读取</Button>
            )}
            <Button disabled={loading} onClick={() => void loadCandidates()}>{loading ? "正在读取…" : "刷新"}</Button>
          </div>
        </div>
        <p className="text-sm leading-6 text-slate-400">勾选后，新选模型限于这些选项；全部不选表示不限制。</p>
        {supportsProviderQuery(config.id) && (
          <p className="text-sm leading-6 text-slate-500">「刷新」只读本地目录；{providerHint(providerHost)}。密钥只在本机后端使用，不会显示或保存。</p>
        )}
        {candidateError && <p role="alert" className="text-xs text-amber-300">{candidateError}</p>}
        {!loading && !candidateError && rows.length === 0 && (
          <p className="text-xs text-slate-500">
            {config.modelsSource === "builtin" ? "该 CLI 没有可读取的模型列表，请在下面手填模型 ID。" : "暂无可用模型。"}
          </p>
        )}
        <ul className="divide-y divide-slate-800 overflow-hidden rounded-xl border border-slate-800">
          {rows.map((row) => (
            <li key={row.id} className={row.checked ? "bg-slate-900" : ""}>
              <label className="flex min-h-11 cursor-pointer items-center gap-3 px-4 py-3 text-sm hover:bg-slate-800/50">
                <input type="checkbox" className="size-4 shrink-0" aria-label={`启用 ${row.label}`} checked={row.checked}
                  onChange={(event) => toggleRow(row.id, event.target.checked)} />
                <span className="min-w-0 flex-1 break-words" title={row.id}>{row.label}</span>
                {row.id === effectiveDefault && <span className="shrink-0 text-xs text-sky-200">默认</span>}
                {row.custom && <span className="shrink-0 text-xs text-slate-500">手填</span>}
              </label>
            </li>
          ))}
        </ul>
        <div className="flex gap-2">
          <input className={inputClass} value={custom} placeholder="手填模型 ID，如 my-model"
            onChange={(event) => setCustom(event.target.value)}
            onKeyDown={(event) => { if (event.key === "Enter") addCustom(); }} />
          <Button className="shrink-0" disabled={custom.trim() === ""} onClick={addCustom}>添加</Button>
        </div>
        <Field label="默认模型">
          <select className={inputClass} value={effectiveDefault ?? ""} onChange={(event) => editDefaultModel(event.target.value || null)}>
            <option value="">使用 CLI 自带默认</option>
            {checkedRows.map((row) => <option key={row.id} value={row.id}>{row.label}</option>)}
          </select>
        </Field>
        <p className="text-sm text-slate-500">成员未单独指定模型时，使用默认模型。</p>
      </section>
      {error && <p role="alert" className="text-sm text-red-300">{error}</p>}
      {saved && <p role="status" className="text-sm text-emerald-400">已保存，从下一回合生效</p>}
      <div className="flex flex-wrap items-center justify-end gap-2 border-t border-slate-800 pt-4">
        {dirty && <span role="status" className="mr-1 text-xs text-amber-300">有未保存的修改</span>}
        <Button onClick={() => void back()}>返回</Button>
        <Button variant="primary" disabled={saving} onClick={() => void save()}>{saving ? "正在保存…" : "保存"}</Button>
      </div>
    </div>
  );
}
