export interface TimelineScrollModel {
  readonly key: string | null;
  readonly messageIds: readonly string[];
  readonly hasOlder?: boolean;
  readonly loading?: boolean;
  readonly error?: string;
  readonly messageSequences?: readonly number[];
  readonly initialReadSequence?: number;
  readonly waitingForLink?: boolean;
  readonly revealMessageId?: string | null;
}

interface ReadingPosition {
  readonly anchorId: string | null;
  readonly anchorOffset: number;
  readonly distanceFromBottom: number;
  readonly sequence?: number;
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
  readonly onVisibleMessages?: (key: string, messageIds: readonly string[]) => void;
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
  const openingReadSequences = new Map<string, number>();
  let opening: { position: ReadingPosition | null; readSequence: number } | null = null;
  let viewport: HTMLElement | null = null;
  let model: TimelineScrollModel = { key: null, messageIds: [] };
  let pending: PendingRestore | null = null;
  let mutationObserver: MutationObserver | null = null;
  let resizeObserver: ResizeObserver | null = null;
  let scheduled = false;
  let applying = false;
  let scrollApplication = 0;
  let appliedScrollTop: number | null = null;
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
    if (!viewportRect.width || !viewportRect.height) return model.key ? positions.get(model.key) ?? null : null;
    const anchor = [...viewport.querySelectorAll<HTMLElement>("[data-message-id]")]
      .find(element => element.getBoundingClientRect().bottom > viewportRect.top + 1) ?? null;
    return {
      anchorId: anchor?.dataset.messageId ?? null,
      anchorOffset: anchor ? anchor.getBoundingClientRect().top - viewportRect.top : 0,
      sequence: anchor ? Number(anchor.dataset.messageSequence) : undefined,
      distanceFromBottom: Math.max(0, viewport.scrollHeight - viewport.scrollTop - viewport.clientHeight)
    };
  };

  const save = () => {
    if (!model.key) return null;
    const position = capture();
    if (position?.anchorId && !opening) positions.set(model.key, position);
    return position;
  };

  const setScrollTop = (value: number) => {
    if (!viewport) return;
    applying = true;
    const application = ++scrollApplication;
    viewport.scrollTop = Math.max(0, value);
    appliedScrollTop = viewport.scrollTop;
    // WebKit delivers scroll events in the rendering step, after microtasks.
    // Keep restored positions separate from user scrolls through that step so
    // an older-page restore cannot trigger another fetch or overwrite its anchor.
    requestAnimationFrame(() => { if (application === scrollApplication) applying = false; });
  };

  const scrollToBottom = () => {
    if (viewport) setScrollTop(viewport.scrollHeight - viewport.clientHeight);
  };

  const scrollToMessage = (id: string, context = false): boolean => {
    const message = messageElement(id);
    if (!viewport || !message) return false;
    const offset = context ? Math.min(120, viewport.clientHeight * 0.25) : Math.min(24, viewport.clientHeight * 0.1);
    setScrollTop(viewport.scrollTop + message.getBoundingClientRect().top - viewport.getBoundingClientRect().top - offset);
    return true;
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

  const reportVisible = () => {
    if (!viewport || !model.key || opening || model.waitingForLink || model.loading
      || document.visibilityState === "hidden" || !document.hasFocus()
      || (document.querySelector("dialog[open]") && !viewport.closest("dialog[open]"))) return;
    const bounds = viewport.getBoundingClientRect();
    if (!bounds.width || !bounds.height || !viewport.getClientRects().length) return;
    const ids = [...viewport.querySelectorAll<HTMLElement>("[data-message-id]")].filter(element => {
      const rect = element.getBoundingClientRect();
      const overlap = Math.min(rect.bottom, bounds.bottom, window.innerHeight) - Math.max(rect.top, bounds.top, 0);
      return rect.width > 0 && overlap >= Math.min(24, rect.height);
    }).map(element => element.dataset.messageId!);
    options.onVisibleMessages?.(model.key, ids);
  };

  const requestOlder = () => {
    if (!model.hasOlder || model.loading || model.error) return;
    const cursor = `${model.key}:${model.messageIds[0] ?? ""}:${model.messageIds.length}`;
    if (requestedOlderAt === cursor) return;
    requestedOlderAt = cursor;
    options.onNearStart?.();
  };

  const reconcile = () => {
    scheduled = false;
    if (!viewport || !model.key) return;
    const bounds = viewport.getBoundingClientRect();
    if (!bounds.width || !bounds.height) return;
    // Concurrent React publications can precede the matching DOM commit.
    // Retain the target until both ends of this model have reached the viewport.
    if (model.messageIds.length ? !messageElement(model.messageIds[0]!) || !messageElement(model.messageIds.at(-1)!)
      : viewport.querySelector("[data-message-id]")) return;
    // Opening targets survive empty renders, paged loading and reconnects.
    // Never acknowledge the latest page while the intended position is absent.
    if (model.waitingForLink && !pending?.revealMessageId) return;
    if (opening && !pending?.revealMessageId) {
      if (model.loading || model.error) return;
      const target = opening.position;
      if (target?.anchorId) {
        if (!restoreAnchor(target)) {
          if (model.hasOlder) { requestOlder(); return; }
          const closest = model.messageIds.find((_, index) => (model.messageSequences?.[index] ?? 0) >= (target.sequence ?? 0));
          if (closest) scrollToMessage(closest);
        }
        followBottom = (capture()?.distanceFromBottom ?? Infinity) <= nearEdge;
      } else if (!target && model.initialReadSequence !== undefined) {
        const sequences = model.messageSequences ?? [];
        if (model.hasOlder && (sequences[0] ?? Infinity) > opening.readSequence) { requestOlder(); return; }
        const unread = model.messageIds.find((_, index) => sequences[index]! > opening!.readSequence);
        if (unread) { scrollToMessage(unread, true); followBottom = false; }
        else { scrollToBottom(); followBottom = true; }
      } else { scrollToBottom(); followBottom = true; }
      opening = null;
      pending = null;
      save();
      reportVisible();
      return;
    }
    if (pending) {
      const restore = pending;
      pending = null;
      revealMessageId = restore.revealMessageId;
      if (revealMessageId && scrollToMessage(revealMessageId)) {
        opening = null;
        revealMessageId = null;
        followBottom = false;
      }
      else if (restore.forceBottom || (!restore.position && followBottom)) scrollToBottom();
      else if (restore.position && !restoreAnchor(restore.position)) {
        setScrollTop(viewport.scrollHeight - viewport.clientHeight - restore.position.distanceFromBottom);
      }
    } else if (revealMessageId && scrollToMessage(revealMessageId)) {
      revealMessageId = null;
      followBottom = false;
    } else if (followBottom) {
      scrollToBottom();
    } else {
      const position = positions.get(model.key);
      if (position) restoreAnchor(position);
    }
    save();
    reportBottom();
    reportVisible();
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
    resizeObserver.observe(viewport);
    for (const message of viewport.querySelectorAll<HTMLElement>("[data-message-id]")) resizeObserver.observe(message);
  };

  const onScroll = () => {
    if (!viewport || pending || viewport.scrollTop === appliedScrollTop) return;
    appliedScrollTop = null;
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
    reportVisible();
  };

  const viewportRef = (element: HTMLElement | null) => {
    if (viewport === element) return;
    if (viewport && !pending) {
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
    // A resize/mutation reconciliation may still be queued after a React commit.
    // Its DOM geometry is not yet the reader's restored position.
    const previous = (pending || scheduled || applying) && model.key
      ? pending?.position ?? positions.get(model.key) ?? capture()
      : save();
    const keyChanged = model.key !== next.key;
    // Several history responses/publications may arrive before the next frame
    // restores the DOM. Keep the original reading anchor through that batch.
    const position = !keyChanged && pending ? pending.position : previous;
    const stored = next.key ? positions.get(next.key) ?? null : null;
    if (keyChanged) {
      opening = next.key ? { position: stored, readSequence: next.initialReadSequence ?? 0 } : null;
      if (next.key && !openingReadSequences.has(next.key)) openingReadSequences.set(next.key, next.initialReadSequence ?? 0);
    }
    const appended = !keyChanged && next.messageIds.at(-1) !== model.messageIds.at(-1);
    // Several host publications can be coalesced into one concurrent React
    // commit (for example accepted reply + cleared composer). Keep a reveal
    // intent until the corresponding message has reached the DOM.
    const carriedReveal = pending?.revealMessageId ?? revealMessageId;
    const explicitReveal = next.revealMessageId ?? (keyChanged ? null : carriedReveal);
    const wasNearBottom = position ? position.distanceFromBottom <= nearEdge : true;
    pending = {
      keyChanged,
      position: keyChanged ? stored : position,
      forceBottom: keyChanged ? !stored && !explicitReveal : (!explicitReveal && appended && wasNearBottom),
      revealMessageId: explicitReveal
    };
    followBottom = pending.forceBottom || (pending.position?.distanceFromBottom ?? 0) <= nearEdge;
    if (keyChanged || next.messageIds[0] !== model.messageIds[0]) requestedOlderAt = null;
    if (keyChanged) reportedBottomAt = null;
    model = next;
    scheduleReconcile();
  };

  const dispose = () => {
    window.removeEventListener("focus", scheduleReconcile);
    document.removeEventListener("visibilitychange", scheduleReconcile);
    mutationObserver?.disconnect();
    resizeObserver?.disconnect();
    viewport = null;
  };

  window.addEventListener("focus", scheduleReconcile);
  document.addEventListener("visibilitychange", scheduleReconcile);
  const goToLatest = () => {
    opening = null; pending = null; revealMessageId = null; followBottom = true;
    scrollToBottom(); save(); reportVisible();
  };
  const unreadAfterSequence = () => model.key ? openingReadSequences.get(model.key) : undefined;
  return Object.freeze({ prepare, viewportRef, onScroll, goToLatest, unreadAfterSequence, dispose });
}

export type TimelineScrollController = ReturnType<typeof createTimelineScrollController>;
