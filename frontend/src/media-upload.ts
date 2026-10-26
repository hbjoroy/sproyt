/** Bounded browser upload: one File.slice/hash/request at a time, never a
 * full-file ArrayBuffer. An uncertain request is retried with the same identity. */
export async function uploadMediaChunks(file: File, channelId: string, options: {
  authQuery?: string; refresh: () => Promise<boolean>; progress: (message: string) => void;
  admitted?: () => boolean;
}): Promise<Response | null> {
  const query = options.authQuery || "";
  async function request(path: string, init: RequestInit = {}): Promise<Response> {
    for (let attempt = 0; ; attempt += 1) {
      if (options.admitted && !options.admitted()) throw new Error("Kontoen vart endra under opplastinga.");
      let response: Response;
      try {
        response = await fetch(`${path}${query}`, { ...init, credentials: "same-origin", signal: AbortSignal.timeout(60000) });
        // Read/validate successful bodies within the SAME retry operation.
        // Headers may arrive even when the connection dies after DB commit.
        if (response.ok) {
          const text = await response.text();
          if (text.length > 16384) throw new Error("For stor opplastingsrespons");
          JSON.parse(text);
          response = new Response(text,{status:response.status,headers:response.headers});
        }
      } catch (error) {
        if (options.admitted && !options.admitted()) throw error;
        if (attempt >= 4) throw error;
        await new Promise(resolve => setTimeout(resolve, 500 * 2 ** attempt));
        continue;
      }
      if (options.admitted && !options.admitted()) throw new Error("Kontoen vart endra under opplastinga.");
      if (response.status === 401 && attempt < 4 && await options.refresh()) continue;
      if ([408,429,502,503,504].includes(response.status) && attempt < 4) {
        await new Promise(resolve => setTimeout(resolve, 500 * 2 ** attempt));
        continue;
      }
      return response;
    }
  }
  const configResponse = await request("/api/v1/media-uploads/config");
  if (!configResponse.ok) return configResponse;
  const config: { enabled: boolean; max_file_bytes: number; chunk_bytes: number; allowed_channel_ids?: string[] | null } = await configResponse.json();
  if (typeof config.enabled !== "boolean") throw new Error("Ugyldig opplastingskonfigurasjon");
  if (!config.enabled) return null;
  if (config.allowed_channel_ids != null) {
    if (!Array.isArray(config.allowed_channel_ids) || !config.allowed_channel_ids.every(id => typeof id === "string"))
      throw new Error("Ugyldig opplastingskonfigurasjon");
    if (!config.allowed_channel_ids.includes(channelId)) return null;
  }
  if (config.chunk_bytes !== 1024 * 1024 || !Number.isSafeInteger(config.max_file_bytes) || config.max_file_bytes > 1024 ** 3)
    throw new Error("Ugyldig opplastingskonfigurasjon");
  if (!file.size || file.size > config.max_file_bytes)
    throw new Error(`Fila må vere mellom 1 byte og ${Math.floor(config.max_file_bytes / 1024 / 1024)} MiB.`);
  const response = await request(`/api/v1/channels/${channelId}/media-uploads`, {
    method: "POST", headers: { "content-type": "application/json" },
    body: JSON.stringify({ request_id: crypto.randomUUID(), filename: file.name || "bilete", content_type: file.type || "application/octet-stream", size_bytes: file.size })
  });
  if (!response.ok) return response;
  const upload: { id: string; chunk_bytes: number; next_chunk: number } = await response.json();
  if (!/^[0-9a-f-]{36}$/i.test(upload.id) || upload.chunk_bytes !== config.chunk_bytes || upload.next_chunk !== 0)
    throw new Error("Ugyldig opplastingsrespons");
  for (let offset = 0, index = 0; offset < file.size; offset += config.chunk_bytes, index += 1) {
    const chunk = file.slice(offset, Math.min(file.size, offset + config.chunk_bytes));
    const hash = new Uint8Array(await crypto.subtle.digest("SHA-256", await chunk.arrayBuffer()));
    const digest = Array.from(hash, byte => byte.toString(16).padStart(2, "0")).join("");
    const result = await request(`/api/v1/media-uploads/${upload.id}/chunks/${index}`, {
      method: "PUT", headers: { "x-chunk-sha256": digest, "content-type": "application/octet-stream" }, body: chunk
    });
    if (!result.ok) return result;
    options.progress(`Lastar opp ${file.name || "fila"} ${Math.round(Math.min(file.size,offset+chunk.size)*100/file.size)} %`);
  }
  options.progress("Originalen er lasta opp. Lagrar vedlegget …");
  return request(`/api/v1/media-uploads/${upload.id}/complete`, { method: "POST" });
}
