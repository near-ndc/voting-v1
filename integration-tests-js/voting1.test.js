// nw = require('near-workspaces')
import { Worker } from "near-workspaces";
import test from "ava";

test("init NEAR", async (t) => {
  const worker = await Worker.init();
  t.context.worker = worker;
  const root = worker.rootAccount;
  const alice = await root.createAccount("alice", { initialBalance: "123" });

  const b = await alice.balance();
  t.is(b.total, 2n, "must be not empty");
});

test.afterEach.always(async (t) => {
  await t.context.worker.tearDown().catch((error) => {
    console.log("Failed tear down the worker:", error);
  });
});

// trying with Deno //
// deno test --allow-env --allow-read --allow-net --v8-flags='--max-heap-size=8000,--max-old-space-size=8000'

/*
import { BN, NEAR, Worker } from "npm:near-workspaces";
  port {
  assertEquals,
  assertNotEquals,
  equal,
} from "https://deno.land/std@0.186.0/testing/asserts.ts";

Deno.test("sample test", () => {
  equal(new BN(2), 2n);
  assertNotEquals(new BN(2), 2n);
  assertNotEquals(new BN(2), 2);
});

Deno.test("init NEAR", async () => {
  const worker = await Worker.init();
  const root = worker.rootAccount;
  const alice = await root.createAccount("alice", { initialBalance: "123" });
  const b = await alice.balance();
  assertEquals(b.total, BN(2));
    ait worker.tearDown().catch((error) => {
    console.log("Failed tear down the worker:", error);
  });
});
  
*/
