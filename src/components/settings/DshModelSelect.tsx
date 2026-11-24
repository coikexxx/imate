import { useEffect, useRef, useState } from "react";
import { api } from "../../api";
import { defaultModelHint, filterDshOptions } from "../../lib/cliConfig";
import { changeDshModel, dshBaseValue, dshEffortChoices, dshEffortOf, dshModelOptions, withDshEffort } from "../../lib/dsh";
import type { CliConfig, DshCatalog, DshProfile } from "../../types";
import { Icon } from "../ui/Icon";
import { Select } from "../ui/Select";
import { Button } from "../ui/ui";
import { DshSettingsButton } from "./DshSettings";

export function DshModelSelect({
  value,
  onChange,
  config,
}: {
  value: string;
  onChange: (value: string) => void;
  config?: CliConfig;
}) {
  const [profile, setProfile] = useState<DshProfile>("web");
  const [catalog, setCatalog] = useState<DshCatalog | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const generation = useRef(0);

  async function refresh() {
    const current = ++generation.current;
    setLoading(true);
    setError(null);
    try {
      const settings = await api.getDshSettings();
      if (current !== generation.current) return;
      setProfile(settings.profile);
      const result = await api.listDshModels(settings.profile);
      if (current === generation.current) setCatalog(result);
    } catch (failure) {
      if (current === generation.current) setError(String(failure));
    } finally {
      if (current === generation.current) setLoading(false);
    }
  }

  useEffect(() => {
    void refresh();
    return () => {
      generation.current += 1;
    };
  }, []);

  const base = dshBaseValue(value);
  const options = filterDshOptions(dshModelOptions(catalog, base), config, base);
  const efforts = dshEffortChoices(catalog, value);

  return (
    <div className="space-y-2">
      <Select
        aria-label="Dsh 模型"
        value={base}
        onChange={(next) => onChange(changeDshModel(catalog, value, next))}
        options={[
          { value: "", label: config?.defaultModel ? defaultModelHint(config) : "使用 Dsh 默认模型与认证设置" },
          ...options,
        ]}
      />
      {base && (
        <div className="space-y-1">
          <Select
            aria-label="推理强度"
            value={dshEffortOf(value)}
            disabled={!efforts.declared && !dshEffortOf(value)}
            onChange={(effort) => onChange(withDshEffort(value, effort))}
            options={efforts.options}
          />
          <p className="text-xs text-ink-muted">
            {efforts.declared
              ? "不选时沿用 Dsh 配置默认值；某些中转站要求开启思考，请在此指定强度。"
              : "该模型未在 Dsh 配置中声明推理强度，只能使用默认值。"}
          </p>
        </div>
      )}
      <div className="flex flex-wrap items-center gap-2">
        <Button disabled={loading} onClick={() => void refresh()}>
          <Icon name="refresh" width="14" height="14" />
          {loading ? "读取中…" : "刷新模型"}
        </Button>
        <DshSettingsButton onSaved={() => void refresh()} />
        <span className="text-xs text-ink-muted">来源：Dsh {profile === "web" ? "Web" : "headless"}</span>
      </div>
      {error && <p role="alert" className="text-xs text-danger">{error}</p>}
      {!loading && !error && options.length === 0 && (
        <p className="text-xs text-ink-muted">暂无可用模型，请先在 Dsh 中配置并登录，然后刷新。</p>
      )}
      {catalog?.failures.map((failure) => (
        <p key={failure.provider} className="text-xs text-warn">
          {failure.provider}：{failure.message}
        </p>
      ))}
    </div>
  );
}
