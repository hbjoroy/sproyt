// Shared by the service worker and app bundle. Raw received files never enter
// URLs, the message outbox, or a channel draft before an explicit Send.
(() => {
  const name = "sproyt-share-inbox", storeName = "receipts", deadline = 5000;
  const maxImage = 35 * 1024 * 1024, maxText = 16000, age = 48 * 60 * 60 * 1000;
  const retained = row => Date.now() - row.createdAt < (row.admission ? 7 * 24 * 60 * 60 * 1000 : age);
  const imageTypes = new Set(["image/jpeg", "image/png", "image/gif", "image/webp", "image/heic", "image/avif"]);
  async function transaction(mode, action) {
    const db = await new Promise((resolve, reject) => {
      const open = indexedDB.open(name, 1);
      let settled = false;
      const timer = setTimeout(() => { settled = true; reject(new Error("Lokal lagring svarar ikkje. Prøv igjen.")); }, deadline);
      open.onupgradeneeded = () => open.result.createObjectStore(storeName, { keyPath: "id" });
      open.onsuccess = () => { if (settled) { open.result.close(); return; } settled = true; clearTimeout(timer); resolve(open.result); };
      open.onerror = open.onblocked = () => { clearTimeout(timer); reject(new Error("Lokal lagring er ikkje tilgjengeleg.")); };
    });
    try {
      return await new Promise((resolve, reject) => {
        let tx;
        try { tx = db.transaction(storeName, mode, { durability: "strict" }); }
        catch { tx = db.transaction(storeName, mode); }
        let result;
        const timer = setTimeout(() => { try { tx.abort(); } catch {} }, deadline);
        // A successful put is not a durable acknowledgement: wait for commit.
        tx.oncomplete = () => { clearTimeout(timer); resolve(result); };
        tx.onerror = tx.onabort = () => { clearTimeout(timer); reject(new Error("Delinga kunne ikkje lagrast lokalt. Prøv igjen.")); };
        const store = tx.objectStore(storeName), request = store.getAll();
        request.onsuccess = () => {
          try { result = action(store, request.result); }
          catch (error) { clearTimeout(timer); tx.abort(); reject(error); }
        };
      });
    } finally { db.close(); }
  }
  const meta = rows => rows.find(row => row.id === "auth") ?? { id: "auth", generation: 0 };
  const ensure = (rows, generation) => { if (meta(rows).generation !== generation) throw new Error("Innlogginga vart endra. Delinga er ikkje teken i bruk."); };
  async function identity() {
    const response = await fetch("/auth/share-identity", { credentials: "same-origin", cache: "no-store", signal: AbortSignal.timeout(deadline) });
    if (response.status === 401) return null;
    if (!response.ok) throw new Error("Kunne ikkje kontrollere innlogginga. Prøv igjen når nettet er tilgjengeleg.");
    const value = await response.json();
    if (!value || typeof value.user_id !== "string" || !value.user_id) throw new Error("Innloggingstenesta svarte ugyldig.");
    return value.user_id;
  }
  const generation = () => transaction("readonly", (_, rows) => meta(rows).generation);
  const receipt = (rows, id, owner, revision) => {
    ensure(rows, revision);
    const item = rows.find(row => row.id === id && row.owner === owner && !row.done);
    if (!item) throw new Error("Delinga er ikkje tilgjengeleg for denne kontoen.");
    return item;
  };
  globalThis.SproytShareInbox = {
    identity, generation,
    async capture(form, capturedGeneration) {
      const revision = capturedGeneration ?? await generation();
      const owner = await identity(); // Only a confirmed 401 means anonymous.
      const text = ["title", "text", "url"].map(key => {
        const value = form.get(key);
        if (value !== null && typeof value !== "string") throw new Error("Delinga inneheld ugyldig tekst.");
        return (value ?? "").trim();
      }).filter((value, index, all) => value && all.indexOf(value) === index).join("\n\n");
      const files = form.getAll("image").filter(value => value instanceof Blob && value.size);
      if (files.length > 1 || form.getAll("image").some(value => !(value instanceof Blob))) throw new Error("Del eitt bilete om gongen.");
      const file = files[0] ?? null;
      if ((!text && !file) || new TextEncoder().encode(text).length > maxText) throw new Error("Delinga er tom eller teksten er for lang.");
      if (file && (file.size > maxImage || !imageTypes.has(file.type))) throw new Error("Del eitt støtta bilete på høgst 35 MiB.");
      const encoded = new TextEncoder().encode(`${text}\n${file?.name ?? ""}\n${file?.type ?? ""}`);
      const bytes = file ? new Uint8Array(await file.arrayBuffer()) : new Uint8Array();
      const combined = new Uint8Array(encoded.length + bytes.length); combined.set(encoded); combined.set(bytes, encoded.length);
      const hash = Array.from(new Uint8Array(await crypto.subtle.digest("SHA-256", combined)), value => value.toString(16).padStart(2, "0")).join("");
      return transaction("readwrite", (store, rows) => {
        ensure(rows, revision);
        const now = Date.now();
        const live = rows.filter(row => row.id !== "auth" && retained(row));
        for (const row of rows) if (row.id !== "auth" && !retained(row)) store.delete(row.id);
        // Retain only a bounded recent dedup tombstone, never accepted content.
        const done = live.filter(row => row.done).sort((a, b) => b.done - a.done);
        for (const row of done.slice(50)) store.delete(row.id);
        const existing = live.find(row => row.owner === owner && row.hash === hash && (!row.done || now - row.done < 120000));
        if (existing) return existing.id;
        const pending = live.filter(row => !row.done);
        if (pending.length >= 5 || pending.reduce((total, row) => total + (row.file?.size ?? 0), file?.size ?? 0) > maxImage * 2)
          throw new Error("Det ligg for mange delingar. Opne Sprøyt og avklar dei først.");
        const item = { id: crypto.randomUUID(), owner, generation: revision, createdAt: now, hash, text, file, admission: null, done: null };
        store.put(item); return item.id;
      });
    },
    list(owner) { return transaction("readonly", (_, rows) => rows.filter(row => row.id !== "auth" && retained(row))
      .filter(row => row.owner === owner || row.owner === null)
      .filter(row => !row.done || (row.owner === owner && row.admission))
      .map(row => row.owner === null ? { ...row, text: "", file: null } : row)); },
    claim(id, owner, revision) { return transaction("readwrite", (store, rows) => {
      ensure(rows, revision);
      const item = rows.find(row => row.id === id && (row.owner === null || row.owner === owner) && !row.done);
      if (!item) throw new Error("Delinga er ikkje tilgjengeleg.");
      const next = { ...item, owner }; store.put(next); return next;
    }); },
    edit(id, owner, revision, text, channelId) { return transaction("readwrite", (store, rows) => {
      const item = receipt(rows, id, owner, revision);
      if (item.admission) throw new Error("Sendinga ventar på avklaring. Utkastet er låst til den opphavlege sendinga.");
      if (typeof text !== "string" || new TextEncoder().encode(text).length > maxText) throw new Error("Teksten er for lang.");
      const next = { ...item, text, channelId }; store.put(next); return next;
    }); },
    admit(id, owner, revision, admission) { return transaction("readwrite", (store, rows) => {
      const item = receipt(rows, id, owner, revision);
      if (item.admission && JSON.stringify(item.admission) !== JSON.stringify(admission)) throw new Error("Sendinga ventar på avklaring. Prøv den opphavlege sendinga igjen.");
      const next = { ...item, admission }; store.put(next); return next;
    }); },
    owned(id, owner, revision) { return transaction("readwrite", (store, rows) => {
      const item = rows.find(row => row.id === id && row.owner === owner && !row.done);
      ensure(rows, revision); if (item) store.put({ ...item, file: null });
    }); },
    finish(id, owner, revision) { return transaction("readwrite", (store, rows) => {
      ensure(rows, revision);
      const item = rows.find(row => row.id === id && row.owner === owner);
      if (!item || item.done) return;
      store.put({ ...item, done: Date.now(), text: "", file: null,
        admission: item.admission ? { ...item.admission, body: "", draft: "", media: [] } : null });
    }); },
    discard(id, owner, revision) { return transaction("readwrite", (store, rows) => {
      const item = receipt(rows, id, owner, revision);
      if (item.admission) throw new Error("Sendinga må avklarast før delinga kan fjernast.");
      store.delete(id);
    }); },
    logout() { return transaction("readwrite", (store, rows) => {
      const next = { id: "auth", generation: meta(rows).generation + 1 };
      store.clear(); store.put(next);
    }); }
  };
})();
