import type { ServerEvent } from "./types";

type ApplicationState = Readonly<{
  session: Readonly<{ refreshDueAt: number }>;
  connection: Readonly<{ connected: boolean; status: string }>;
  transport: Readonly<{ lastEventType: ServerEvent["type"] | null; processedEvents: number }>;
}>;

type ApplicationStore = Readonly<{
  readonly snapshot: ApplicationState;
  updateSession(patch: Partial<ApplicationState["session"]>): void;
  updateConnection(patch: Partial<ApplicationState["connection"]>): void;
  reduceServerEvent(event: ServerEvent): ServerEvent;
}>;

export function createApplicationStore(): ApplicationStore {
  let state: ApplicationState = Object.freeze({
    session: Object.freeze({ refreshDueAt: 0 }),
    connection: Object.freeze({ connected: false, status: "Koplar til …" }),
    transport: Object.freeze({ lastEventType: null, processedEvents: 0 })
  });

  return Object.freeze({
    get snapshot(): ApplicationState { return state; },
    updateSession(patch: Partial<ApplicationState["session"]>): void {
      state = Object.freeze({ ...state, session: Object.freeze({ ...state.session, ...patch }) });
    },
    updateConnection(patch: Partial<ApplicationState["connection"]>): void {
      state = Object.freeze({ ...state, connection: Object.freeze({ ...state.connection, ...patch }) });
    },
    reduceServerEvent(event: ServerEvent): ServerEvent {
      state = Object.freeze({ ...state, transport: Object.freeze({ lastEventType: event.type, processedEvents: state.transport.processedEvents + 1 }) });
      return event;
    }
  });
}

type MailboxOptions<TEvent> = Readonly<{
  reduce(event: TEvent): TEvent;
  deliver(event: TEvent): void;
}>;

type ServerEventMailbox<TEvent> = Readonly<{
  enqueue(event: TEvent): void;
  readonly size: number;
}>;

export function createServerEventMailbox<TEvent>({
  reduce,
  deliver
}: MailboxOptions<TEvent>): ServerEventMailbox<TEvent> {
  const queue: TEvent[] = [];
  let draining = false;
  return Object.freeze({
    enqueue(event: TEvent): void {
      queue.push(event);
      if (draining) return;
      draining = true;
      try {
        while (queue.length > 0) {
          const nextEvent = queue.shift();
          if (nextEvent === undefined) break;
          deliver(reduce(nextEvent));
        }
      } finally {
        draining = false;
      }
    },
    get size(): number { return queue.length; }
  });
}
