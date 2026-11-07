import type { HttpClient } from "./api";
import { isRecord } from "./types";

export type ChannelReference = Readonly<{ id: string; slug: string; name: string; is_member: boolean }>;
type ReferenceScope = Readonly<{ owner: string; epoch: number; channelId: string }>;

export class ChannelReferenceApi {
  constructor(private readonly http: HttpClient) {}

  resolve(scope: ReferenceScope, slugs: string[], signal: AbortSignal, admitted: () => boolean) {
    return this.http.json(`/api/v1/channels/${encodeURIComponent(scope.channelId)}/channel-references`, value => {
      if (!isRecord(value) || !Array.isArray(value.channels)) throw new Error("Ugyldige kanallenker");
      return value.channels.map((channel: unknown): ChannelReference => {
        if (!isRecord(channel) || typeof channel.id !== "string" || typeof channel.slug !== "string"
          || typeof channel.name !== "string" || typeof channel.is_member !== "boolean")
          throw new Error("Ugyldig kanallenkje");
        return { id: channel.id, slug: channel.slug, name: channel.name, is_member: channel.is_member };
      });
    }, {
      method: "POST", body: JSON.stringify({ slugs }), signal,
      headers: { "content-type": "application/json", "x-sproyt-account": scope.owner }
    }, admitted);
  }
}

/** One queue per open conversation, shared by its timeline and threads. */
export class ChannelReferenceDiscovery {
  #active: {
    scope: ReferenceScope;
    controller: AbortController;
    refs: Map<string, ChannelReference | null>;
    pending: Set<string>;
    running: boolean;
  } | null = null;

  constructor(private readonly api: ChannelReferenceApi, private readonly changed: () => void) {}

  scope(scope: ReferenceScope | null) {
    const current = this.#active;
    if (current && scope && current.scope.owner === scope.owner
      && current.scope.epoch === scope.epoch && current.scope.channelId === scope.channelId) return;
    current?.controller.abort();
    this.#active = scope ? { scope, controller: new AbortController(), refs: new Map(), pending: new Set(), running: false } : null;
  }

  find(scope: ReferenceScope, slug: string) {
    const active = this.#active;
    if (active && active.scope.owner === scope.owner && active.scope.epoch === scope.epoch
      && active.scope.channelId === scope.channelId) return active.refs.get(slug) ?? undefined;
  }

  async refresh(channelId: string, channel: ChannelReference, admitted: () => boolean) {
    const active = this.#active;
    const current = () => active === this.#active && admitted();
    if (!active || active.scope.channelId !== channelId || !current()) return;
    const result = await this.api.resolve(active.scope, [channel.slug], active.controller.signal, current);
    if (!current()) return;
    const target = result.find(item => item.slug === channel.slug);
    active.refs.set(channel.slug, target ?? null);
    if (target?.id !== channel.id || target.name !== channel.name || target.is_member !== channel.is_member) this.changed();
    return target?.id === channel.id ? target : undefined;
  }

  collect(channelId: string, slugs: ReadonlySet<string>, admitted: () => boolean) {
    const active = this.#active;
    if (!active || active.scope.channelId !== channelId || !admitted()) return;
    for (const slug of slugs) {
      const cached = active.refs.get(slug);
      if (cached !== undefined) {
        active.refs.delete(slug);
        active.refs.set(slug, cached);
      } else if (active.pending.size < 512) active.pending.add(slug);
    }
    if (active.running || !active.pending.size) return;
    active.running = true;
    queueMicrotask(async () => {
      const current = () => active === this.#active && !active.controller.signal.aborted && admitted();
      try {
        while (current() && active.pending.size) {
          const batch = [...active.pending].slice(0, 64);
          const result = await this.api.resolve(active.scope, batch, active.controller.signal, current);
          if (!current()) return;
          const found = new Map(result.map(channel => [channel.slug, channel]));
          for (const slug of batch) {
            active.pending.delete(slug);
            active.refs.set(slug, found.get(slug) ?? null);
          }
          while (active.refs.size > 512) active.refs.delete(active.refs.keys().next().value!);
          if (found.size) this.changed();
        }
      } catch {
        // Keep these tokens plain for this view. Reopening the conversation retries.
        if (current()) {
          for (const slug of active.pending) active.refs.set(slug, null);
          active.pending.clear();
          while (active.refs.size > 512) active.refs.delete(active.refs.keys().next().value!);
        }
      } finally { active.running = false; }
    });
  }
}
