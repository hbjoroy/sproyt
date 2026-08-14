type ApplicationState = Readonly<{
  session: Readonly<{ refreshDueAt: number }>;
  connection: Readonly<{ connected: boolean; status: string }>;
  transport: Readonly<{ lastEventType: unknown; processedEvents: number }>;
}>;

type StateSlice = "session" | "connection" | "transport";

type ServerEvent = Readonly<{ type?: unknown }>;

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

  const replaceSlice = <TSlice extends StateSlice>(
    slice: TSlice,
    patch: Partial<ApplicationState[TSlice]>
  ): void => {
    state = Object.freeze({
      ...state,
      [slice]: Object.freeze({ ...state[slice], ...patch })
    }) as ApplicationState;
  };

  return Object.freeze({
    get snapshot(): ApplicationState { return state; },
    updateSession(patch: Partial<ApplicationState["session"]>): void {
      replaceSlice("session", patch);
    },
    updateConnection(patch: Partial<ApplicationState["connection"]>): void {
      replaceSlice("connection", patch);
    },
    reduceServerEvent(event: ServerEvent): ServerEvent {
      replaceSlice("transport", {
        lastEventType: event.type || null,
        processedEvents: state.transport.processedEvents + 1
      });
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
          const nextEvent = queue.shift() as TEvent;
          deliver(reduce(nextEvent));
        }
      } finally {
        draining = false;
      }
    },
    get size(): number { return queue.length; }
  });
}
