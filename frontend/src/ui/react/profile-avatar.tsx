import { useEffect, useState, type ReactNode } from "react";

export function avatarUrl(path: string): string {
  const url = new URL(path, location.origin);
  const participant = new URLSearchParams(location.search).get("participant");
  if (participant) url.searchParams.set("participant", participant);
  return url.pathname + url.search;
}

const revisions = new Map<string, string>();
export function avatarChanged(person: string, revision: string) {
  revisions.set(person, revision);
  window.dispatchEvent(new CustomEvent("sproyt-avatar", { detail: person }));
}

export function ProfileAvatar({ person, fallback }: { person: string; fallback?: ReactNode }) {
  const [revision, setRevision] = useState(() => revisions.get(person) ?? "");
  useEffect(() => {
    const refresh = (event: Event) => {
      if ((event as CustomEvent<string>).detail === person) setRevision(revisions.get(person) ?? "");
    };
    window.addEventListener("sproyt-avatar", refresh);
    return () => window.removeEventListener("sproyt-avatar", refresh);
  }, [person]);
  const src = avatarUrl(`/api/v1/people/${encodeURIComponent(person)}/avatar${revision ? "?v=" + encodeURIComponent(revision) : ""}`);
  const [loaded, setLoaded] = useState("");
  return <>{loaded !== src && fallback}<img className="sp-profile-avatar" src={src} alt="" hidden={loaded !== src}
    onLoad={() => setLoaded(src)} onError={() => setLoaded("")} /></>;
}
