import assert from "node:assert/strict";
import test from "node:test";

import { typed } from "../dist/index.js";

function failure(call) {
  try {
    call();
  } catch (error) {
    return error;
  }
  throw new Error("the call was expected to fail");
}

test("an export reports its own failure instead of trapping", () => {
  assert.equal(typed("invoke", () => "value", "app_tool"), "value");
  const thrown = failure(() => typed("invoke", () => { throw new Error("boom"); }, "app_tool"));
  assert.deepEqual(thrown.payload, {
    code: "app_tool_invoke_failed",
    message: "boom",
    retryable: false,
  });
});

test("a host import's own typed payload survives verbatim", () => {
  const payload = { payload: { code: "host_denied", message: "no", retryable: true } };
  assert.equal(failure(() => typed("observe", () => { throw payload; }, "app_environment")), payload);
});
