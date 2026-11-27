import { HttpClient } from "./api";
import type { AvatarActions, AvatarDraft } from "./ui/react/avatar-editor";
import { avatarUrl } from "./ui/react/profile-avatar";
import { isRecord } from "./types";

function decode(value: unknown): AvatarDraft {
  if (!isRecord(value) || typeof value.id !== "string" || typeof value.state !== "string") throw new Error("Ugyldig svar frå avatartenesta.");
  return { id: value.id, state: value.state, ...(value.state === "ready" ? { preview: avatarUrl(`/api/v1/me/avatar/${encodeURIComponent(value.id)}/preview`) } : {}) };
}

export function avatarActions(http: HttpClient, current: () => string | null, epoch: () => number): AvatarActions {
  function fence(owner: string) {
    const generation = epoch();
    return () => current() === owner && epoch() === generation;
  }
  const headers = (owner: string) => ({ "x-sproyt-account": owner });
  return {
    current(owner, signal) {
      return http.json("/api/v1/me/avatar", value => value === null ? null : decode(value), { signal, headers: headers(owner) }, fence(owner));
    },
    generate(owner, photo, style, request) {
      if (photo.size > 8 * 1024 * 1024) return Promise.reject(new Error("Vel eit foto på høgst 8 MiB."));
      return http.json(`/api/v1/me/avatar/generate/${style}/${request}`, decode, { method: "POST", headers: headers(owner), body: photo }, fence(owner));
    },
    retry(owner, id) {
      return http.json(`/api/v1/me/avatar/${encodeURIComponent(id)}/retry`, decode, { method: "POST", headers: headers(owner) }, fence(owner));
    },
    review(owner, id, approve) {
      return http.empty(`/api/v1/me/avatar/${encodeURIComponent(id)}/review`, { method: "POST", headers: { ...headers(owner), "content-type": "application/json" }, body: JSON.stringify({ approve }) }, fence(owner));
    }
  };
}
