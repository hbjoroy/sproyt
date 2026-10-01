import { useEffect, useState } from "react";

/** Keep the native download path so mobile browsers can hand the file to the OS. */
export function ImageDownloadLink({ href, name, className }: { href: string; name: string; className: string }) {
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    if (!attempt) return;
    const timer = window.setTimeout(() => setAttempt(0), 4000);
    return () => window.clearTimeout(timer);
  }, [attempt]);

  return <>
    <a className={className} href={href} download={name} onClick={() => setAttempt(value => value + 1)}
      aria-label={`Last ned ${name}`} title={`Last ned ${name}`}>↓</a>
    {attempt > 0 && <output role="status" aria-live="polite" className="sp-media-download-notice">Nedlasting starta</output>}
  </>;
}
