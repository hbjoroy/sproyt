import assert from "node:assert/strict";
import test from "node:test";
import { createCommunityRequests } from "../src/application/community-requests";
import { createCommunityHost } from "../src/application/community-host";
import type { ServerEvent } from "../src/types";

test("community requests retire pending reads on account ABA and disconnect", async () => {
  const requests = createCommunityRequests(); requests.bindOwner("A");
  const epoch = requests.generation;
  const old = requests.request("users_listed", () => "old");
  const rejection = assert.rejects(old, /Kontoen/);
  requests.bindOwner("B"); requests.bindOwner("A"); await rejection;
  assert.ok(requests.generation > epoch); assert.ok(requests.isRetired("old"));
  requests.observe({ protocol: "sproyt.chat.v1", type: "users_listed", request_id: "old", payload: { users: [] } });
  const latest = requests.request("users_listed", () => "new");
  requests.observe({ protocol: "sproyt.chat.v1", type: "users_listed", request_id: "new", payload: { users: [] } });
  assert.deepEqual((await latest).payload.users, []);
  const disconnected = requests.request("users_listed", () => "disconnected");
  const failed = assert.rejects(disconnected, /Sambandet/); requests.reset(); await failed;
  assert.ok(requests.isRetired("disconnected"));
});

for (const boundary of ["invitation", "delivery"] as const) test(`an invitation chain cannot cross accounts after ${boundary} acknowledgement`, async () => {
  let owner = "A", serial = 0;
  const requests = createCommunityRequests(); requests.bindOwner(owner);
  const commands: string[] = [];
  const send = (type: string) => { commands.push(type); return `r${++serial}`; };
  const host = createCommunityHost({ requests, selfId: () => owner,
    channels: () => [], circles: () => new Map([["circle", { id: "circle", name: "Friends", slug: "friends", role: "owner", created_by: "A", created_at: "" }]]),
    send: (type: string, ..._args: unknown[]) => send(type), openDirect: () => send("open"), openDirectForDelivery: () => send("delivery"),
    slugify: value => value, channelSlug: (_id, value) => value, invitationToken: value => value,
    enrollment: {} as Parameters<typeof createCommunityHost>[0]["enrollment"] });
  const operation = host.invite("circle", undefined, "friend");
  const failed = assert.rejects(operation, /Kontoen/);
  requests.observe({ protocol: "sproyt.chat.v1", type: "invitation_created", request_id: "r1",
    payload: { invitation: { token: "test-token" } } } as unknown as ServerEvent);
  if (boundary === "delivery") {
    await new Promise(resolve => setTimeout(resolve, 0));
    assert.deepEqual(commands, ["create_invitation", "delivery"]);
    requests.observe({ protocol: "sproyt.chat.v1", type: "direct_channel_opened", request_id: "r2",
      payload: { channel: { id: "delivery-channel" } } } as unknown as ServerEvent);
  }
  owner = "B"; requests.bindOwner(owner); owner = "A"; requests.bindOwner(owner);
  await failed;
  assert.ok(!commands.includes("send_message"));
  assert.equal(commands.length, boundary === "invitation" ? 1 : 2);
});
