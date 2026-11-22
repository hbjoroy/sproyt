/** Render inline Sprøyt references as plain text, including a final reference
 * cut short by the directory's 96-character message budget. */
export function messageExcerpt(body: string): string {
  return body.replace(/\[\[([^\[\]]*)(\]\]|\]?$)/gu, (match, payload: string, end: string) => {
    if (payload.startsWith("media:")) {
      const [, mime = "", encodedName] = payload.split("|");
      if (mime.startsWith("image/")) return "Bilete";
      if (mime.startsWith("video/")) return "Video";
      if (mime.startsWith("audio/")) return "Lyd";
      if (end === "]]" && encodedName) {
        try {
          const name = decodeURIComponent(encodedName).trim();
          if (name) return name;
        } catch { /* An older or incomplete filename still has a readable fallback. */ }
      }
      return "Vedlegg";
    }
    if (payload.startsWith("invite:")) return "Invitasjon";
    if (payload.startsWith("work-item-status:")) return "Oppgåvestatus";
    if (payload.startsWith("work-item-task:") || payload.startsWith("process-task:")) return "Oppgåve";
    if (/^(?:https?:\/\/|\/\?)/u.test(payload)) return "Lenkje";
    return match;
  }).replace(/\s+/gu, " ").trim();
}
