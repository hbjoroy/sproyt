import { createApplicationStore, createServerEventMailbox } from "../client-store";
import type { ServerEvent } from "../types";

export type RuntimeSnapshot = ReturnType<typeof createApplicationStore>["snapshot"];

/** The presentation-independent status/event boundary. Construct once for the
 * page, never in a component render. Rendering subscribes; it cannot create a
 * second transport, session controller, outbox, or polling loop here.
 */
export function createApplicationRuntime(deliver: (event: ServerEvent) => void) {
  const state = createApplicationStore();
  const listeners = new Set<() => void>();
  let disposed = false;
  const publish = () => {
    for (const listener of [...listeners]) listener();
  };
  const mailbox = createServerEventMailbox({
    reduce: state.reduceServerEvent,
    deliver(event: ServerEvent) {
      if (disposed) return;
      deliver(event);
      publish();
    }
  });
  const store = Object.freeze({
    get snapshot(): RuntimeSnapshot { return state.snapshot; },
    updateSession(patch: Partial<RuntimeSnapshot["session"]>) {
      if (disposed) return;
      state.updateSession(patch);
      publish();
    },
    updateConnection(patch: Partial<RuntimeSnapshot["connection"]>) {
      if (disposed) return;
      state.updateConnection(patch);
      publish();
    }
  });

  return Object.freeze({
    store,
    getSnapshot: (): RuntimeSnapshot => state.snapshot,
    subscribe(listener: () => void): () => void {
      if (disposed) return () => {};
      listeners.add(listener);
      return () => { listeners.delete(listener); };
    },
    enqueue(event: ServerEvent): void {
      if (!disposed) mailbox.enqueue(event);
    },
    get size(): number { return mailbox.size; },
    /** Releases presentation subscriptions; transport lifecycle stays with its
     * existing owner until that owner is migrated into this boundary. */
    dispose(): void {
      disposed = true;
      listeners.clear();
    }
  });
}

export type ApplicationRuntime = ReturnType<typeof createApplicationRuntime>;
