import { createContext, isValidElement, useContext, useEffect, useId, useRef, useState, type ComponentProps } from "react";
import { createPortal } from "react-dom";
import Markdown, { type Components, type ExtraProps } from "react-markdown";
import remarkGfm from "remark-gfm";
import { Button, Dialog, Status } from "@sproyt/ui/react";
import type { ChannelReference } from "../../channel-references";

const mediaToken = /\[\[media:[0-9a-f-]{36}\|[^|\]]+\|[^\]]*\]\]/giu;
const invitationToken = /\[\[invite:[A-Za-z0-9_-]{32,128}\]\]/gu;

let mermaidModule: Promise<typeof import("mermaid").default> | null = null;

function loadMermaid() {
  mermaidModule ??= import("mermaid").then(module => {
    module.default.initialize({
      startOnLoad: false,
      securityLevel: "strict",
      theme: window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "default"
    });
    return module.default;
  });
  return mermaidModule;
}

export function markdownTextFromMessage(body: string): string {
  return body.replace(mediaToken, "").replace(invitationToken, "").trim();
}

function MermaidDiagram({ source }: { readonly source: string }) {
  const reactId = useId().replace(/[^A-Za-z0-9_-]/g, "");
  const target = useRef<HTMLDivElement>(null);
  const [error, setError] = useState("");

  useEffect(() => {
    let active = true;
    setError("");
    void loadMermaid().then(async mermaid => {
      const result = await mermaid.render(`sproyt-mermaid-${reactId}`, source);
      if (!active || !target.current) return;
      const document = new DOMParser().parseFromString(result.svg, "image/svg+xml");
      if (document.querySelector("parsererror") || document.documentElement.localName !== "svg") {
        throw new Error("Mermaid gav ugyldig SVG.");
      }
      const svg = window.document.importNode(document.documentElement, true);
      target.current.replaceChildren(svg);
      result.bindFunctions?.(target.current);
    }).catch(() => {
      if (active) setError("Diagrammet kunne ikkje renderast.");
    });
    return () => {
      active = false;
      target.current?.replaceChildren();
    };
  }, [reactId, source]);

  return <div className="mermaid-shell">
    {error ? <span role="alert">{error}</span> : <div ref={target} className="mermaid" aria-label="Mermaid-diagram" />}
  </div>;
}

function appLink(href: string | undefined): boolean {
  if (!href || typeof window === "undefined") return false;
  try {
    const url = new URL(href, window.location.href);
    return url.origin === window.location.origin && url.pathname === "/"
      && (url.searchParams.has("channel") || ["settings", "inbox", "circles", "direct"].includes(url.searchParams.get("view") ?? ""));
  } catch { return false; }
}
function DiceRoll({ source }: { readonly source: string }) {
  const trimmed=source.trim();
  if (!/^[1-6](?:,[1-6]){0,9}$/u.test(trimmed)) return <code>{source}</code>;
  const values=trimmed.split(",").map(Number);
  return <div role="img" aria-label={`Terningkast: ${values.join(" og ")}`} style={{display:"flex",flexWrap:"wrap",gap:4}}>
    {values.map((value,index)=><img key={index} src={`/assets/dice/v1/${value}-${index % 2 === 0 ? "a" : "b"}.png`}
      width={96} height={96} alt="" loading="lazy" decoding="async" onError={event=>{event.currentTarget.hidden=true;}} style={{objectFit:"contain"}} />)}
  </div>;
}

const components: Components = {
  pre: ({children,...props}) => isValidElement<{className?:string}>(children) && children.props.className === "language-dice"
    ? <>{children}</> : <pre {...props}>{children}</pre>,
  a: MarkdownAnchor,
  code: ({ className, children, ...props }) => {
    const language = /(?:^|\s)language-([^\s]+)/u.exec(className ?? "")?.[1]?.toLowerCase();
    if (language === "dice") return <DiceRoll source={String(children)} />;
    if (language === "mermaid") return <MermaidDiagram source={String(children).replace(/\n$/u, "")} />;
    return <code {...props} className={className}>{children}</code>;
  },
  img: ({ alt }) => <span className="sp-markdown-image-placeholder">{alt ? `Bilete: ${alt}` : "Eksternt bilete"}</span>
};

export type MessageChannelReferences = Readonly<{
  scopeKey: string;
  find(slug: string): ChannelReference | undefined;
  collect(slugs: ReadonlySet<string>): void;
  href(channel: ChannelReference): string;
  open(channel: ChannelReference): Promise<ChannelReference | undefined>;
  join(channel: ChannelReference): Promise<void>;
}>;
const ChannelReferencesContext = createContext<MessageChannelReferences | undefined>(undefined);

function MarkdownAnchor({ node, children, href, ...props }: ComponentProps<"a"> & ExtraProps) {
  const references = useContext(ChannelReferencesContext);
  const slug = node?.properties["data-channel-reference"];
  const channel = typeof slug === "string" ? references?.find(slug) : undefined;
  if (references && channel)
    return <ChannelReferenceLink channel={channel} href={href} references={references}>{children}</ChannelReferenceLink>;
  return <a {...props} href={href} target={appLink(href) ? undefined : "_blank"} rel="noopener noreferrer" referrerPolicy="no-referrer">{children}</a>;
}

