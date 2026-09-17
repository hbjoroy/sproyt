/** The design-system interface is the application default. Local development
 * retains an explicit legacy escape hatch for regression comparison only. */
export function shouldMountReactInterface(location: Pick<URL, "hostname" | "search">): boolean {
  const local = ["localhost", "127.0.0.1", "[::1]"].includes(location.hostname);
  return !(local && new URLSearchParams(location.search).get("ui") === "legacy");
}
