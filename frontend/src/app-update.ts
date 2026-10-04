import { isRecord } from "./types";

export type UpdatePosition = Readonly<{ key: string; position: { anchorId: string | null; anchorOffset: number; distanceFromBottom: number; sequence?: number } }>;
export type UpdateState = Readonly<{ busy: boolean; message: string; error: boolean }>;
type Resume = { version: string; owner: string; positions: readonly UpdatePosition[] };
const receiptKey = "sproyt.app-update.v1";
const timeoutMs = 15_000;

export function appVersion(document: Document, base: string): string {
  const script = [...document.querySelectorAll<HTMLScriptElement>('script[type="module"][src]')]
    .map(element => new URL(element.getAttribute("src")!, base))
    .find(url => url.origin === new URL(base).origin && /^\/assets\/app\/[a-zA-Z0-9_-]+\/app\.js$/.test(url.pathname));
  if (!script) throw new Error("Kunne ikkje kontrollere appversjonen. Prøv igjen.");
  return script.pathname;
}

/** One deliberate reload, with a receipt checked by the next page. Never clears
 * drafts, authentication, the durable outbox or protocol admissions. */
export function createAppUpdate(options: {
  currentVersion: string;
  storage: Storage;
  latestVersion: () => Promise<string>;
  updateWorker: () => Promise<void>;
  prepare: () => Promise<{ owner: string; positions: readonly UpdatePosition[] }>;
  reload: () => void;
}) {
  let state: UpdateState = { busy: false, message: "", error: false };
  let resume: Resume | null = null;
  const listeners = new Set<() => void>();
  const publish = (next: UpdateState) => { state = next; listeners.forEach(listener => listener()); };
  try {
    const saved: unknown = JSON.parse(options.storage.getItem(receiptKey) || "null");
    if (isRecord(saved) && typeof saved.version === "string" && typeof saved.owner === "string" && Array.isArray(saved.positions)
      && saved.positions.every(entry => isRecord(entry) && typeof entry.key === "string" && isRecord(entry.position)
        && (entry.position.anchorId === null || typeof entry.position.anchorId === "string")
        && typeof entry.position.anchorOffset === "number" && Number.isFinite(entry.position.anchorOffset)
        && typeof entry.position.distanceFromBottom === "number" && Number.isFinite(entry.position.distanceFromBottom)
        && (entry.position.sequence === undefined || Number.isSafeInteger(entry.position.sequence)))) {
      resume = saved as Resume;
      state = saved.version === options.currentVersion
        ? { busy: false, error: false, message: "Appen er oppdatert. Samtalane blir lasta på nytt." }
        : { busy: false, error: true, message: "Appen vart lasta på nytt, men appversjonen kunne ikkje stadfestast. Prøv igjen." };
    }
    options.storage.removeItem(receiptKey);
  } catch { /* No update was requested in this session, or storage is unavailable. */ }
  return Object.freeze({
    getSnapshot: () => state,
    subscribe(listener: () => void) { listeners.add(listener); return () => { listeners.delete(listener); }; },
    resumePosition(owner: string, key: string) { return resume?.owner === owner ? resume.positions.find(entry => entry.key === key)?.position ?? null : null; },
    resumeThread(owner: string, channelId: string) {
      return resume?.owner === owner && resume.positions.some(entry => entry.key === `channel:${channelId}`)
        ? resume.positions.find(entry => entry.key.startsWith("thread:"))?.key.slice(7) ?? null : null;
    },
    async run(capture: () => readonly UpdatePosition[] = () => []) {
      if (state.busy) return;
      publish({ busy: true, error: false, message: "Kontrollerer appversjonen …" });
      try {
        // First protect local data; check again immediately before navigation
        // because uploads or edits can begin while the network check runs.
        await options.prepare();
        const version = await options.latestVersion();
        publish({ busy: true, error: false, message: "Hentar oppdateringa …" });
        await options.updateWorker();
        const context = await options.prepare();
        const receipt = JSON.stringify({ version, owner: context.owner, positions: [...context.positions, ...capture()] });
        options.storage.setItem(receiptKey, receipt);
        if (options.storage.getItem(receiptKey) !== receipt) throw new Error("Utkasta kunne ikkje vernast før oppdatering. Prøv igjen når lagring er tilgjengeleg.");
        publish({ busy: true, error: false, message: "Lastar appen og samtalane på nytt …" });
        options.reload();
      } catch (error) {
        publish({ busy: false, error: true, message: error instanceof Error ? error.message : "Oppdateringa feila. Prøv igjen." });
      }
    }
  });
}

export type AppUpdate = ReturnType<typeof createAppUpdate>;

export async function latestAppVersion(): Promise<string> {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  try {
    const url = new URL(location.href);
    // The version check needs only the page path and same-origin cookies.
    // Keep invitation, navigation and other query values out of this request.
    url.search = ""; url.hash = "";
    const response = await fetch(url, { cache: "no-store", credentials: "same-origin", signal: controller.signal });
    if (!response.ok || !response.headers.get("content-type")?.includes("text/html")
      || new URL(response.url).origin !== url.origin || new URL(response.url).pathname !== url.pathname)
      throw new Error("Kunne ikkje hente appversjonen. Kontroller sambandet og prøv igjen.");
    return appVersion(new DOMParser().parseFromString(await response.text(), "text/html"), url.href);
  } catch (error) {
    if (controller.signal.aborted) throw new Error("Oppdateringa tok for lang tid. Prøv igjen.");
    throw error;
  } finally { clearTimeout(timer); }
}

export async function updateAppWorker(ready: Promise<ServiceWorkerRegistration | null>, deadlineMs = timeoutMs): Promise<void> {
  if (!("serviceWorker" in navigator)) return;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let cancelled = false;
  let stopListening: (() => void) | undefined;
  try {
    await Promise.race([
      (async () => {
        const registration = await ready;
        if (!registration || cancelled) return;
        await registration.update();
        if (cancelled) return;
        const worker = registration.installing ?? registration.waiting;
        if (!worker || worker.state === "activated") return;
        await new Promise<void>((resolve, reject) => {
          const changed = () => {
            if (worker.state === "activated" || worker.state === "redundant") {
              worker.removeEventListener("statechange", changed);
              if (worker.state === "activated") resolve();
              else reject(new Error("Oppdateringa kunne ikkje aktiverast. Prøv igjen."));
            }
          };
          worker.addEventListener("statechange", changed); changed();
          stopListening = () => worker.removeEventListener("statechange", changed);
        });
      })(),
      new Promise<never>((_, reject) => { timer = setTimeout(() => reject(new Error("Oppdateringa tok for lang tid. Prøv igjen.")), deadlineMs); })
    ]);
  } finally { cancelled = true; clearTimeout(timer); stopListening?.(); }
}
