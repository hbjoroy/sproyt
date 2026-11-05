import assert from "node:assert/strict";
import test from "node:test";
import { personalMessageReference } from "./personal-space";

test("only standalone local message links are resolved as personal message references", () => {
  const origin = "https://sproyt.example";
  const channelId = "018f0000-0000-7000-8000-000000000001";
  const messageId = "018f0000-0000-7000-8000-000000000002";
  const threadId = "018f0000-0000-7000-8000-000000000003";
  const link = `/?channel=${channelId}&message=${messageId}&sequence=7&thread=${threadId}`;
  const expected = { channelId, messageId, threadId, sequence: 7 };
  assert.deepEqual(personalMessageReference(`[[${origin}${link}]]`, origin), expected);
  assert.deepEqual(personalMessageReference(`[Lagra melding](${link})`, origin), expected);
  assert.deepEqual(personalMessageReference(`[[[Lagra melding](${link})]]`, origin), expected);
  for (const body of [
    `[[https://other.example${link}]]`,
    `[[${link}&sequence=8]]`,
    `[[${link.replace(messageId, "unknown")}]]`,
    `Tekst [[${link}]]`,
    `\`[[${link}]]\``
  ]) assert.equal(personalMessageReference(body, origin), null, body);
});
