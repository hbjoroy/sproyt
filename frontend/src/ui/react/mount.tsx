import { createRoot } from "react-dom/client";
import { ConversationView } from "./conversation-view";
import type { ConversationViewProps } from "./conversation-view";

const mountedContainers = new WeakSet<HTMLElement>();

/** Mount only into an empty, exclusively React-owned region after installing
 * the application's single stylesheet. The caller owns its explicit height.
 * Runtime, sockets, draft persistence and commands must already exist. */
export function mountConversationView(container: HTMLElement, initial: ConversationViewProps) {
  if (mountedContainers.has(container) || container.hasChildNodes()) {
    throw new Error("Conversation view requires an empty, unowned container");
  }
  const root = createRoot(container);
  mountedContainers.add(container);
  let disposed = false;
  const update = (props: ConversationViewProps) => {
    if (disposed) throw new Error("Conversation view has been unmounted");
    root.render(<ConversationView {...props} />);
  };
  update(initial);
  return Object.freeze({
    update,
    unmount() {
      if (disposed) return;
      disposed = true;
      root.unmount();
      mountedContainers.delete(container);
      // Unmounting a view never disposes the application runtime.
    }
  });
}
