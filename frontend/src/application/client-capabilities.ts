/** Explicit server bootstrap contract; presentation visibility is not feature availability or authorization. */
export function clientCapabilities(value: string | null | undefined): Readonly<{ agent: boolean; heart: boolean }> {
  const flags = new Set((value ?? "").split(/\s+/));
  return Object.freeze({ agent: flags.has("agent"), heart: flags.has("heart") });
}
