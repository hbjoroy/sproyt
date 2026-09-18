import type { MediaObject } from "../types";

export interface ComposerTarget {
  readonly channelId: string;
  readonly parentMessageId: string | null;
}

export interface ComposerSnapshot {
  readonly value: string;
  readonly disabled: boolean;
  readonly busy: boolean;
  readonly sendOnEnter: boolean;
  readonly media: readonly MediaObject[];
  readonly uploadStatus?: string;
  readonly error?: string;
}

/** Application state and guarded commands shared by presentation adapters.
 * Neither drafts nor permission/transport gates are read from form controls. */
export function createComposerController(dependencies: {
  snapshot: (target: ComposerTarget) => ComposerSnapshot;
  changeDraft: (target: ComposerTarget, value: string) => void;
  send: (target: ComposerTarget) => void;
}) {
  return {
    channel: { draft: "", readOnly: false },
    threadReadOnly: false,
    snapshot: dependencies.snapshot,
    changeDraft(target: ComposerTarget, value: string) {
      const state = dependencies.snapshot(target);
      if (!state.disabled && !state.busy) dependencies.changeDraft(target, value);
    },
    send(target: ComposerTarget) {
      const state = dependencies.snapshot(target);
      if (!state.disabled && !state.busy) dependencies.send(target);
    }
  };
}
