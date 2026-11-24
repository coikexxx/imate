import { confirm } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "../../api";
import {
  addCustomModel,
  isDetailDirty,
  mergeModels,
  providerHint,
  reconcileDefault,
  selectedModels,
  supportsProviderQuery,
  toInput,
  type ModelRow,
} from "../../lib/cliConfig";
import { dshModelOptions } from "../../lib/dsh";
import type { CliConfig, CliModel } from "../../types";
import { CliIcon } from "../ui/CliIcon";
import { Icon } from "../ui/Icon";
import { Select } from "../ui/Select";
import { Switch } from "../ui/Switch";
import { Button, Field, inputClass } from "../ui/ui";
import { DshSettingsPanel } from "./DshSettings";

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
  const requestSeq = useRef(0);
  const [candidateError, setCandidateError] = useState<string | null>(null);
  const [providerHost, setProviderHost] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);

  const loadCandidates = useCallback(
    async (fromProvider = false) => {
      const seq = ++requestSeq.current;
      const latest = () => requestSeq.current === seq;
      setLoading(true);
      setCandidateError(null);
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
        if (fromProvider) setProviderHost(host);
        setRows((current) => mergeModels(selectedModels(current), candidates));
      } catch (failure) {
        if (latest()) setCandidateError(String(failure));
      } finally {
        if (latest()) setLoading(false);
      }
    },
    [config.id],
  );

  useEffect(() => {
    void loadCandidates();
    return () => {
      requestSeq.current += 1;
    };
  }, [loadCandidates]);

  async function redetect() {
    setRefreshing(true);
    try {
      await onRefresh();
    } finally {
      setRefreshing(false);
    }
  }

  const checkedRows = rows.filter((r) => r.checked);
  const effectiveDefault = reconcileDefault(rows, defaultModel);
  const dirty = isDetailDirty(config, {
    enabled,
    customPath,
    models: selectedModels(rows),
    defaultModel: effectiveDefault,
  });

  function edited<T>(setter: (value: T) => void) {
    return (value: T) => {
      setSaved(false);
      setter(value);
    };
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
    setSaving(true);
    setError(null);
    setSaved(false);
    try {
      const next = await api.saveCliConfig(
        config.id,
        toInput(config, {
          enabled,
          customPath: customPath.trim() || null,
          models: selectedModels(rows),
          defaultModel: effectiveDefault,
        }),
      );
      onSaved(next);
      setDefaultModel(next.defaultModel);
      setSaved(true);
    } catch (failure) {
      setError(String(failure));
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className="max-w-3xl space-y-6 rounded-xl border border-line bg-surface p-6 shadow-[0_1px_2px_rgba(24,25,23,0.03)]">
      <div className="space-y-4">
        <button
          type="button"
          className="inline-flex items-center gap-1.5 rounded-md text-xs font-medium text-ink-secondary hover:text-ink"
          onClick={() => void back()}
        >
          <Icon name="back" width="14" height="14" />应用管理
        </button>
        <div className="flex items-center gap-3">
          <CliIcon id={config.id} label={config.label} />
          <h3 className="text-[17px] font-medium text-ink">{config.label}</h3>
          <span className="flex-1" />
          <span className="text-xs font-medium text-ink-secondary">{enabled && config.installed ? "已启用" : "未启用"}</span>
          <Switch checked={enabled && config.installed} disabled={!config.installed} label={`${config.label} 开关`} onChange={editEnabled} />
        </div>
      </div>

      <section className="space-y-4 border-t border-line pt-6">
        <h4 className="text-sm font-medium text-ink">接入状态</h4>
        <div className="rounded-lg border border-line bg-surface-subtle p-3.5 text-[13px] text-ink-secondary">
          <p className="flex items-center gap-2">
            <span className={`size-2 rounded-full ${config.installed ? "bg-accent" : "bg-ink-muted"}`} aria-hidden="true" />
            <span>
              {config.installed
                ? `已检测到${config.version ? ` · 版本 ${config.version}` : ""}`
                : "未检测到可执行文件，请先安装或在下方手动指定完整路径"}
            </span>
          </p>
          {config.path && <p className="mt-1.5 break-all pl-4 font-mono text-xs text-ink-muted">{config.path}</p>}
        </div>

        <Field label="手动指定路径" hint="留空表示自动从系统环境检测；保存后按新路径重新探测">
          <div className="flex gap-2">
            <input
              className={`${inputClass} font-mono text-xs`}
              value={customPath}
              placeholder="可执行文件的绝对路径"
              onChange={(event) => editCustomPath(event.target.value)}
            />
            <Button disabled={saving || refreshing} onClick={() => void redetect()}>
              <Icon name="refresh" width="14" height="14" />
              {refreshing ? "检测中…" : "重新检测"}
            </Button>
          </div>
        </Field>

        {config.id === "dsh" && (
          <div className="rounded-xl border border-line bg-paper p-4">
            <p className="mb-3 text-sm font-medium text-ink">Dsh 认证与配置来源</p>
            <DshSettingsPanel
              embedded
              onClose={() => undefined}
              onSaved={() => {
                void loadCandidates();
                void onRefresh();
              }}
            />
          </div>
        )}
      </section>

      <section className="space-y-4 border-t border-line pt-6">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div>
            <h4 className="text-sm font-medium text-ink">可用模型白名单</h4>
            <p className="mt-0.5 text-xs text-ink-secondary">勾选后新成员可选模型限于此范围；全部不勾选表示不限制</p>
          </div>
          <div className="flex flex-wrap items-center gap-2">
            {supportsProviderQuery(config.id) && (
              <Button disabled={loading} title={providerHint(providerHost)} onClick={() => void loadCandidates(true)}>
                从供应商读取
              </Button>
            )}
            <Button disabled={loading} onClick={() => void loadCandidates()}>
              <Icon name="refresh" width="14" height="14" />
              {loading ? "读取中…" : "刷新"}
            </Button>
          </div>
        </div>

        {supportsProviderQuery(config.id) && (
          <p className="text-xs leading-5 text-ink-muted">
            「刷新」只读本地目录；{providerHint(providerHost)}。密钥仅在本机后端使用，不会显示或写入日志。
          </p>
        )}
        {candidateError && (
          <p role="alert" className="rounded-lg border border-warn-border bg-warn-soft px-3 py-2 text-xs text-warn">
            {candidateError}
          </p>
        )}
        {!loading && !candidateError && rows.length === 0 && (
          <p className="text-xs text-ink-muted">
            {config.modelsSource === "builtin" ? "该 CLI 没有可读取的模型列表，请在下方手填模型 ID" : "暂无可用模型"}
          </p>
        )}

        {rows.length > 0 && (
          <ul className="divide-y divide-line overflow-hidden rounded-xl border border-line">
            {rows.map((row) => (
              <li key={row.id} className={row.checked ? "bg-accent-soft/40" : "bg-surface"}>
                <label className="flex min-h-10 cursor-pointer items-center gap-3 px-3.5 py-2 text-[13px] hover:bg-surface-subtle/70">
                  <input
                    type="checkbox"
                    className="size-4 shrink-0"
                    aria-label={`启用 ${row.label}`}
                    checked={row.checked}
                    onChange={(event) => toggleRow(row.id, event.target.checked)}
                  />
                  <span className="min-w-0 flex-1 break-words font-mono text-xs text-ink" title={row.id}>
                    {row.label}
                  </span>
                  {row.id === effectiveDefault && (
                    <span className="shrink-0 rounded bg-accent-soft px-1.5 py-0.5 text-[11px] font-medium text-accent">
                      默认
                    </span>
                  )}
                  {row.custom && (
                    <span className="shrink-0 rounded bg-surface-subtle px-1.5 py-0.5 text-[11px] text-ink-muted">
                      手填
                    </span>
                  )}
                </label>
              </li>
            ))}
          </ul>
        )}

        <div className="flex gap-2">
          <input
            className={`${inputClass} font-mono text-xs`}
            value={custom}
            placeholder="手填自定义模型 ID，如 claude-sonnet-4-6"
            onChange={(event) => setCustom(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") addCustom();
            }}
          />
          <Button className="shrink-0" disabled={custom.trim() === ""} onClick={addCustom}>
            添加
          </Button>
        </div>

        <Field label="默认模型" hint="成员未单独指定模型时使用此默认值">
          <Select
            aria-label="默认模型"
            value={effectiveDefault ?? ""}
            onChange={(value) => editDefaultModel(value || null)}
            options={[
              { value: "", label: "使用 CLI 自带默认" },
              ...checkedRows.map((row) => ({ value: row.id, label: row.label })),
            ]}
          />
        </Field>
      </section>

      {error && (
        <p role="alert" className="rounded-lg border border-danger-border bg-danger-soft px-3 py-2 text-sm text-danger">
          {error}
        </p>
      )}
      {saved && (
        <p role="status" className="text-sm font-medium text-accent">
          已保存，从下一回合生效
        </p>
      )}

      <div className="sticky bottom-0 -mx-6 -mb-6 flex flex-wrap items-center justify-end gap-2 rounded-b-xl border-t border-line bg-surface/95 px-6 py-3.5 backdrop-blur-xs">
        {dirty && <span role="status" className="mr-auto text-xs font-medium text-warn">有未保存的修改</span>}
        <Button onClick={() => void back()}>返回</Button>
        <Button variant="primary" disabled={saving} onClick={() => void save()}>
          {saving ? "正在保存…" : "保存"}
        </Button>
      </div>
    </div>
  );
}
