import { useState } from "react";
import { Link } from "react-router";
import { AppManager } from "../components/AppManager";
import { DshSettingsPanel } from "../components/DshSettings";
import { ProxySettingsPanel } from "../components/ProxySettings";

type Section = "apps" | "proxy" | "dsh";

const sections: { key: Section; label: string }[] = [
  { key: "apps", label: "应用管理" },
  { key: "proxy", label: "网络代理" },
  { key: "dsh", label: "Dsh 设置" },
];

/** 系统设置整页：网络代理与 Dsh 设置都收在这里 */
export default function Settings() {
  const [section, setSection] = useState<Section>("apps");
  const [savedTick, setSavedTick] = useState(0);
  // 面板保存成功后会调用 onClose；整页里不关闭，只提示已保存
  const saved = () => setSavedTick((n) => n + 1);
  return (
    <main className="mx-auto max-w-6xl p-8">
      <header className="mb-6 flex items-baseline gap-4">
        <Link to="/" className="text-sm text-slate-400 hover:text-slate-100">← 返回</Link>
        <h1 className="text-2xl font-bold">系统设置</h1>
      </header>
      <div className="grid grid-cols-[200px_minmax(0,1fr)] gap-8">
        <nav className="space-y-1" role="tablist" aria-label="设置分类">
          {sections.map(({ key, label }) => (
            <button key={key} type="button" role="tab" aria-selected={section === key}
              onClick={() => { setSection(key); setSavedTick(0); }}
              className={`w-full rounded px-3 py-2 text-left text-sm ${
                section === key ? "bg-sky-900/60 text-sky-100" : "text-slate-400 hover:bg-slate-800 hover:text-slate-200"
              }`}>
              {label}
            </button>
          ))}
        </nav>
        <section className="max-w-3xl">
          <h2 className="mb-4 text-lg font-semibold">{sections.find((s) => s.key === section)?.label}</h2>
          {/* 用 key 让切换分类、保存后都重新读取最新配置 */}
          {section === "apps"
            ? <AppManager />
            : section === "proxy"
              ? <ProxySettingsPanel key={`proxy:${savedTick}`} embedded onClose={saved} />
              : <DshSettingsPanel key={`dsh:${savedTick}`} embedded onClose={saved} />}
          {section !== "apps" && savedTick > 0 && <p role="status" className="mt-3 text-sm text-emerald-400">已保存，从下一回合生效。</p>}
        </section>
      </div>
    </main>
  );
}
