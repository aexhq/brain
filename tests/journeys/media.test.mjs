import assert from "node:assert/strict";
import test from "node:test";
import { fixture, collect, failure } from "./support.mjs";

const f = fixture();
const media = [
  { type: "image", url: "https://media.example.com/diagram.png?token=image" },
  { type: "file", media_type: "application/pdf", url: "https://media.example.com/report.pdf?token=pdf" },
];

test("discovery is authenticated, omits endpoints, and retains models.dev modalities", async () => {
  await assert.rejects(f.client({ token: "wrong" }).models(), failure(401));
  const discovery = await f.brain.models("openai");
  assert.equal(discovery.providers.length, 1);
  const provider = discovery.providers[0];
  assert.equal(provider.dialect, "openai_responses");
  assert.deepEqual(provider.media_inputs, ["image", "application/pdf"]);
  assert.ok(provider.models.some(model => model.input_modalities?.includes("image")));
  assert.ok(discovery.snapshot_digest.length > 0);
  assert.equal(provider.base_url, undefined);
  await assert.rejects(f.brain.models("openai-responses"), failure(400));
});

test("HTTPS images and PDFs survive a real Component, journal reload, and continuation", { timeout: 30_000 }, async t => {
  const session = await f.create(t);
  await session.send({ message: "Read the diagram and report", media });
  assert.deepEqual((await session.transcript()).messages[0].content.slice(1), media);
  const parts = f.modelRequests.at(-1).input.flatMap(item => Array.isArray(item.content) ? item.content : []);
  assert.deepEqual(parts, [
    { type: "input_image", image_url: media[0].url },
    { type: "input_file", file_url: media[1].url },
  ]);
  await f.stop();
  await f.start();
  const reopened = await f.brain.sessions.get(session.id);
  assert.deepEqual((await reopened.transcript()).messages[0].content.slice(1), media);
  await reopened.send("Compare them again");
  assert.ok(JSON.stringify(f.modelRequests.at(-1).input).includes(media[1].url));
  assert.ok((await collect(reopened.events())).some(event => event.type === "model_call_ended"));
  const calls = f.modelRequests.length;
  for (const url of ["data:image/png;base64,AAAA", "s3://bucket/file", "file:///tmp/report.pdf", "http://media.example.com/report.pdf"]) {
    await assert.rejects(reopened.send({ message: "invalid media", media: [{ ...media[1], url }] }), failure(400));
  }
  assert.equal(f.modelRequests.length, calls);
});
