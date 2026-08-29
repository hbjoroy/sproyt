export type RawClientCoreInstance = Readonly<{ exports: WebAssembly.Exports }>;
export type ClientCoreWasmLoader = () => Promise<RawClientCoreInstance>;

export const clientCoreWasmMetaName = "sproyt-client-core";

export function clientCoreWasmUrl(document: Document = globalThis.document): string | null {
  const value = document.querySelector(`meta[name="${clientCoreWasmMetaName}"]`)?.getAttribute("content")?.trim();
  return value || null;
}

let sharedClientCore: Promise<RawClientCoreInstance> | null = null;

export function loadClientCoreWasm(): Promise<RawClientCoreInstance> {
  if (sharedClientCore !== null) return sharedClientCore;
  sharedClientCore = (async () => {
    const url = clientCoreWasmUrl();
    if (!url) throw new Error("WASM client core asset metadata is missing");
    const response = await fetch(url, { credentials: "same-origin" });
    if (!response.ok) throw new Error(`WASM client core asset failed to load (${response.status})`);
    const bytes = await response.arrayBuffer();
    const { instance } = await WebAssembly.instantiate(bytes, {});
    return instance;
  })();
  return sharedClientCore;
}
