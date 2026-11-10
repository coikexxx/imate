import { useEffect, useState } from "react";
import { Link, useParams } from "react-router";
import { api } from "../api";
import type { ProjectDetail } from "../types";

export default function Project() {
  const { id = "" } = useParams();
  const [detail, setDetail] = useState<ProjectDetail | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api
      .getProject(id)
      .then(setDetail)
      .catch((e) => setError(String(e)));
  }, [id]);

  return (
    <main className="p-8">
      <Link to="/" className="text-sm text-slate-400 hover:text-slate-100">
        ← 返回
      </Link>
      {error && <p className="mt-4 text-sm text-red-400">{error}</p>}
      {detail && (
        <h1 className="mt-4 text-xl font-bold">
          {detail.project.name}（{detail.agents.length} 个成员）
        </h1>
      )}
    </main>
  );
}
