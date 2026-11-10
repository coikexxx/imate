import { useEffect, useState } from "react";
import { api } from "./api";

export default function App() {
  const [summary, setSummary] = useState("加载中…");

  useEffect(() => {
    Promise.all([api.listProjects(), api.listClis()])
      .then(([projects, clis]) => {
        const installed = clis.filter((c) => c.installed).map((c) => c.label);
        setSummary(`${projects.length} 个项目 · 已安装的 CLI：${installed.join("、") || "无"}`);
      })
      .catch((e) => setSummary(`出错：${String(e)}`));
  }, []);

  return (
    <main className="p-8">
      <h1 className="text-2xl font-bold">imates</h1>
      <p className="mt-2 text-sm text-slate-400">{summary}</p>
    </main>
  );
}
