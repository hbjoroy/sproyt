import { useEffect, useId, useRef, useState } from "react";
import Markdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";

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

const components: Components = {
  a: ({ href, children, ...props }) => <a {...props} href={href} target="_blank" rel="noopener noreferrer" referrerPolicy="no-referrer">{children}</a>,
  code: ({ className, children, ...props }) => {
    const language = /(?:^|\s)language-([^\s]+)/u.exec(className ?? "")?.[1]?.toLowerCase();
    if (language === "mermaid") return <MermaidDiagram source={String(children).replace(/\n$/u, "")} />;
    return <code {...props} className={className}>{children}</code>;
  },
  img: ({ alt }) => <span className="sp-markdown-image-placeholder">{alt ? `Bilete: ${alt}` : "Eksternt bilete"}</span>
};

export function MarkdownContent({ source, className }: { readonly source: string; readonly className?: string }) {
  if (!source.trim()) return null;
  return <div className={className}>
    <Markdown remarkPlugins={[remarkGfm]} components={components}>{source}</Markdown>
  </div>;
}
