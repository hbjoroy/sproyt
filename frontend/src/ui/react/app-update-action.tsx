import { Button, Status } from "@sproyt/ui/react";
import { useSyncExternalStore } from "react";
import type { AppUpdate, UpdatePosition } from "../../app-update";

export function AppUpdateAction({ update, capture }: { update: AppUpdate; capture: () => readonly UpdatePosition[] }) {
  const state = useSyncExternalStore(update.subscribe, update.getSnapshot);
  return <div>
    <Button data-app-update busy={state.busy} disabled={state.busy} onClick={() => void update.run(capture)}>{state.error ? "Prøv oppdateringa igjen" : "Oppdater appen"}</Button>
    {state.message && <Status tone={state.error ? "error" : undefined}>{state.message}</Status>}
  </div>;
}

export function AppUpdateNotice({ update }: { update: AppUpdate }) {
  const state = useSyncExternalStore(update.subscribe, update.getSnapshot);
  return state.message && !state.busy ? <Status tone={state.error ? "error" : undefined}>{state.message}</Status> : null;
}
