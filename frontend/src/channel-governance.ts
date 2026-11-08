import type { HttpClient } from "./api";
import { isRecord, type Channel } from "./types";

export interface ChannelGovernance {
  hidden: boolean;
  events_enabled?: boolean;
  members: Array<{ user_id: string; role: Channel["role"] }>;
}
function decode(value: unknown): ChannelGovernance {
  if (!isRecord(value) || typeof value.hidden !== "boolean" || (value.events_enabled !== undefined && typeof value.events_enabled !== "boolean") || !Array.isArray(value.members)) throw new Error("Ugyldige kanalrettar.");
  return { hidden: value.hidden, events_enabled: value.events_enabled as boolean | undefined, members: value.members.map(member => {
    if (!isRecord(member) || typeof member.user_id !== "string" || !["owner", "moderator", "member", "observer"].includes(String(member.role))) throw new Error("Ugyldig kanalrolle.");
    return { user_id: member.user_id, role: member.role as Channel["role"] };
  }) };
}
export class ChannelGovernanceApi {
  constructor(private readonly http: HttpClient, private readonly owner: () => string | null, private readonly epoch: () => number) {}
  async request(channelId: string, suffix = "/governance", body?: unknown): Promise<ChannelGovernance> {
    const owner = this.owner(), epoch = this.epoch();
    if (!owner) throw new Error("Du må logge inn.");
    const admitted = () => owner === this.owner() && epoch === this.epoch();
    const result = await this.http.json(`/api/v1/channels/${encodeURIComponent(channelId)}${suffix}`, decode, {
      method: body === undefined ? "GET" : "PUT",
      headers: { "x-sproyt-account": owner, ...(body === undefined ? {} : { "content-type": "application/json" }) },
      ...(body === undefined ? {} : { body: JSON.stringify(body) })
    }, admitted);
    if (!admitted()) throw new Error("Kontoen eller sambandet vart endra.");
    return result;
  }
}
