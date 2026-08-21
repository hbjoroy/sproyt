import { expect, test } from "@playwright/test";
import { readJson } from "../src/api";
import { hasUnsupportedProtocol, parseSocketEvent } from "../src/connection";
import { asWireEvent, parseStoredStringMap } from "../src/types";

test("wire events reject malformed and unsupported frames", () => {
  const valid = {
    protocol: "sproyt.chat.v1",
    request_id: "r1",
    type: "hello",
    payload: { participant_id: "u1" }
  };
  expect(asWireEvent(valid)).toEqual(valid);
  expect(asWireEvent({ ...valid, protocol: "sproyt.chat.v2" })).toBeNull();
  expect(hasUnsupportedProtocol(JSON.stringify({ ...valid, protocol: "sproyt.chat.v2" }))).toBe(true);
  expect(hasUnsupportedProtocol("not json")).toBe(false);
  expect(asWireEvent({ ...valid, type: "future_event" })).toBeNull();
  expect(asWireEvent({ ...valid, request_id: 4 })).toBeNull();
  expect(asWireEvent({ ...valid, payload: null })).toBeNull();
  expect(asWireEvent({ ...valid, payload: { participant_id: undefined } })).toBeNull();
  expect(asWireEvent([valid])).toBeNull();
  expect(parseSocketEvent("not json")).toBeNull();
  expect(parseSocketEvent(new Uint8Array())).toBeNull();
});

test("wire events accept Rust base and summary DTOs without conflating their shapes", () => {
  const baseChannel = { id: "c1", slug: "prat", name: "Prat", kind: "private", circle_id: "circle-1", created_by: "u1" };
  expect(asWireEvent({ protocol: "sproyt.chat.v1", type: "channel_created", payload: { channel: baseChannel } })).not.toBeNull();
  expect(asWireEvent({ protocol: "sproyt.chat.v1", type: "channels_listed", payload: { channels: [{ id: "c1", slug: "prat", name: "Prat", kind: "private", circle_id: "circle-1", direct_user_id: null, description: "", role: "owner", last_read_sequence: 0, latest_sequence: 0 }] } })).not.toBeNull();
  const circle = { id: "circle-1", slug: "venner", name: "Venner", created_by: "u1", created_at: "2026-08-20T08:00:00Z" };
  expect(asWireEvent({ protocol: "sproyt.chat.v1", type: "circle_created", payload: { circle } })).not.toBeNull();
  expect(asWireEvent({ protocol: "sproyt.chat.v1", type: "circles_listed", payload: { circles: [[circle, "owner"]] } })).not.toBeNull();
});

test("storage parser retains only bounded string mappings", () => {
  expect(parseStoredStringMap(null)).toEqual({});
  expect(parseStoredStringMap("not json")).toEqual({});
  expect(parseStoredStringMap("[]")).toEqual({});
  expect(parseStoredStringMap('{"circle":"channel","poison":3}')).toEqual({ circle: "channel" });
  expect(parseStoredStringMap(JSON.stringify({ ["x".repeat(129)]: "channel" }))).toEqual({});
});

test("JSON response reader validates the JSON value boundary", async () => {
  await expect(readJson(new Response(""))).resolves.toBeNull();
  await expect(readJson(new Response('{"ok":true}'))).resolves.toEqual({ ok: true });
  await expect(readJson(new Response("not json"))).rejects.toThrow();
});
