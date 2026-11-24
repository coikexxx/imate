import { useState } from "react";
import { useSearchParams } from "react-router";
import { AppShell } from "../components/layout/AppShell";
import { AppManager } from "../components/settings/AppManager";
import { ProxySettingsPanel } from "../components/settings/ProxySettings";
import { RuntimeSettingsPanel } from "../components/settings/RuntimeSettingsPanel";
import { Icon, type IconName } from "../components/ui/Icon";
import { settingsSectionFromParam, type SettingsSection as Section } from "../lib/settingsSection";

const sections: { key: Section; label: string; icon: IconName; description: string }[] = [
  { key: "apps", label: "应用管理", icon: "grid", description: "启用本机 CLI 并配置可选模型白名单与默认模型，关闭不影响已有成员" },
  { key: "runtime", label: "运行设置", icon: "settings", description: "配置内置助理所用 CLI，以及讨论轮数、回合预算和超时的安全护栏" },
  { key: "proxy", label: "网络代理", icon: "connection", description: "统一设置所有已接入 CLI 成员与内置助理的子进程网络连接" },
];

export default function Settings() {
  const [params, setParams] = useSearchParams();
  const section = settingsSectionFromParam(params.get("tab"));
  const [savedTick, setSavedTick] = useState(0);
  const saved = () => setSavedTick((n) => n + 1);
  const active = sections.find((s) => s.key === section) ?? sections[0];

  return (
    <AppShell>
      <main className="studio-page">
        <header className="studio-page-head">
          <div>
            <p className="studio-kicker"><Icon name="settings" width="14" height="14" />工作空间 / 设置</p>
            <h1 className="studio-page-title">系统设置</h1>
          </div>
        </header>

        <nav className="studio-setting-tabs" role="tablist" aria-label="设置分类">
          {sections.map(({ key, label, icon }) => (
            <button
              key={key}
              type="button"
              role="tab"
              aria-selected={section === key}
              id={`settings-tab-${key}`}
              aria-controls={`settings-panel-${key}`}
              onClick={() => {
                setParams({ tab: key }, { replace: true });
                setSavedTick(0);
              }}
              className="studio-setting-tab"
            >
              <Icon name={icon} width="15" height="15" />
              {label}
            </button>
          ))}
        </nav>

        <section
          role="tabpanel"
          id={`settings-panel-${section}`}
          aria-labelledby={`settings-tab-${section}`}
          className="studio-settings-content"
        >
          <p className="studio-subtitle mb-5">{active.description}</p>
          {section === "apps" && <AppManager />}
          {section === "runtime" && (
            <div className="max-w-3xl rounded-xl border border-line bg-surface p-6 shadow-[0_1px_2px_rgba(24,25,23,0.03)]">
              <RuntimeSettingsPanel />
            </div>
          )}
          {section === "proxy" && (
            <div className="max-w-2xl rounded-xl border border-line bg-surface p-6 shadow-[0_1px_2px_rgba(24,25,23,0.03)]">
              <ProxySettingsPanel key={`proxy:${savedTick}`} embedded onClose={saved} />
            </div>
          )}
          {section === "proxy" && savedTick > 0 && (
            <p role="status" className="mt-3 text-sm font-medium text-accent">已保存，从下一回合生效</p>
          )}
        </section>
      </main>
    </AppShell>
  );
}
