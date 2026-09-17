export interface TimelineScrollModel {
  readonly key: string | null;
  readonly messageIds: readonly string[];
  readonly hasOlder?: boolean;
  readonly revealMessageId?: string | null;
}

interface ReadingPosition {
  readonly anchorId: string | null;
  readonly anchorOffset: number;
  readonly distanceFromBottom: number;
}

interface PendingRestore {
  readonly keyChanged: boolean;
  readonly position: ReadingPosition | null;
  readonly forceBottom: boolean;
  readonly revealMessageId: string | null;
}

export interface TimelineScrollControllerOptions {
  readonly onNearStart?: () => void;
  readonly onReachedBottom?: (lastMessageId: string | null) => void;
  readonly nearEdge?: number;
}

/**
 * Owns reading position outside the React presentation tree. The controller is
 * deliberately transport-free: the host supplies pagination/read callbacks and
 * calls prepare before it publishes the next immutable timeline snapshot.
 */
export function createTimelineScrollController(options: TimelineScrollControllerOptions = {}) {
  const nearEdge = options.nearEdge ?? 80;
  const positions = new Map<string, ReadingPosition>();
  let viewport: HTMLDivElement | null = null;
  let model: TimelineScrollModel = { key: null, messageIds: [] };
  let pending: PendingRestore | null = null;
  let mutationObserver: MutationObserver | null = null;
  let resizeObserver: ResizeObserver | null = null;
  let scheduled = false;
  let applying = false;
  let followBottom = true;
  let revealMessageId: string | null = null;
  let requestedOlderAt: string | null = null;
  let reportedBottomAt: string | null = null;

  const messageElement = (id: string): HTMLElement | null => {
    if (!viewport) return null;
    return [...viewport.querySelectorAll<HTMLElement>("[data-message-id]")]
      .find(element => element.dataset.messageId === id) ?? null;
  };

  const capture = (): ReadingPosition | null => {
    if (!viewport) return null;
    const viewportRect = viewport.getBoundingClientRect();
    const anchor = [...viewport.querySelectorAll<HTMLElement>("[data-message-id]")]
      .find(element => element.getBoundingClientRect().bottom > viewportRect.top + 1) ?? null;
    return {
      anchorId: anchor?.dataset.messageId ?? null,
      anchorOffset: anchor ? anchor.getBoundingClientRect().top - viewportRect.top : 0,
      distanceFromBottom: Math.max(0, viewport.scrollHeight - viewport.scrollTop - viewport.clientHeight)
    };
  };

  const save = () => {
    if (!model.key) return null;
    const position = capture();
    if (position) positions.set(model.key, position);
    return position;
  };

  const setScrollTop = (value: number) => {
    if (!viewport) return;
    applying = true;
    viewport.scrollTop = Math.max(0, value);
    queueMicrotask(() => { applying = false; });
  };

  const scrollToBottom = () => {
    if (viewport) setScrollTop(viewport.scrollHeight - viewport.clientHeight);
  };

  const restoreAnchor = (position: ReadingPosition): boolean => {
    if (!viewport || !position.anchorId) return false;
    const anchor = messageElement(position.anchorId);
    if (!anchor) return false;
    const delta = anchor.getBoundingClientRect().top - viewport.getBoundingClientRect().top - position.anchorOffset;
    if (Math.abs(delta) > 0.5) setScrollTop(viewport.scrollTop + delta);
    return true;
  };

  const reportBottom = () => {
    if (!viewport || !model.key) return;
    const distance = viewport.scrollHeight - viewport.scrollTop - viewport.clientHeight;
    if (distance > nearEdge) return;
    const lastMessageId = model.messageIds.at(-1) ?? null;
    if (reportedBottomAt === `${model.key}:${lastMessageId ?? ""}`) return;
    reportedBottomAt = `${model.key}:${lastMessageId ?? ""}`;
    options.onReachedBottom?.(lastMessageId);
  };

  const reconcile = () => {
    scheduled = false;
    if (!viewport || !model.key) return;
    if (pending) {
      const restore = pending;
      pending = null;
      revealMessageId = restore.revealMessageId;
      if (revealMessageId && messageElement(revealMessageId)) {
        scrollToBottom();
        followBottom = true;
      }
      else if (restore.forceBottom) scrollToBottom();
      else if (restore.position && !restoreAnchor(restore.position)) {
        setScrollTop(viewport.scrollHeight - viewport.clientHeight - restore.position.distanceFromBottom);
      }
    } else if (revealMessageId && revealMessageId === model.messageIds.at(-1)) {
      scrollToBottom();
    } else if (followBottom) {
      scrollToBottom();
    } else {
      const position = positions.get(model.key);
      if (position) restoreAnchor(position);
    }
    save();
    reportBottom();
  };

  const scheduleReconcile = () => {
    if (scheduled) return;
    scheduled = true;
    requestAnimationFrame(reconcile);
  };

  const observeMessages = () => {
    resizeObserver?.disconnect();
    if (!viewport || typeof ResizeObserver !== "function") return;
    resizeObserver = new ResizeObserver(scheduleReconcile);
    for (const message of viewport.querySelectorAll<HTMLElement>("[data-message-id]")) resizeObserver.observe(message);
  };

  const onScroll = () => {
    if (!viewport || applying) return;
    const position = save();
    if (!position) return;
    followBottom = position.distanceFromBottom <= nearEdge;
    if (!followBottom) revealMessageId = null;
    if (viewport.scrollTop <= nearEdge && model.hasOlder) {
      const oldest = model.messageIds[0] ?? "";
      if (requestedOlderAt !== `${model.key}:${oldest}`) {
        requestedOlderAt = `${model.key}:${oldest}`;
        options.onNearStart?.();
      }
    } else if (viewport.scrollTop > nearEdge * 2) {
      requestedOlderAt = null;
    }
    reportBottom();
  };

  const viewportRef = (element: HTMLDivElement | null) => {
    if (viewport === element) return;
    if (viewport) {
      save();
    }
    mutationObserver?.disconnect();
    resizeObserver?.disconnect();
    viewport = element;
    if (!viewport) return;
    mutationObserver = new MutationObserver(() => {
      observeMessages();
      scheduleReconcile();
    });
    mutationObserver.observe(viewport, { childList: true, subtree: true });
    observeMessages();
    scheduleReconcile();
  };

  const prepare = (next: TimelineScrollModel) => {
    const previous = save();
    const keyChanged = model.key !== next.key;
    const stored = next.key ? positions.get(next.key) ?? null : null;
    const appended = !keyChanged && next.messageIds.at(-1) !== model.messageIds.at(-1);
    // Several host publications can be coalesced into one concurrent React
    // commit (for example accepted reply + cleared composer). Keep a reveal
    // intent until the corresponding message has reached the DOM.
    const carriedReveal = pending?.revealMessageId ?? revealMessageId;
    const explicitReveal = next.revealMessageId
      ?? (carriedReveal && next.messageIds.includes(carriedReveal) ? carriedReveal : null);
    const wasNearBottom = previous ? previous.distanceFromBottom <= nearEdge : true;
    pending = {
      keyChanged,
      position: keyChanged ? stored : previous,
      forceBottom: keyChanged ? !stored : (Boolean(explicitReveal) || (appended && wasNearBottom)),
      revealMessageId: explicitReveal
    };
    followBottom = pending.forceBottom || (pending.position?.distanceFromBottom ?? 0) <= nearEdge;
    if (keyChanged || next.messageIds[0] !== model.messageIds[0]) requestedOlderAt = null;
    if (keyChanged) reportedBottomAt = null;
    model = next;
    scheduleReconcile();
  };

  const dispose = () => {
    mutationObserver?.disconnect();
    resizeObserver?.disconnect();
    viewport = null;
  };

  return Object.freeze({ prepare, viewportRef, onScroll, dispose });
}

export type TimelineScrollController = ReturnType<typeof createTimelineScrollController>;
