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
