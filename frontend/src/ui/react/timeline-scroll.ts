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

export interface ReadingPosition {
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
  readonly restorePosition?: (key: string) => ReadingPosition | null;
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
  let clampedAnchor = false;
  let geometry: { height: number; viewportHeight: number; top: number } | null = null;
  let scrollIntent = false;
  let scrollIntentTop = 0;
  let scrollIntentGeneration = 0;
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

  const rememberGeometry = () => {
    geometry = viewport ? { height: viewport.scrollHeight, viewportHeight: viewport.clientHeight, top: viewport.scrollTop } : null;
  };

  const geometryChanged = () => !!viewport && !!geometry
    && (geometry.height !== viewport.scrollHeight || geometry.viewportHeight !== viewport.clientHeight);

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

  const restoreAnchor = (position: ReadingPosition): "missing" | "restored" | "clamped" => {
    clampedAnchor = false;
    if (!viewport || !position.anchorId) return "missing";
    const anchor = messageElement(position.anchorId);
    if (!anchor) return "missing";
    const delta = anchor.getBoundingClientRect().top - viewport.getBoundingClientRect().top - position.anchorOffset;
    if (Math.abs(delta) > 0.5) setScrollTop(viewport.scrollTop + delta);
    // Message wrappers commit before lazy media/diagrams have their final size.
    // The browser may clamp the requested scroll to a temporary bottom. Keep
    // the original offset until a later resize can actually reach it.
    clampedAnchor = Math.abs(anchor.getBoundingClientRect().top - viewport.getBoundingClientRect().top - position.anchorOffset) > 1;
    return clampedAnchor ? "clamped" : "restored";
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
    if (!viewport || !model.key || scrollIntent) return;
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
        const restored = restoreAnchor(target);
        if (restored === "clamped") return;
        if (restored === "missing") {
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
      rememberGeometry();
      reportVisible();
      return;
    }
    if (pending) {
      const restore = pending;
      pending = null;
      revealMessageId = restore.revealMessageId;
      if (revealMessageId && scrollToMessage(revealMessageId)) {
        opening = null;
        clampedAnchor = false;
        revealMessageId = null;
        followBottom = false;
      }
      else if (restore.forceBottom || (!restore.position && followBottom)) scrollToBottom();
      else if (restore.position) {
        const restored = restoreAnchor(restore.position);
        if (restored === "clamped") { pending = restore; return; }
        if (restored === "missing") setScrollTop(viewport.scrollHeight - viewport.clientHeight - restore.position.distanceFromBottom);
      }
    } else if (revealMessageId && scrollToMessage(revealMessageId)) {
      revealMessageId = null;
      clampedAnchor = false;
      followBottom = false;
    } else if (followBottom) {
      scrollToBottom();
    } else {
      const position = positions.get(model.key);
      if (position && restoreAnchor(position) === "clamped") {
        pending = { keyChanged: false, position, forceBottom: false, revealMessageId: null };
        return;
      }
    }
    save();
    rememberGeometry();
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

  const retainScrollIntent = () => {
    scrollIntent = true;
    const generation = ++scrollIntentGeneration;
    // Keep the gesture through native smooth-scroll frames, but expire input
    // that produces no movement or has stopped before a later layout change.
    requestAnimationFrame(() => requestAnimationFrame(() => {
      if (generation === scrollIntentGeneration && scrollIntent) {
        scrollIntent = false;
        scheduleReconcile();
      }
    }));
  };

  const onScroll = () => {
    if (!viewport || pending || viewport.scrollTop === appliedScrollTop) return;
    // WebKit can deliver layout-induced scroll before ResizeObserver. Media
    // growth is not evidence that the reader stopped following the bottom.
    if (!scrollIntent && geometryChanged()) {
      scheduleReconcile();
      return;
    }
    const movedUp = scrollIntent ? viewport.scrollTop < scrollIntentTop : geometry !== null && viewport.scrollTop < geometry.top;
    if (scrollIntent) { scrollIntentTop = viewport.scrollTop; retainScrollIntent(); }
    appliedScrollTop = null;
    const position = save();
    if (!position) return;
    rememberGeometry();
    // Native smooth scrolling can begin with a tiny upward step. Respect that
    // step even inside the near-bottom threshold instead of snapping it back.
    followBottom = !movedUp && position.distanceFromBottom <= nearEdge;
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

  const onScrollIntent = (event: Event) => {
    if (!viewport) return;
    if (event instanceof KeyboardEvent && !["ArrowUp", "ArrowDown", "PageUp", "PageDown", "Home", "End", " "].includes(event.key)) return;
    if (event.type === "pointerdown" && event.target !== viewport) return;
    scrollIntentTop = viewport.scrollTop;
    retainScrollIntent();
    if (!opening && pending && !pending.revealMessageId) {
      pending = null;
      appliedScrollTop = null;
      const position = save();
      followBottom = followBottom && (position?.distanceFromBottom ?? Infinity) <= nearEdge;
    }
    if (!clampedAnchor) return;
    // An unreachable offset must never trap the reader after genuine input
    // (for example if content was removed while they visited another channel).
    clampedAnchor = false;
    opening = null;
    pending = null;
    appliedScrollTop = null;
    const position = save();
    followBottom = (position?.distanceFromBottom ?? Infinity) <= nearEdge;
    // End/wheel at an already-clamped bottom may not emit a scroll event.
    scheduleReconcile();
  };

  const detachScrollIntent = () => {
    viewport?.removeEventListener("wheel", onScrollIntent);
    viewport?.removeEventListener("touchstart", onScrollIntent);
    viewport?.removeEventListener("keydown", onScrollIntent);
    viewport?.removeEventListener("pointerdown", onScrollIntent);
  };

  const viewportRef = (element: HTMLElement | null) => {
    if (viewport === element) return;
    if (viewport && !pending && (scrollIntent || !geometryChanged())) {
      save();
    }
    mutationObserver?.disconnect();
    resizeObserver?.disconnect();
    detachScrollIntent();
    viewport = element;
    geometry = null;
    if (!viewport) return;
    viewport.addEventListener("wheel", onScrollIntent, { passive: true });
    viewport.addEventListener("touchstart", onScrollIntent, { passive: true });
    viewport.addEventListener("keydown", onScrollIntent);
    viewport.addEventListener("pointerdown", onScrollIntent);
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
    const previous = (pending || scheduled || applying || (!scrollIntent && geometryChanged())) && model.key
      ? pending?.position ?? positions.get(model.key) ?? capture()
      : save();
    const keyChanged = model.key !== next.key;
    // Several history responses/publications may arrive before the next frame
    // restores the DOM. Keep the original reading anchor through that batch.
    let position = !keyChanged && pending ? pending.position : previous;
    // Deletion can remove the current anchor. Capture a surviving neighbour
    // before the DOM changes so its content keeps the same viewport offset.
    if (!keyChanged && position?.anchorId && !next.messageIds.includes(position.anchorId) && viewport) {
      const nextId = next.messageIds.find((_, index) => (next.messageSequences?.[index] ?? 0) >= (position?.sequence ?? 0))
        ?? next.messageIds.at(-1);
      const neighbour = nextId ? messageElement(nextId) : null;
      if (neighbour) position = { ...position, anchorId: nextId!, sequence: Number(neighbour.dataset.messageSequence),
        anchorOffset: neighbour.getBoundingClientRect().top - viewport.getBoundingClientRect().top };
    }
    const stored = next.key ? positions.get(next.key) ?? options.restorePosition?.(next.key) ?? null : null;
    if (keyChanged) {
      clampedAnchor = false;
      scrollIntent = false;
      opening = next.key ? { position: stored, readSequence: next.initialReadSequence ?? 0 } : null;
      if (next.key && !openingReadSequences.has(next.key)) openingReadSequences.set(next.key, next.initialReadSequence ?? 0);
    }
    // Several host publications can be coalesced into one concurrent React
    // commit (for example accepted reply + cleared composer). Keep a reveal
    // intent until the corresponding message has reached the DOM.
    const carriedReveal = pending?.revealMessageId ?? revealMessageId;
    const carriedBottom = !keyChanged && !scrollIntent && pending?.forceBottom;
    const explicitReveal = next.revealMessageId ?? (keyChanged ? null : carriedReveal);
    const wasNearBottom = followBottom && (position ? position.distanceFromBottom <= nearEdge : true);
    // A publication during native scrolling must not install a restore that
    // blocks the upcoming scroll event. Opening and explicit reveals retain
    // their targets; ordinary same-channel input owns its actual position.
    pending = !keyChanged && scrollIntent && !opening && !explicitReveal ? null : {
      keyChanged,
      position: keyChanged ? stored : position,
      forceBottom: keyChanged ? !stored && !explicitReveal : (!explicitReveal && (carriedBottom || wasNearBottom)),
      revealMessageId: explicitReveal
    };
    if (pending) followBottom = pending.forceBottom || ((keyChanged || followBottom) && (pending.position?.distanceFromBottom ?? 0) <= nearEdge);
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
    detachScrollIntent();
    viewport = null;
  };

  window.addEventListener("focus", scheduleReconcile);
  document.addEventListener("visibilitychange", scheduleReconcile);
  const goToLatest = () => {
    opening = null; pending = null; clampedAnchor = false; revealMessageId = null; followBottom = true;
    scrollToBottom(); save(); rememberGeometry(); reportVisible();
  };
  const unreadAfterSequence = () => model.key ? openingReadSequences.get(model.key) : undefined;
  const readingPosition = () => { const position = save(); return model.key && position ? { key: model.key, position } : null; };
  return Object.freeze({ prepare, viewportRef, onScroll, goToLatest, unreadAfterSequence, readingPosition, dispose });
}

export type TimelineScrollController = ReturnType<typeof createTimelineScrollController>;
