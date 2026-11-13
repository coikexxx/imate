import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import type { CliConfig, DshCatalog, DshProfile } from "../types";
import { changeDshModel, dshBaseValue, dshEffortChoices, dshEffortOf, dshModelOptions, withDshEffort } from "../lib/dsh";
import { defaultModelHint, filterDshOptions } from "../lib/cliConfig";
import { Button } from "./ui";
import { Select } from "./Select";
import { DshSettingsButton } from "./DshSettings";

export function DshModelSelect({ value, onChange, config }: { value: string; onChange: (value: string) => void; config?: CliConfig }) {
  const [profile, setProfile] = useState<DshProfile>("web");
  const [catalog, setCatalog] = useState<DshCatalog | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const generation = useRef(0);
  async function refresh() {
    const current = ++generation.current;
    setLoading(true); setError(null);
    try {
      const settings = await api.getDshSettings();
      if (current !== generation.current) return;
      setProfile(settings.profile);
      const result = await api.listDshModels(settings.profile);
      if (current === generation.current) setCatalog(result);
    } catch (failure) { if (current === generation.current) setError(String(failure)); }
    finally { if (current === generation.current) setLoading(false); }
  }
  useEffect(() => { void refresh(); return () => { generation.current += 1; }; }, []);
  // 模型下拉框和白名单都按「不含推理强度」的值比较，强度单独保存在下面的选项里。
  const base = dshBaseValue(value);
  const options = filterDshOptions(dshModelOptions(catalog, base), config, base);
  const efforts = dshEffortChoices(catalog, value);
  return <div className="space-y-2">
    <Select aria-label="Dsh 模型" value={base} onChange={(next) => onChange(changeDshModel(catalog, value, next))}
      options={[{ value: "", label: config?.defaultModel ? defaultModelHint(config) : "使用 Dsh 默认模型与认证设置" }, ...options]} />
    {base && <div className="space-y-1">
      <Select aria-label="推理强度" value={dshEffortOf(value)} disabled={!efforts.declared && !dshEffortOf(value)}
        onChange={(effort) => onChange(withDshEffort(value, effort))} options={efforts.options} />
      <p className="text-xs text-slate-500">
        {efforts.declared
          ? "不选时沿用 Dsh 配置里的默认值。某些中转站要求模型必须开启思考，这时请在这里选一个强度。"
          : "这个模型没有在 Dsh 配置里声明推理强度，只能使用默认值；需要时到 Dsh 配置的 reasoningEfforts 里补充。"}
      </p>
    </div>}
    <div className="flex flex-wrap items-center gap-2">
      <Button disabled={loading} onClick={() => void refresh()}>{loading ? "正在读取模型…" : "刷新模型"}</Button>
      <DshSettingsButton onSaved={() => void refresh()} />
      <span className="text-xs text-slate-500">来源：Dsh {profile === "web" ? "Web" : "headless"}</span>
    </div>
    {error && <p role="alert" className="text-xs text-red-300">{error}</p>}
    {!loading && !error && options.length === 0 && <p className="text-xs text-slate-500">暂无可用模型，请先在 Dsh 中配置并登录，然后刷新。</p>}
    {catalog?.failures.map(failure => <p key={failure.provider} className="text-xs text-amber-300">{failure.provider}：{failure.message}</p>)}
  </div>;
}
