import assert from "node:assert/strict";
import { test } from "node:test";
import type { AgentPlaceLookupApi } from "../src/agent-place-lookup";
import { AgentPlaceDiscovery } from "../src/ui/react/agent-place-discovery";

const tick = () => new Promise<void>(resolve => setTimeout(resolve, 0));

test("discovery batches loaded messages and fences late results after a channel change", async () => {
  const calls: { owner: string; channel: string; ids: readonly string[]; resolve: (ids: { messageIds: readonly string[]; googleMessageIds: readonly string[]; geoapifyMessageIds: readonly string[] }) => void; signal?: AbortSignal }[] = [];
  const api = { list: (owner: string, channel: string, ids: readonly string[], _admitted: () => boolean, signal?: AbortSignal) =>
    new Promise<{ messageIds: readonly string[]; googleMessageIds: readonly string[]; geoapifyMessageIds: readonly string[] }>(resolve => calls.push({ owner, channel, ids, resolve, signal })) } as unknown as AgentPlaceLookupApi;
  let renders = 0;
  const discovery = new AgentPlaceDiscovery(api, () => { renders++; });
  const candidates = Array.from({ length: 65 }, (_, index) => ({ id: String(index), revision: "first" }));
  discovery.sync("alice", "first", 1, candidates, () => true);
  await tick();
  assert.deepEqual(calls.map(call => call.ids.length), [30, 30, 5]);
  calls[0]!.resolve({ messageIds: ["0"], googleMessageIds: [], geoapifyMessageIds: ["0"] });
  await tick();
  assert.equal(discovery.status("0"), "yes");
  assert.equal(discovery.geoapifyAvailable("0"), true);
  assert.equal(discovery.googleAvailable("0"), false);
  discovery.sync("alice", "second", 1, [{ id: "30", revision: "first" }], () => true);
  assert.equal(calls[1]!.signal?.aborted, true);
  calls[1]!.resolve({ messageIds: ["30"], googleMessageIds: ["30"], geoapifyMessageIds: ["30"] });
  await tick();
  assert.equal(discovery.status("30"), "pending");
  calls[3]!.resolve({ messageIds: [], googleMessageIds: [], geoapifyMessageIds: [] });
  await tick();
  assert.equal(discovery.status("30"), "no");
  assert.equal(discovery.geoapifyAvailable("30"), false);
  assert.equal(renders, 2);
  discovery.dispose();
});
