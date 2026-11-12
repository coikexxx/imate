import { useState } from "react";
import { AppManager } from "../components/AppManager";
import { ProxySettingsPanel } from "../components/ProxySettings";
import { AppShell } from "../components/AppShell";
import { Icon, type IconName } from "../components/Icon";

type Section = "apps" | "proxy";
const sections: { key: Section; label: string; icon: IconName; description: string }[] = [
  { key: "apps", label: "应用管理", icon: "grid", description: "连接本机 CLI，为你的团队准备合适的模型" },
  { key: "proxy", label: "网络代理", icon: "connection", description: "统一设置团队成员和内置助理的网络连接" },
];

export default function Settings() {
  const [section, setSection] = useState<Section>("apps");
  const [savedTick, setSavedTick] = useState(0);
  const saved = () => setSavedTick((n) => n + 1);
  return <AppShell><main className="studio-page">
    <header className="studio-page-head"><div><p className="studio-kicker"><Icon name="settings" />工作空间 / 设置</p><h1 className="studio-page-title">系统设置</h1><p className="studio-subtitle mt-2">让本地工具成为团队的一部分</p></div></header>
    <nav className="studio-setting-tabs" role="tablist" aria-label="设置分类">
      {sections.map(({ key, label, icon }) => <button key={key} type="button" role="tab" aria-selected={section === key} id={`settings-tab-${key}`} aria-controls={`settings-panel-${key}`} onClick={() => { setSection(key); setSavedTick(0); }} className="studio-setting-tab"><Icon name={icon} />{label}</button>)}
    </nav>
    <section role="tabpanel" id={`settings-panel-${section}`} aria-labelledby={`settings-tab-${section}`} className="studio-settings-content">
      <div className="mb-6"><h2 className="text-lg font-medium">{sections.find((s) => s.key === section)?.label}</h2><p className="studio-subtitle mt-1">{sections.find((s) => s.key === section)?.description}</p></div>
      {section === "apps" ? <AppManager /> : <div className="max-w-2xl rounded-xl border border-slate-800 bg-white p-6"><ProxySettingsPanel key={`proxy:${savedTick}`} embedded onClose={saved} /></div>}
      {section !== "apps" && savedTick > 0 && <p role="status" className="mt-3 text-sm text-emerald-400">已保存，从下一回合生效</p>}
    </section>
  </main></AppShell>;
}
