import { useEffect, useState } from "react";
import { api } from "../api";
import type { ProxySettings, ProxyState } from "../types";
import { Button, Field, inputClass } from "./ui";

/** 网络代理设置表单，嵌入系统设置页 */
export function ProxySettingsPanel({ onClose, embedded = false }: { onClose: () => void; embedded?: boolean }) {
  const [settings, setSettings] = useState<ProxySettings>({ mode: "system", url: "" });
  const [detected, setDetected] = useState<ProxyState | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let disposed = false;
    api.getNetworkProxy().then((result) => {
      if (!disposed) { setSettings(result.settings); setDetected(result); }
    }).catch((failure) => { if (!disposed) setError(String(failure)); })
      .finally(() => { if (!disposed) setLoading(false); });
    return () => { disposed = true; };
  }, []);

  async function save() {
    setSaving(true); setError(null);
    try { await api.saveNetworkProxy(settings); onClose(); }
    catch (failure) { setError(String(failure)); }
    finally { setSaving(false); }
  }

  return (
    <div className="space-y-4">
      <p className="text-sm text-slate-400">所有项目成员和内置助理共用此设置，适用于 Claude、Codex、Grok、Dsh 等已接入的 CLI。保存后从新回合生效，正在进行的回合继续使用原来的代理。</p>
      <Field label="连接方式">
        <select className={inputClass} disabled={loading || saving} value={settings.mode}
          onChange={(event) => setSettings({ ...settings, mode: event.target.value as ProxySettings["mode"] })}>
          <option value="system">跟随本机代理</option>
          <option value="manual">手动代理</option>
          <option value="direct">直连</option>
        </select>
      </Field>
      {settings.mode === "system" && <div className="rounded border border-slate-700 p-3 text-xs text-slate-400">
        <p>{loading ? "正在读取本机代理…" : detected?.detectedProxy ?? "读取失败，请重试或使用手动代理"}</p>
        <p className="mt-2">优先沿用启动环境中的代理。Windows 未设置代理环境变量时，读取系统静态代理；其他系统可手动填写地址。</p>
        {detected?.detectionError && <p className="mt-2 text-amber-300">{detected.detectionError}</p>}
      </div>}
      {settings.mode === "manual" && <Field label="代理地址">
        <input className={inputClass} value={settings.url} disabled={saving} placeholder="http://127.0.0.1:7890"
          onChange={(event) => setSettings({ ...settings, url: event.target.value })} />
        <span className="text-xs text-slate-500">填写协议、主机和端口，支持 HTTP、HTTPS、SOCKS5。</span>
      </Field>}
      {error && <p role="alert" className="text-sm text-red-300">{error}</p>}
      <div className="flex justify-end gap-2">
        {!embedded && <Button disabled={saving} onClick={onClose}>取消</Button>}
        <Button variant="primary" disabled={loading || saving} onClick={() => void save()}>{saving ? "正在保存…" : "保存代理设置"}</Button>
      </div>
    </div>
  );
}
