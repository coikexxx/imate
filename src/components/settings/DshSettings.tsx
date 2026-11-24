import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { api } from "../../api";
import type { DshSettings as Settings } from "../../types";
import { Select } from "../ui/Select";
import { Button, Field, Modal } from "../ui/ui";

export function DshSettingsButton({ onSaved }: { onSaved?: () => void }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <Button onClick={() => setOpen(true)}>Dsh 设置</Button>
      {open &&
        createPortal(
          <Modal title="Dsh 认证与模型设置" onClose={() => setOpen(false)}>
            <DshSettingsPanel onClose={() => setOpen(false)} onSaved={onSaved} />
          </Modal>,
          document.body,
        )}
    </>
  );
}

/** Dsh 认证与模型设置表单，可嵌入任意容器（独立弹窗或系统设置） */
export function DshSettingsPanel({
  onClose,
  onSaved,
  embedded = false,
}: {
  onClose: () => void;
  onSaved?: () => void;
  embedded?: boolean;
}) {
  const [settings, setSettings] = useState<Settings>({ authentication: "headless", profile: "web" });
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let disposed = false;
    api
      .getDshSettings()
      .then((value) => {
        if (!disposed) setSettings(value);
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
      await api.saveDshSettings(settings);
      onSaved?.();
      onClose();
    } catch (failure) {
      setError(String(failure));
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className="space-y-4">
      <p className="text-xs leading-5 text-ink-secondary">
        所有项目和内置助理共用，从下一回合生效。账号登录、密钥和模型目录由 Dsh 原生管理。
      </p>
      <Field label="默认认证方式" hint="未指定成员模型时使用此方式；指定自定义模型后沿用该供应商在 Dsh 中的凭据">
        <Select
          aria-label="默认认证方式"
          disabled={loading || saving}
          value={settings.authentication}
          onChange={(authentication) =>
            setSettings({ ...settings, authentication: authentication as Settings["authentication"] })
          }
          options={[
            { value: "headless", label: "沿用 Dsh headless 配置" },
            { value: "account", label: "使用已登录的 DeepSeek 账号" },
          ]}
        />
      </Field>
      <Field label="读取模型配置的位置" hint="成员模型列表从此处读取；账号认证也从此配置读取服务信息">
        <Select
          aria-label="读取模型配置的位置"
          disabled={loading || saving}
          value={settings.profile}
          onChange={(profile) => setSettings({ ...settings, profile: profile as Settings["profile"] })}
          options={[
            { value: "web", label: "Dsh Web（dsh web）" },
            { value: "headless", label: "Dsh headless" },
          ]}
        />
      </Field>
      {error && (
        <p role="alert" className="rounded-lg border border-danger-border bg-danger-soft px-3 py-2 text-xs text-danger">
          {error}
        </p>
      )}
      <div className="flex justify-end gap-2">
        {!embedded && (
          <Button disabled={saving} onClick={onClose}>
            取消
          </Button>
        )}
        <Button variant="primary" disabled={loading || saving} onClick={() => void save()}>
          {saving ? "正在保存…" : "保存 Dsh 设置"}
        </Button>
      </div>
    </div>
  );
}
