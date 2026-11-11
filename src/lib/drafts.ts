import type { DraftInput, RuleDoc } from "../types";
import { isPersonaComplete, parsePersona } from "./persona";
import { validateRules } from "./wizard";

export function canApplyRules(rules: RuleDoc[], busy: boolean, closed: boolean): boolean {
  return !busy && !closed && validateRules(rules) === null;
}

export function canApplyPersona(persona: string, busy: boolean, closed: boolean): boolean {
  return !busy && !closed && isPersonaComplete(parsePersona(persona));
}

export function canSendFeedback(feedback: string, busy: boolean, closed: boolean): boolean {
  return !busy && !closed && feedback.trim().length > 0;
}

export function acceptDraftResponse<T>(activeDraftId: string | null, responseDraftId: string, value: T): T | null {
  return activeDraftId !== null && activeDraftId === responseDraftId ? value : null;
}

export function feedbackInput(base: Omit<DraftInput, "draftId" | "feedback" | "currentText">,
  draftId: string, currentText: string, feedback: string): DraftInput {
  return { ...base, draftId, currentText, feedback };
}
