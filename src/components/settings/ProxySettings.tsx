import { useEffect, useState } from "react";
import { api } from "../../api";
import type { ProxySettings, ProxyState } from "../../types";
import { Select } from "../ui/Select";
import { Button, Field, inputClass } from "../ui/ui";

/** 网络代理设置表单，嵌入系统设置页 */
export function ProxySettingsPanel({ onClose, embedded = false }: { onClose: () => void; embedded?: boolean }) {
  const [settings, setSettings] = useState<ProxySettings>({ mode: "system", url: "" });
  const [detected, setDetected] = useState<ProxyState | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let disposed = false;
    api
      .getNetworkProxy()
      .then((result) => {
        if (!disposed) {
          setSettings(result.settings);
          setDetected(result);
        }
      })
      .catch((failure) => {
        if (!disposed) setError(String(failure));
      })
      .finally(() => {
        if (!disposed) setLoading(false);
      });
    return () => {
      disposed = true;
    };
  }, []);

  async function save() {
    setSaving(true);
    setError(null);
    try {
      await api.saveNetworkProxy(settings);
      onClose();
    } catch (failure) {
      setError(String(failure));
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className="space-y-4">
      <p className="text-[13px] leading-5 text-ink-secondary">
        所有已接入 CLI 成员与内置助理共用此设置。保存后从新回合生效，正在运行的回合保持原代理。
      </p>
      <Field label="连接方式">
        <Select
          aria-label="连接方式"
          disabled={loading || saving}
          value={settings.mode}
          onChange={(mode) => setSettings({ ...settings, mode: mode as ProxySettings["mode"] })}
          options={[
            { value: "system", label: "跟随本机代理" },
            { value: "manual", label: "手动代理" },
            { value: "direct", label: "直连" },
          ]}
        />
      </Field>
      {settings.mode === "system" && (
        <div className="rounded-lg border border-line bg-surface-subtle p-3.5 text-xs leading-5 text-ink-secondary">
          <p className="font-mono text-ink">
            {loading ? "正在读取本机代理…" : detected?.detectedProxy ?? "读取失败，请重试或改用手动代理"}
          </p>
          <p className="mt-1.5 text-ink-muted">
            优先继承启动环境中的代理变量；Windows 未设置环境变量时读取系统静态代理，其他平台可手填地址。
          </p>
          {detected?.detectionError && <p className="mt-1.5 text-warn">{detected.detectionError}</p>}
        </div>
      )}
      {settings.mode === "manual" && (
        <Field label="代理地址" hint="填写协议、主机和显式端口，支持 HTTP、HTTPS、SOCKS5">
          <input
            className={`${inputClass} font-mono text-xs`}
            value={settings.url}
            disabled={saving}
            placeholder="http://127.0.0.1:7890"
            onChange={(event) => setSettings({ ...settings, url: event.target.value })}
          />
        </Field>
      )}
      {error && (
        <p role="alert" className="rounded-lg border border-danger-border bg-danger-soft px-3 py-2 text-sm text-danger">
          {error}
        </p>
      )}
      <div className="flex justify-end gap-2 border-t border-line pt-4">
        {!embedded && (
          <Button disabled={saving} onClick={onClose}>
            取消
          </Button>
        )}
        <Button variant="primary" disabled={loading || saving} onClick={() => void save()}>
          {saving ? "正在保存…" : "保存代理设置"}
        </Button>
      </div>
    </div>
  );
}
