import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import type { CliConfig, DshCatalog, DshProfile } from "../types";
import { dshModelOptions } from "../lib/dsh";
import { defaultModelHint, filterDshOptions } from "../lib/cliConfig";
import { Button, inputClass } from "./ui";
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
  const options = filterDshOptions(dshModelOptions(catalog, value), config, value);
  return <div className="space-y-2">
    <select className={inputClass} value={value} onChange={event => onChange(event.target.value)}>
      <option value="">{config?.defaultModel ? defaultModelHint(config) : "使用 Dsh 默认模型与认证设置"}</option>
      {options.map(option => <option key={option.value} value={option.value}>{option.label}</option>)}
    </select>
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
