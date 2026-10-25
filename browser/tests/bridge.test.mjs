import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { decodeCommands, validateExports, validateProps, planCommands, LIMITS } from "../../web/bridge.mjs";

function frame(payload = [], count = 0) {
  return new Uint8Array([82, 86, 78, 50, ...word(count), ...payload]);
}
const word = n => [n & 255, (n >>> 8) & 255, (n >>> 16) & 255, (n >>> 24) & 255];

test("strict command framing, identifiers and UTF-8", () => {
  assert.deepEqual(decodeCommands(frame()), []);
  const text = new TextEncoder().encode("Rust <script> 🌱");
  assert.equal(decodeCommands(frame([2, ...word(1), ...word(2), ...word(text.length), ...text], 1))[0].text, "Rust <script> 🌱");
  const bom = new TextEncoder().encode("\uFEFFtext");
  assert.equal(decodeCommands(frame([2, ...word(1), ...word(2), ...word(bom.length), ...bom], 1))[0].text, "\uFEFFtext");
  for (const data of [new Uint8Array(7), new Uint8Array(LIMITS.bytes + 1), frame([0]), frame([], LIMITS.commands + 1), frame([99, ...word(1)], 1), frame([5, ...word(0)], 1), frame([4, ...word(1), ...word(1), 2], 1), frame([2, ...word(1), ...word(1), ...word(1), 255], 1)]) {
    assert.throws(() => decodeCommands(data));
  }
});

test("every truncated prefix fails and trailing bytes are rejected", () => {
  const valid = frame([1, ...word(1), ...word(2), ...word(0), 1, ...word(0)], 1);
  assert.equal(decodeCommands(valid).length, 1);
  for (let size = 0; size < valid.length; size++) assert.throws(() => decodeCommands(valid.slice(0, size)));
  assert.throws(() => decodeCommands(new Uint8Array([...valid, 0])));
});

test("deterministic mutation corpus stays within decoder budgets", () => {
  const original = frame([2, ...word(1), ...word(2), ...word(4), 116, 101, 120, 116], 1);
  for (let index = 0; index < original.length; index++) {
    for (let value = 0; value < 256; value++) {
      const bytes = original.slice(); bytes[index] = value;
      let commands;
      try { commands = decodeCommands(bytes); }
      catch (error) { assert.ok(error instanceof Error); continue; }
      assert.ok(commands.length <= LIMITS.commands);
      for (const command of commands) if (command.op === 2) assert.ok(new TextEncoder().encode(command.text).length <= LIMITS.text);
    }
  }
});

test("compiled Rust module has no host imports and executes the real reactor", async () => {
  const file = process.env.RUVANE_WASM_FILE || new URL("../../target/wasm32-unknown-unknown/release/ruvane_browser.wasm", import.meta.url);
  const module = await WebAssembly.compile(await readFile(file));
  assert.deepEqual(WebAssembly.Module.imports(module), []);
  const e = validateExports(new WebAssembly.Instance(module, {}).exports);
  assert.throws(() => validateExports({ ...e, ruvane_abi_version: () => 0 }));
  assert.throws(() => validateExports({ ...e, ruvane_dispatch: null }));
  const drain = () => {
    const commands = decodeCommands(new Uint8Array(e.memory.buffer, e.ruvane_commands_ptr() >>> 0, e.ruvane_commands_len() >>> 0).slice());
    assert.equal(e.ruvane_commands_clear(), 1);
    return commands;
  };
  assert.equal(e.ruvane_abi_version(), 3);
  assert.equal(e.ruvane_mount(0, 1, 1, 0), 0);
  assert.equal(e.ruvane_last_error(), 1);
  for (const args of [[1, 1, 2, 1], [1, 2, 2, 2], [1, 1, 1, 1001], [1, 99, 1, 0]]) {
    assert.equal(e.ruvane_mount(...args), 0);
    assert.equal(e.ruvane_last_error(), 7);
    assert.equal(e.ruvane_live_instances(), 0);
  }
  const view = e.ruvane_mount(1, 1, 1, 5); assert.ok(view > 0);
  const initial = drain();
  assert.deepEqual(initial.filter(c => c.op === 2 && [5, 8].includes(c.node)).map(c => c.text), ["5", "10"]);
  assert.equal(initial.filter(c => c.op === 3).length, 3);
  assert.equal(e.ruvane_dispatch(view, 9, 1), 1);
  assert.deepEqual(drain().filter(c => c.op === 2).map(c => c.text), ["6", "12"]);
  assert.equal(e.ruvane_dispatch(view, 11, 3), 1); drain();
  assert.equal(e.ruvane_dispatch(view, 9, 999), 0);
  assert.equal(e.ruvane_last_error(), 4);
  assert.equal(e.ruvane_dispose(view), 1);
  assert.deepEqual(drain(), [{ op: 5, view }]);
  assert.equal(e.ruvane_dispatch(view, 9, 1), 0);
  const toggle = e.ruvane_mount(2, 2, 2, 1);
  assert.ok(toggle > view);
  assert.ok(drain().some(c => c.op === 2 && c.text === "Enabled"));
  assert.equal(e.ruvane_dispatch(toggle, 6, 7), 1);
  assert.deepEqual(drain().filter(c => c.op === 2).map(c => c.text), ["Disabled"]);
  assert.equal(e.ruvane_dispose(toggle), 1); drain();
  assert.deepEqual([e.ruvane_live_instances(), e.ruvane_live_signals(), e.ruvane_live_effects()], [0, 0, 0]);
});

