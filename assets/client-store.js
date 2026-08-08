export function createApplicationStore() {
  let state = Object.freeze({
    session: Object.freeze({ refreshDueAt: 0 }),
    connection: Object.freeze({ connected: false, status: "Koplar til …" }),
    transport: Object.freeze({ lastEventType: null, processedEvents: 0 })
  });

  const replaceSlice = (slice, patch) => {
    state = Object.freeze({
      ...state,
      [slice]: Object.freeze({ ...state[slice], ...patch })
    });
  };

  return Object.freeze({
    get snapshot() { return state; },
    updateSession(patch) { replaceSlice("session", patch); },
    updateConnection(patch) { replaceSlice("connection", patch); },
    reduceServerEvent(event) {
      replaceSlice("transport", {
        lastEventType: event.type || null,
        processedEvents: state.transport.processedEvents + 1
      });
      return event;
    }
  });
}

export function createServerEventMailbox({ reduce, deliver }) {
  const queue = [];
  let draining = false;
  return Object.freeze({
    enqueue(event) {
      queue.push(event);
      if (draining) return;
      draining = true;
      try {
        while (queue.length > 0) deliver(reduce(queue.shift()));
      } finally {
        draining = false;
      }
    },
    get size() { return queue.length; }
  });
}