type MarkdownNode = {
  type: string; value?: string; children?: MarkdownNode[]; url?: string;
  data?: { hProperties: Record<string, string> };
  position?: { start: { offset?: number }; end: { offset?: number } };
};
const literalNodes = new Set(["link", "linkReference", "image", "imageReference", "inlineCode", "code", "html"]);
const channelToken = /(^|[\s([{])#([A-Za-z0-9_-]{1,80})(?![\p{L}\p{N}_-])/gu;
const rawChannelToken = /(^|[\s([{])(\\*)#([A-Za-z0-9_-]{1,80})(?![\p{L}\p{N}_-])/gu;

function channelReferencePlugin(source: string, references: MessageChannelReferences, candidates: Set<string>) {
  function transform(node: MarkdownNode) {
    if (!node.children || literalNodes.has(node.type)) return;
    node.children = node.children.flatMap(child => {
      if (child.type !== "text" || !child.value) {
        transform(child);
        return [child];
      }
      const text = child.value;
      const raw = source.slice(child.position?.start.offset, child.position?.end.offset);
      // Markdown decodes a backslash before the hash. Match the source tokens
      // in order so an escaped occurrence stays literal beside an ordinary one.
      const rawTokens = raw.includes("\\#") ? [...raw.matchAll(rawChannelToken)] : [];
      let rawCursor = 0;
      const children: MarkdownNode[] = [];
      let end = 0;
      for (const match of text.matchAll(channelToken)) {
        const sourceIndex = rawTokens.findIndex((token, index) => index >= rawCursor && token[3] === match[2]);
        if (sourceIndex !== -1) {
          rawCursor = sourceIndex + 1;
          if (rawTokens[sourceIndex]![2]!.length % 2 === 1) continue;
        }
        const slug = match[2]!.toLowerCase();
        if (candidates.size < 64) candidates.add(slug);
        const channel = references.find(slug);
        if (!channel) continue;
        const start = match.index + match[1]!.length;
        if (start > end) children.push({ type: "text", value: text.slice(end, start) });
        children.push({ type: "link", url: references.href(channel),
          data: { hProperties: { "data-channel-reference": slug } },
          children: [{ type: "text", value: text.slice(start, match.index + match[0].length) }] });
        end = match.index + match[0].length;
      }
      if (!end) return [child];
      if (end < text.length) children.push({ type: "text", value: text.slice(end) });
      return children;
    });
  }
  return () => transform;
}

function ChannelReferenceLink({ channel, href, references, children }: {
  channel: ChannelReference; href: string | undefined; references: MessageChannelReferences; children: React.ReactNode;
}) {
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const link = useRef<HTMLAnchorElement>(null);
  const dialogHost = open ? link.current?.closest(".sp-theme") : null;
  return <>
    <a ref={link} href={href} data-channel-reference={channel.slug} aria-busy={busy || undefined} onClick={event => {
      if (event.button !== 0 || event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
      event.preventDefault();
      if (busy) return;
      setError(""); setBusy(true);
      void references.open(channel).then(current => {
        if (current && !current.is_member) setOpen(true);
      }, () => setError("Kunne ikkje opne kanallenkja. Prøv igjen.")).finally(() => setBusy(false));
    }}>{children}</a>
    {!open && error && <span role="status"> {error}</span>}
    {dialogHost && createPortal(<Dialog open title={`Bli med i ${channel.name}`} onClose={() => setOpen(false)}>
      <p>Du må bli med i kanalen for å opne han.</p>
      {error && <Status tone="error">{error}</Status>}
      <Button variant="primary" busy={busy} onClick={() => {
        setBusy(true); setError("");
        void references.join(channel).then(() => setOpen(false), cause => setError(cause instanceof Error ? cause.message : "Kunne ikkje bli med i kanalen."))
          .finally(() => setBusy(false));
      }}>Bli med og opne kanalen</Button>
    </Dialog>, dialogHost)}
  </>;
}

export function MarkdownContent({ source, className, channelReferences }: {
  readonly source: string; readonly className?: string; readonly channelReferences?: MessageChannelReferences;
}) {
  const candidates = new Set<string>();
  const viewport = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!channelReferences || !viewport.current || !candidates.size) return;
    // Only visible message bodies compete for the bounded conversation cache.
    const observer = new IntersectionObserver(entries => {
      if (entries.some(entry => entry.isIntersecting)) channelReferences.collect(candidates);
    });
    observer.observe(viewport.current);
    return () => observer.disconnect();
  }, [source, channelReferences?.scopeKey]);
  const render = (value: string) => <Markdown remarkPlugins={channelReferences
    ? [remarkGfm, channelReferencePlugin(value, channelReferences, candidates)] : [remarkGfm]} components={components}>{value}</Markdown>;
  if (!source.trim()) return null;
  // New shared place answers stay ordinary Markdown for other clients. Keep
  // provider detail available without repeating it above the compact result list.
  const detailMarker = "\n\n**Om stadene**\n\n";
  const detailAt = source.startsWith("**Barforslag · Google Maps**") ? source.indexOf(detailMarker) : -1;
  return <ChannelReferencesContext.Provider value={channelReferences}>
    {detailAt >= 0 ? <div ref={viewport} className={[className, "sp-bar-results"].filter(Boolean).join(" ")}>
      {render(source.slice(0, detailAt))}
      <details><summary>Om stadene</summary>
        {render(source.slice(detailAt + detailMarker.length))}
      </details>
    </div> : <div ref={viewport} className={className}>{render(source)}</div>}
  </ChannelReferencesContext.Provider>;
}
