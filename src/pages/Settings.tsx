import { useState } from "react";
import { Link } from "react-router";
import { AppManager } from "../components/AppManager";
import { ProxySettingsPanel } from "../components/ProxySettings";

type Section = "apps" | "proxy";

// Dsh 的认证与配置来源设置已并入「应用管理」里 Dsh 的详情页，这里不再单列
const sections: { key: Section; label: string }[] = [
  { key: "apps", label: "应用管理" },
  { key: "proxy", label: "网络代理" },
];

/** 系统设置整页：应用管理（含 Dsh 设置）与网络代理 */
export default function Settings() {
  const [section, setSection] = useState<Section>("apps");
  const [savedTick, setSavedTick] = useState(0);
  // 面板保存成功后会调用 onClose；整页里不关闭，只提示已保存
  const saved = () => setSavedTick((n) => n + 1);
  return (
    <main className="mx-auto min-h-full max-w-6xl px-6 py-8 lg:px-8">
      <header className="mb-8 flex items-center gap-4 border-b border-slate-800 pb-6">
        <Link to="/" className="text-sm text-slate-400 hover:text-slate-100">← 返回</Link>
        <h1 className="text-2xl font-medium">系统设置</h1>
      </header>
      <div className="grid gap-6 md:grid-cols-[200px_minmax(0,1fr)] md:gap-8">
        <nav className="flex gap-1 self-start md:sticky md:top-8 md:flex-col" role="tablist" aria-label="设置分类">
          {sections.map(({ key, label }) => (
            <button key={key} type="button" role="tab" aria-selected={section === key}
              id={`settings-tab-${key}`} aria-controls={`settings-panel-${key}`}
              onClick={() => { setSection(key); setSavedTick(0); }}
              className={`flex min-h-9 w-full items-center gap-3 rounded-lg px-3 py-2 text-left text-sm ${
                section === key ? "bg-slate-800 text-slate-100" : "text-slate-400 hover:bg-slate-900 hover:text-slate-200"
              }`}>
              <span aria-hidden className={`size-1 rounded-full ${section === key ? "bg-sky-300" : "bg-transparent"}`} />
              {label}
            </button>
          ))}
        </nav>
        <section role="tabpanel" id={`settings-panel-${section}`} aria-labelledby={`settings-tab-${section}`} className="min-w-0 max-w-3xl">
          <h2 className="mb-4 text-xl font-medium">{sections.find((s) => s.key === section)?.label}</h2>
          {/* 用 key 让切换分类、保存后都重新读取最新配置 */}
          {section === "apps"
            ? <AppManager />
            : <ProxySettingsPanel key={`proxy:${savedTick}`} embedded onClose={saved} />}
          {section !== "apps" && savedTick > 0 &&<p role="status" className="mt-3 text-sm text-emerald-400">已保存，从下一回合生效</p>}
        </section>
      </div>
    </main>
  );
}
