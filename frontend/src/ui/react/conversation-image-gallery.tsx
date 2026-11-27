import { createContext, useContext, useEffect, useRef, useState, useSyncExternalStore, type ReactNode } from "react";
import { ImageViewer } from "./image-viewer";

type GalleryImage = { src: string; downloadSrc: string; name: string };

// Track actual image occurrences so the same attachment can appear in several
// messages or panes. Saved previews participate only while their source is available.
function createGallery() {
  const images = new Map<HTMLButtonElement, GalleryImage>();
  const listeners = new Set<() => void>();
  let selected: HTMLButtonElement | null = null;
  let version = 0;
  const changed = () => {
    version++;
    for (const listener of listeners) listener();
  };
  return {
    register(button: HTMLButtonElement, image: GalleryImage) {
      images.set(button, image);
      if (selected) changed();
      return () => {
        images.delete(button);
        if (selected) changed();
      };
    },
    select(button: HTMLButtonElement | null) { selected = button; changed(); },
    subscribe(listener: () => void) { listeners.add(listener); return () => { listeners.delete(listener); }; },
    getVersion: () => version,
    current: () => selected,
    get: (button: HTMLButtonElement) => images.get(button),
    ordered: () => [...images.keys()]
      .map(button => ({ button, sequence: Number(button.closest<HTMLElement>("[data-message-sequence]")?.dataset.messageSequence) }))
      .sort((a, b) => a.sequence - b.sequence
        || (a.button.compareDocumentPosition(b.button) & Node.DOCUMENT_POSITION_FOLLOWING ? -1 : 1))
      .map(entry => entry.button)
  };
}

const GalleryContext = createContext<ReturnType<typeof createGallery> | null>(null);
export const useConversationImageGallery = () => useContext(GalleryContext);

interface HistoryPage {
  oldest?: number;
  loading?: boolean;
  hasOlder?: boolean;
  error?: string;
  onLoadOlder?: () => void;
}

export function ConversationImageGallery({ children, ...history }: HistoryPage & { children: ReactNode }) {
  const [gallery] = useState(createGallery);
  return <GalleryContext.Provider value={gallery}>{children}<GalleryViewer gallery={gallery} {...history} /></GalleryContext.Provider>;
}

function GalleryViewer({ gallery, oldest, loading, hasOlder, error, onLoadOlder }: HistoryPage & { gallery: ReturnType<typeof createGallery> }) {
  const version = useSyncExternalStore(gallery.subscribe, gallery.getVersion);
  const current = gallery.current();
  const image = current && gallery.get(current);
  const ordered = image ? gallery.ordered() : [];
  const index = current ? ordered.indexOf(current) : -1;
  const previous = ordered[index - 1];
  const next = ordered[index + 1];
  const [seeking, seek] = useState<HTMLButtonElement | null>(null);
  const requested = useRef<number | null>(null);
  useEffect(() => {
    // A newly prepended page registers its thumbnails during this commit.
    // Wait for that registry snapshot before concluding the search has ended.
    if (version !== gallery.getVersion()) return;
    if (current && !image) gallery.select(null);
    if (!seeking) return;
    if (!image || seeking !== current || previous || error || !hasOlder || !onLoadOlder) {
      if (seeking === current && previous) gallery.select(previous);
      seek(null);
      requested.current = null;
    } else if (!loading && oldest !== undefined && requested.current !== oldest) {
      requested.current = oldest;
      onLoadOlder();
    }
  }, [current, image, gallery, seeking, oldest, loading, hasOlder, error, onLoadOlder, previous, version]);
  if (!image) return null;
  return <ImageViewer {...image} onClose={() => gallery.select(null)}
    onPrevious={previous ? () => gallery.select(previous) : hasOlder && !error && onLoadOlder && !seeking ? () => seek(current) : undefined}
    onNext={next ? () => gallery.select(next) : undefined} gallery />;
}
