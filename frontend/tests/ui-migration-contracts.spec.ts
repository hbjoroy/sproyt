import { expect, test } from "@playwright/test";
import { createDurableOutbox, type DurableOutboxStorage, type DurableSend } from "../src/durable-outbox";
import { NavigationController } from "../src/navigation";

class MemoryStorage implements Storage {
  readonly values = new Map<string, string>();

  get length(): number { return this.values.size; }
  clear(): void { this.values.clear(); }
  getItem(key: string): string | null { return this.values.get(key) ?? null; }
  key(index: number): string | null { return [...this.values.keys()][index] ?? null; }
  removeItem(key: string): void { this.values.delete(key); }
  setItem(key: string, value: string): void { this.values.set(key, value); }
}

class MemoryDurableOutboxStorage implements DurableOutboxStorage {
  readonly entries = new Map<string, DurableSend>();
  rejectWrites = false;

  async list(userId: string): Promise<readonly DurableSend[]> {
    return [...this.entries.values()].filter((entry) => entry.userId === userId);
  }

  async put(entry: DurableSend): Promise<void> {
    if (this.rejectWrites) throw new Error("storage unavailable");
    this.entries.set(`${entry.userId}:${entry.requestId}`, entry);
  }

  async delete(userId: string, requestId: string): Promise<void> {
    this.entries.delete(`${userId}:${requestId}`);
  }
}

const channel = (id: string) => ({
  id,
  slug: id,
  name: id,
  circle_id: null,
  direct_user_id: null
});

test("compact list/detail navigation keeps channel and thread drafts isolated through reload", () => {
  const storage = new MemoryStorage();
  const navigation = new NavigationController(storage, new URL("https://chat.example.test/"));

  navigation.setActiveChannel(channel("channel-a"));
  navigation.persistChannelDraft("channel-a", "utkast i A");
  navigation.persistThreadDraft("channel-a", "root-a1", "svar i A1");
  navigation.persistThreadDraft("channel-a", "root-a2", "svar i A2");

  // A compact layout temporarily deactivates the detail view while showing
  // the conversation list. That transition must not discard its restore key.
  navigation.deactivateChannel();
  expect(navigation.activeChannelId).toBeNull();
  expect(navigation.restoredChannelId).toBe("channel-a");

  navigation.setActiveChannel(channel("channel-b"));
  navigation.persistChannelDraft("channel-b", "utkast i B");
  navigation.persistThreadDraft("channel-b", "root-b1", "svar i B1");

  expect(navigation.restoreChannelDraft("channel-a")).toBe("utkast i A");
  expect(navigation.restoreThreadDraft("channel-a", "root-a1")).toBe("svar i A1");
  expect(navigation.restoreChannelDraft("channel-b")).toBe("utkast i B");

  const reloaded = new NavigationController(storage, new URL("https://chat.example.test/"));
  expect(reloaded.restoredChannelId).toBe("channel-b");
  expect(reloaded.restoreChannelDraft("channel-a")).toBe("utkast i A");
  expect(reloaded.restoreThreadDraft("channel-a", "root-a1")).toBe("svar i A1");
  expect(reloaded.restoreThreadDraft("channel-a", "root-a2")).toBe("svar i A2");
  expect(reloaded.restoreChannelDraft("channel-b")).toBe("utkast i B");
  expect(reloaded.restoreThreadDraft("channel-b", "root-b1")).toBe("svar i B1");

  reloaded.clearThreadDraft("channel-a", "root-a1");
  expect(reloaded.restoreThreadDraft("channel-a", "root-a1")).toBe("");
  expect(reloaded.restoreThreadDraft("channel-a", "root-a2")).toBe("svar i A2");
  expect(reloaded.restoreThreadDraft("channel-b", "root-b1")).toBe("svar i B1");
  expect(reloaded.restoreChannelDraft("channel-a")).toBe("utkast i A");
});

test("controlled send flow leaves draft clearing to the host after durable acceptance", async () => {
  const draftStorage = new MemoryStorage();
  const navigation = new NavigationController(draftStorage, new URL("https://chat.example.test/"));
  const journalStorage = new MemoryDurableOutboxStorage();
  const outbox = createDurableOutbox(journalStorage, () => 1_000);
  await outbox.setUser("user-a");

  navigation.persistChannelDraft("channel-a", "  hei frå skrivefeltet  ");
  journalStorage.rejectWrites = true;
  await expect(outbox.enqueue({
    requestId: "request-failed",
    channelId: "channel-a",
    parentMessageId: null,
    body: "hei frå skrivefeltet",
    draft: "  hei frå skrivefeltet  ",
    media: []
  })).rejects.toThrow("storage unavailable");
  expect(navigation.restoreChannelDraft("channel-a")).toBe("  hei frå skrivefeltet  ");

  journalStorage.rejectWrites = false;
  const accepted = await outbox.enqueue({
    requestId: "request-accepted",
    channelId: "channel-a",
    parentMessageId: null,
    body: "hei frå skrivefeltet",
    draft: "  hei frå skrivefeltet  ",
    media: []
  });

  // Composer components are controlled. Persisting a send must not mutate
  // their value; the application clears it explicitly once it accepts the send.
  expect(navigation.restoreChannelDraft("channel-a")).toBe("  hei frå skrivefeltet  ");
  expect(accepted).toMatchObject({
    requestId: "request-accepted",
    channelId: "channel-a",
    body: "hei frå skrivefeltet",
    draft: "  hei frå skrivefeltet  "
  });
  expect(outbox.pending().map((entry) => entry.requestId)).toEqual(["request-accepted"]);

  navigation.persistChannelDraft("channel-a", "");
  expect(navigation.restoreChannelDraft("channel-a")).toBe("");
  expect(outbox.pending().map((entry) => entry.requestId)).toEqual(["request-accepted"]);
});
