import { isRecord } from "./types";

export const feedbackStage = {
  linked: "Knytt til eksisterande sak",
  starting: "Startar vurderinga i Heart",
  review: "Til vurdering hos produkteigaren",
  review_submitted: "Vurderinga er registrert",
  waiting_for_close: "Ventar på lukking i GitHub",
  resolved: "Løyst",
  rejected: "Avvist",
  failed: "Heart-prosessen stoppa"
} as const;

export type FeedbackIssue = Readonly<{
  issue_number: number; issue_url: string; publication_kind: "create" | "comment" | "duplicate";
  published_title: string | null; published_body: string; stage: keyof typeof feedbackStage;
  heart_instance_id: string | null; can_review: boolean; github_state: string | null;
  github_proposed?: boolean | null; github_status_name?: string | null;
}>;

export function decodeFeedbackIssue(value: unknown): FeedbackIssue {
  if (!isRecord(value) || typeof value.issue_number !== "number" || !Number.isSafeInteger(value.issue_number)
    || value.issue_number < 1 || typeof value.issue_url !== "string"
    || !/^https:\/\/github\.com\/[\w.-]+\/[\w.-]+\/issues\/[1-9]\d*$/u.test(value.issue_url)
    || !value.issue_url.endsWith(`/issues/${value.issue_number}`)
    || !["create", "comment", "duplicate"].includes(String(value.publication_kind))
    || typeof value.stage !== "string" || !Object.hasOwn(feedbackStage, value.stage)
    || typeof value.can_review !== "boolean" || typeof value.published_body !== "string"
    || (value.github_proposed != null && typeof value.github_proposed !== "boolean")
    || (value.github_status_name != null && typeof value.github_status_name !== "string")
    || ![value.published_title, value.github_state, value.heart_instance_id].every(field => field === null || typeof field === "string")) {
    throw new Error("Kunne ikkje lese GitHub-saka for meldinga.");
  }
  return value as FeedbackIssue;
}

export function feedbackIssuePath(channelId: string, messageId: string): string {
  return `/api/v1/channels/${encodeURIComponent(channelId)}/messages/${encodeURIComponent(messageId)}/feedback-issue`;
}

export const activationLabels = {
  available: "Aktiver tilbakemeldinga",
  pending: "Meldinga er i kø for vurdering",
  ready: "Meldinga er vurdert",
  sending: "Sender til GitHub",
  uncertain: "Kontrollerer publiseringa i GitHub",
  published: "Publisert i GitHub",
  done: "Meldinga er behandla",
  ignored: "Vurdert utan å opprette sak",
  needs_review: "Treng manuell vurdering",
} as const;

export type FeedbackActivation = keyof typeof activationLabels;
export function decodeFeedbackActivation(value: unknown): FeedbackActivation | null {
  if (value === null) return null;
  if (typeof value === "string" && Object.hasOwn(activationLabels, value)) return value as FeedbackActivation;
  throw new Error("Kunne ikkje lese status for tilbakemeldinga.");
}

export const intakeStage = {
  starting: "Startar tilbakemeldingsprosessen",
  qualify: "Vurderer om meldinga skal til GitHub",
  match: "Samanliknar med opne saker",
  clarify: "Til manuell avklaring",
  "clarify-publication": "Avklar endra grunnlag for publisering",
  "final-clarify": "Avklar avslutting etter endringar i GitHub",
  "retry-create": "Opprettar avklara GitHub-sak",
  "retry-comment": "Publiserer avklara kommentar",
  "retry-link": "Knyter til avklara GitHub-sak",
  create: "Opprettar ny GitHub-sak",
  comment: "Legg til kommentar i GitHub",
  link: "Knyter til eksisterande sak",
  ack: "Kvitterer i tråden",
  review: feedbackStage.review,
  "await-github": feedbackStage.waiting_for_close,
  ignored: "Avslutta utan GitHub-sak",
  linked: feedbackStage.linked,
  resolved: feedbackStage.resolved,
  rejected: feedbackStage.rejected,
  failed: feedbackStage.failed,
} as const;

export type FeedbackIntake = Readonly<{
  stage: keyof typeof intakeStage; source_body: string; heart_instance_id: string | null;
  clarification_task_id: string | null; review_task_id: string | null;
  reason: string | null; candidates: readonly Readonly<{ number: number; title: string }>[];
  can_clarify: boolean; can_review: boolean; dismiss_only?: boolean;
}>;

export type Clarification = Readonly<{
  task_id: string; decision: "create" | "existing" | "dismiss"; issue_number: number | null; note: string;
}>;

export type FeedbackPanel = Readonly<{ issue: FeedbackIssue | null; intake: FeedbackIntake | null }>;

export function decodeFeedbackIntake(value: unknown): FeedbackIntake {
  if (!isRecord(value) || typeof value.stage !== "string" || !Object.hasOwn(intakeStage, value.stage)
    || typeof value.source_body !== "string" || typeof value.can_clarify !== "boolean" || typeof value.can_review !== "boolean"
    || ![value.heart_instance_id, value.clarification_task_id, value.review_task_id, value.reason].every(field => field === null || typeof field === "string")
    || (value.dismiss_only !== undefined && typeof value.dismiss_only !== "boolean")
    || (value.can_clarify && (!value.clarification_task_id || !["clarify", "clarify-publication", "final-clarify"].includes(value.stage)))
    || (value.can_review && (!value.review_task_id || value.stage !== "review"))
    || !Array.isArray(value.candidates) || !value.candidates.every(candidate => isRecord(candidate) && typeof candidate.title === "string"
      && typeof candidate.number === "number" && Number.isSafeInteger(candidate.number) && candidate.number > 0)) {
    throw new Error("Kunne ikkje lese tilbakemeldingsprosessen.");
  }
  return value as FeedbackIntake;
}
