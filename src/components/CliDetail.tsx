import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "../api";
import {
  addCustomModel, mergeModels, reconcileDefault, selectedModels, toInput, type ModelRow,
} from "../lib/cliConfig";
import { dshModelOptions } from "../lib/dsh";
import type { CliConfig, CliModel } from "../types";
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
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);

  const loadCandidates = useCallback(async () => {
    const seq = ++requestSeq.current;
    const latest = () => requestSeq.current === seq;
    setLoading(true); setCandidateError(null);
    try {
      let candidates: CliModel[];
      let failureText: string | null = null;
      if (config.id === "dsh") {
        const settings = await api.getDshSettings();
        const catalog = await api.listDshModels(settings.profile);
        candidates = dshModelOptions(catalog, "").map((o) => ({ id: o.value, label: o.label, custom: false }));
        failureText = catalog.failures.map((f) => `${f.provider}：${f.message}`).join("；") || null;
      } else {
        const catalog = await api.listCliModels(config.id);
        candidates = catalog.models;
        failureText = catalog.error;
      }
      if (!latest()) return;
      setCandidateError(failureText);
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

  async function save() {
    setSaving(true); setError(null); setSaved(false);
    try {
      const next = await api.saveCliConfig(config.id, toInput(config, {
        enabled,
        customPath: customPath.trim() || null,
        models: selectedModels(rows),
        defaultModel: reconcileDefault(rows, defaultModel),
      }));
      onSaved(next);
      setDefaultModel(next.defaultModel);
      setSaved(true);
    } catch (failure) { setError(String(failure)); }
    finally { setSaving(false); }
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center gap-3">
        <button type="button" className="text-sm text-slate-400 hover:text-slate-100" onClick={onBack}>← 应用管理</button>
        <h3 className="text-lg font-semibold">{config.label}</h3>
        <span className="flex-1" />
        <Switch checked={enabled && config.installed} disabled={!config.installed} label={`${config.label} 开关`} onChange={setEnabled} />
      </div>

      <section className="space-y-3">
        <h4 className="text-sm font-medium">接入</h4>
        <div className="rounded border border-slate-800 p-3 text-xs text-slate-400">
          <p>{config.installed ? `已检测到${config.version ? `，版本 ${config.version}` : ""}` : "未检测到。请先安装，或在下面手动指定可执行文件的完整路径。"}</p>
          {config.path && <p className="mt-1 break-all font-mono text-slate-500">{config.path}</p>}
        </div>
        <Field label="手动指定路径（可选，留空自动检测）">
          <input className={inputClass} value={customPath} placeholder="可执行文件的绝对路径" onChange={(event) => setCustomPath(event.target.value)} />
        </Field>
        <p className="text-xs text-slate-500">路径保存后生效；保存后会按新路径重新检测</p>
        <div className="flex gap-2">
          <Button disabled={saving || refreshing} onClick={() => void redetect()}>{refreshing ? "正在检测…" : "重新检测"}</Button>
        </div>
        {config.id === "dsh" && (
          <div className="rounded border border-slate-800 p-3">
            <p className="mb-3 text-sm font-medium">Dsh 认证与配置来源</p>
            <DshSettingsPanel embedded onClose={() => undefined} onSaved={() => void loadCandidates()} />
          </div>
        )}
      </section>

      <section className="space-y-3">
        <div className="flex items-center justify-between">
          <h4 className="text-sm font-medium">模型（勾选后，成员只能从这些里选；不勾选任何一个表示不限制）</h4>
          <Button disabled={loading} onClick={() => void loadCandidates()}>{loading ? "正在读取…" : "刷新"}</Button>
        </div>
        {candidateError && <p role="alert" className="text-xs text-amber-300">{candidateError}</p>}
        {!loading && !candidateError && rows.length === 0 && (
          <p className="text-xs text-slate-500">
            {config.modelsSource === "builtin" ? "该 CLI 没有可读取的模型列表，请在下面手填模型 ID。" : "暂无可用模型。"}
          </p>
        )}
        <ul className="divide-y divide-slate-800 rounded border border-slate-800">
          {rows.map((row) => (
            <li key={row.id} className="flex items-center gap-3 px-3 py-2 text-sm">
              <input type="checkbox" aria-label={`启用 ${row.label}`} checked={row.checked}
                onChange={(event) => setRows((current) => current.map((r) => (r.id === row.id ? { ...r, checked: event.target.checked } : r)))} />
              <span className="min-w-0 flex-1 truncate" title={row.id}>{row.label}</span>
              {row.custom && <span className="shrink-0 rounded bg-slate-800 px-1.5 text-xs text-slate-400">手填</span>}
            </li>
          ))}
        </ul>
        <div className="flex gap-2">
          <input className={inputClass} value={custom} placeholder="手填模型 ID，如 my-model"
            onChange={(event) => setCustom(event.target.value)}
            onKeyDown={(event) => { if (event.key === "Enter") { setRows((c) => addCustomModel(c, custom)); setCustom(""); } }} />
          <Button className="shrink-0" disabled={custom.trim() === ""} onClick={() => { setRows((c) => addCustomModel(c, custom)); setCustom(""); }}>添加</Button>
        </div>
        <Field label="默认模型（成员没有单独指定模型时使用）">
          <select className={inputClass} value={reconcileDefault(rows, defaultModel) ?? ""} onChange={(event) => setDefaultModel(event.target.value || null)}>
            <option value="">使用 CLI 自带默认</option>
            {checkedRows.map((row) => <option key={row.id} value={row.id}>{row.label}</option>)}
          </select>
        </Field>
      </section>

      {error && <p role="alert" className="text-sm text-red-300">{error}</p>}
      {saved && <p role="status" className="text-sm text-emerald-400">已保存，从下一回合生效。</p>}
      <div className="flex justify-end gap-2">
        <Button onClick={onBack}>返回</Button>
        <Button variant="primary" disabled={saving} onClick={() => void save()}>{saving ? "正在保存…" : "保存"}</Button>
      </div>
    </div>
  );
}