test("mount props accept only typed u32 and boolean state", () => {
  assert.deepEqual(validateProps({ component: 1, state: { kind: "number", value: 5 } }), { component: 1, kind: 1, value: 5 });
  assert.deepEqual(validateProps({ component: 2, state: { kind: "boolean", value: true } }), { component: 2, kind: 2, value: 1 });
  for (const props of [null, {}, { component: 0, state: { kind: "number", value: 0 } }, { component: 1, state: { kind: "boolean", value: 1 } }, { component: 1, state: { kind: "text", value: "<script>" } }]) assert.throws(() => validateProps(props));
  for (const value of [-1, 0x100000000, NaN, Infinity, 1.5, "5"]) assert.throws(() => validateProps({ component: 1, state: { kind: "number", value } }));
});

const blank = () => ({ nodes: new Map([[0, { tag: 0, depth: 0, key: 0, children: [], element: null }]]), nextNode: 0 });
const create = (view, node, parent, tag, key = 0) => ({ op: 1, view, node, parent, tag, key });
const label = (node, text = "text") => ({ op: 2, view: 1, node, text });

test("tree planning rejects invalid structural batches without changing the previous plan", () => {
  const previous = planCommands([create(1, 1, 0, 7, 1), create(1, 2, 1, 8, 1), create(1, 3, 2, 4, 1), label(3)], 1, blank(), true);
  const before = JSON.stringify([...previous.nodes]);
  const invalid = [
    [create(1, 4, 1, 8, 1)],
    [create(1, 4, 1, 2, 2), label(4)],
    [create(1, 3, 1, 8, 2)],
    [{ op: 6, view: 2, node: 2 }],
    [{ op: 6, view: 1, node: 99 }],
    [{ op: 7, view: 1, node: 2, before: 3 }],
    [{ op: 7, view: 1, node: 2, before: 2 }],
    [{ op: 3, view: 1, node: 3, event: 1 }],
    [label(3, "changed"), { op: 6, view: 1, node: 2 }],
    [{ op: 6, view: 1, node: 2 }, create(1, 4, 1, 8, 2), { op: 6, view: 1, node: 4 }],
    [{ op: 6, view: 1, node: 2 }, { op: 6, view: 1, node: 1 }],
  ];
  for (const batch of invalid) {
    assert.throws(() => planCommands(batch, 1, previous));
    assert.equal(JSON.stringify([...previous.nodes]), before);
  }
  const changed = planCommands([{ op: 6, view: 1, node: 2 }, create(1, 4, 1, 8, 1)], 1, previous);
  assert.deepEqual(changed.nodes.get(1).children, [4]);
  assert.equal(changed.nextNode, 4);
  assert.equal(changed.nodes.has(2), false);
  assert.equal(changed.nodes.has(3), false);
  assert.equal(JSON.stringify([...previous.nodes]), before);
});

test("compiled keyed list preserves handles, retires bindings and releases removed item scopes", async () => {
  const file = process.env.RUVANE_WASM_FILE || new URL("../../target/wasm32-unknown-unknown/release/ruvane_browser.wasm", import.meta.url);
  const { instance } = await WebAssembly.instantiate(await readFile(file), {});
  const e = validateExports(instance.exports);
  const id = e.ruvane_mount(1, 3, 1, 3);
  assert.ok(id > 0);
  let model = blank();
  const batch = initial => {
    const commands = decodeCommands(new Uint8Array(e.memory.buffer, e.ruvane_commands_ptr() >>> 0, e.ruvane_commands_len() >>> 0).slice());
    model = planCommands(commands, id, model, initial);
    assert.equal(e.ruvane_commands_clear(), 1);
    return commands;
  };
  const run = event => {
    const [node] = [...model.nodes].find(([, n]) => n.event === event);
    assert.equal(e.ruvane_dispatch(id, node, event), 1);
    return batch(false);
  };
  batch(true);
  const [list] = [...model.nodes].find(([, n]) => n.tag === 7);
  const initial = [...model.nodes.get(list).children];
  const oldB = [...model.nodes].find(([, n]) => n.event === 24)[0];
  run(20); run(26);
  assert.equal(e.ruvane_live_signals(), 4);
  assert.equal(e.ruvane_dispatch(id, oldB, 24), 0);
  assert.equal(e.ruvane_last_error(), 4);
  const moved = run(2);
  assert.ok(moved.some(c => c.op === 7));
  assert.deepEqual(model.nodes.get(list).children, [initial[2], initial[0]]);
  const oldA = [...model.nodes].find(([, n]) => n.event === 20)[0];
  run(21); run(3);
  assert.equal(model.nodes.has(initial[0]), false);
  assert.equal(e.ruvane_live_signals(), 4);
  run(3);
  assert.notEqual(model.nodes.get(list).children[1], initial[0]);
  assert.equal(e.ruvane_dispatch(id, oldA, 20), 0);
  assert.ok([...model.nodes.values()].some(n => n.tag === 5 && n.text === "1"));
  for (let n = 0; n < 1000; n++) {
    run(1);
    const last = Math.max(...[...model.nodes.values()].filter(n => n.tag === 8).map(n => n.key));
    const old = [...model.nodes].find(([, node]) => node.event === 16 + last * 4)[0];
    run(18 + last * 4);
    assert.equal(e.ruvane_live_signals(), 4);
    assert.equal(e.ruvane_dispatch(id, old, 16 + last * 4), 0);
    assert.equal(model.nodes.size, 20);
  }
  assert.equal(e.ruvane_dispose(id), 1); e.ruvane_commands_clear();
  assert.deepEqual([e.ruvane_live_instances(), e.ruvane_live_signals(), e.ruvane_live_effects()], [0, 0, 0]);
});
