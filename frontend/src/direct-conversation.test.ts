import assert from "node:assert/strict";
import test from "node:test";
import { directConversationTitle, existingDirectChannel } from "./direct-conversation";
import type { Channel } from "./types";

const channel = (changes: Partial<Channel> = {}): Channel => ({ id: "original-one-to-one", slug: "direct", name: "Lagra tittel",
  is_direct: true, direct_user_id: "ada", circle_id: null, kind: "private", role: "member", description: "", last_read_sequence: 0, latest_sequence: 0, ...changes });

test("one-to-one titles use the current display name while handles and stored group names remain separate", () => {
  const profiles = [{ id: "ada", display_name: "Ada Lovelace" }, { id: "grace", display_name: "Grace Hopper" }];
  assert.equal(directConversationTitle(channel(), profiles), "Ada Lovelace");
  assert.equal(directConversationTitle(channel(), []), "Lagra tittel");
  assert.equal(directConversationTitle(channel({ direct_user_id: null, name: "Ada Lovelace, Grace Hopper og meg" }), profiles), "Ada Lovelace, Grace Hopper og meg");
  assert.equal(directConversationTitle(channel({ direct_user_id: null, name: "" }), profiles), "Gruppesamtale");
  assert.equal(directConversationTitle(null, profiles), "Direktesamtale");
});

test("an existing direct person selection keeps its channel ID and never selects a group or normal channel", () => {
  const original = channel();
  const group = channel({ id: "separate-group", name: "Ada og vener", direct_user_id: null });
  const normal = channel({ id: "normal-channel", is_direct: false });
  assert.equal(existingDirectChannel([group, normal, original], "ada"), original);
  assert.equal(existingDirectChannel([group, normal], "ada"), undefined);
  assert.equal(existingDirectChannel([original], "grace"), undefined);
  const legacy = channel({ is_direct: undefined });
  assert.equal(existingDirectChannel([legacy], "ada"), legacy);
  assert.equal(original.id, "original-one-to-one");
});
