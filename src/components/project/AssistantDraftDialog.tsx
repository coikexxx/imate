import { useEffect, useRef, useState } from "react";
import { api } from "../../api";
import { acceptDraftResponse, canApplyPersona, canApplyRules, canSendFeedback } from "../../lib/drafts";
import type { DraftInput, DraftResponse, RuleDoc } from "../../types";
import { Button, Field, Modal, inputClass } from "../ui/ui";
import { PersonaEditor } from "./PersonaEditor";
import { RulesEditor } from "./RulesEditor";

export type DraftContext = Omit<DraftInput, "draftId" | "currentText" | "feedback">;

interface Props {
  kind: "rules" | "persona";
  context: DraftContext;
  initialDraft: RuleDoc[] | string;
  acceptLabel?: string;
  onGenerate: (input: DraftInput) => Promise<DraftResponse>;
  onRewrite: (feedback: string, draftId: string, currentText: string) => Promise<DraftResponse>;
  onAccept: (draft: RuleDoc[] | string) => void | Promise<void>;
  onCancel: () => void;
}

function rulesText(rules: RuleDoc[]): string {
  return rules.map((rule) => `=== FILE: ${rule.filename} ===\n${rule.content}`).join("\n") + "\n=== END ===";
}

export function AssistantDraftDialog({
  kind,
  context,
  initialDraft,
  acceptLabel = "应用到编辑器",
  onGenerate,
  onRewrite,
  onAccept,
  onCancel,
}: Props) {
  const [rules, setRules] = useState<RuleDoc[]>(Array.isArray(initialDraft) ? initialDraft.map((r) => ({ ...r })) : []);
  const [persona, setPersona] = useState(typeof initialDraft === "string" ? initialDraft : "");
  const [feedback, setFeedback] = useState("");
  const [rawText, setRawText] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [ready, setReady] = useState(false);
  const [hasSession, setHasSession] = useState(false);
  const activeDraft = useRef<string | null>(null);
  const activeRun = useRef<string | null>(null);
  const finishedRuns = useRef(new Set<string>());
  const sequence = useRef(0);
  const inFlight = useRef(false);

  useEffect(() => {
    const id = crypto.randomUUID();
    activeDraft.current = id;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    api
      .listenAssistantEvent((event) => {
        if (disposed || !inFlight.current || activeDraft.current !== event.draftId || finishedRuns.current.has(event.runId))
          return;
        if (activeRun.current !== null && activeRun.current !== event.runId) return;
        if (event.seq <= sequence.current) return;
        activeRun.current = event.runId;
        sequence.current = event.seq;
        setRawText((current) => current + event.text);
      })
      .then((remove) => {
        if (disposed) remove();
        else {
          unlisten = remove;
          setReady(true);
        }
      })
      .catch((failure) => {
        if (!disposed) setError(String(failure));
      });
    return () => {
      disposed = true;
      unlisten?.();
      if (activeDraft.current === id) activeDraft.current = null;
      void api.cancelDraft(id).catch(() => {});
    };
  }, []);

  const currentText = kind === "rules" ? rulesText(rules) : persona;

  async function generate(rewrite: boolean) {
    const id = activeDraft.current;
    if (!ready || !id || inFlight.current) return;
    if (rewrite && !canSendFeedback(feedback, busy, false)) return;
    inFlight.current = true;
    activeRun.current = null;
    sequence.current = 0;
    setBusy(true);
    setError(null);
    setRawText("");
    try {
      const response = rewrite
        ? await onRewrite(feedback.trim(), id, currentText)
        : await onGenerate({ ...context, draftId: id, feedback: null, currentText: currentText.trim() ? currentText : null });
      if (!acceptDraftResponse(activeDraft.current, response.draftId, response)) return;
      finishedRuns.current.add(response.runId);
      setRawText(response.rawText);
      setHasSession(response.sessionId !== null);
      setError(response.error);
      if (kind === "rules" && Array.isArray(response.draft)) setRules(response.draft);
      if (kind === "persona" && typeof response.draft === "string") setPersona(response.draft);
    } catch (failure) {
      if (activeDraft.current === id) setError(String(failure));
    } finally {
      if (activeDraft.current === id) {
        inFlight.current = false;
        setBusy(false);
      }
    }
  }

  function close() {
    const id = activeDraft.current;
    activeDraft.current = null;
    inFlight.current = false;
    if (id) void api.cancelDraft(id).catch(() => {});
    onCancel();
  }

  const canApply =
    kind === "rules"
      ? canApplyRules(rules, busy, activeDraft.current === null)
      : canApplyPersona(persona, busy, activeDraft.current === null);

  async function accept() {
    if (!canApply) return;
    setBusy(true);
    try {
      await onAccept(kind === "rules" ? rules : persona);
      close();
    } catch (failure) {
      setError(String(failure));
      setBusy(false);
    }
  }

  return (
    <Modal title={kind === "rules" ? "助理起草规章" : "助理起草人设"} onClose={close}>
      <div className="space-y-4">
        <p className="text-xs leading-5 text-ink-secondary">
          助理以讨论模式生成草稿；CLI 继承原生配置，不构成文件硬隔离。确认后才应用内容。
        </p>
        <div className="flex items-center gap-3">
          <Button disabled={!ready || busy} onClick={() => generate(false)}>
            {busy ? "正在生成…" : hasSession ? "重新生成（新会话）" : "生成草稿"}
          </Button>
          {busy && (
            <span role="status" className="text-xs font-medium text-accent">
              可随时关闭并停止生成
            </span>
          )}
        </div>
        {kind === "rules" ? (
          <RulesEditor rules={rules} templates={[]} onChange={setRules} />
        ) : (
          <PersonaEditor value={persona} onChange={setPersona} />
        )}
        <Field label="修改意见">
          <textarea
            className={inputClass}
            rows={2}
            value={feedback}
            onChange={(event) => setFeedback(event.target.value)}
            placeholder="说明希望助理调整的内容"
          />
        </Field>
        <Button
          disabled={!hasSession || !canSendFeedback(feedback, busy, activeDraft.current === null)}
          onClick={() => generate(true)}
        >
          按反馈重写
        </Button>
        {(rawText || busy) && (
          <details open={busy} className="rounded-lg border border-line bg-surface-subtle p-3 text-xs">
            <summary className="cursor-pointer font-medium text-ink-secondary">
              原始输出{busy ? "（接收中）" : ""}
            </summary>
            <pre className="mt-2 max-h-52 overflow-auto whitespace-pre-wrap break-words font-mono text-ink-muted">
              {rawText || "等待 CLI 输出…"}
            </pre>
          </details>
        )}
        {error && (
          <p role="alert" className="rounded-lg border border-danger-border bg-danger-soft px-3 py-2 text-xs text-danger">
            {error}
          </p>
        )}
        <footer className="flex justify-end gap-2 border-t border-line pt-4">
          <Button onClick={close}>{busy ? "停止并关闭" : "取消"}</Button>
          <Button variant="primary" disabled={!canApply} onClick={accept}>
            {acceptLabel}
          </Button>
        </footer>
      </div>
    </Modal>
  );
}
