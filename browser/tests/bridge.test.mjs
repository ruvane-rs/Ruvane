import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { decodeCommands, validateExports, validateProps, LIMITS } from "../../web/bridge.mjs";

function frame(payload = [], count = 0) {
  return new Uint8Array([82, 86, 78, 49, count, 0, 0, 0, ...payload]);
}
const word = n => [n & 255, (n >>> 8) & 255, (n >>> 16) & 255, (n >>> 24) & 255];

test("strict command framing, identifiers and UTF-8", () => {
  assert.deepEqual(decodeCommands(frame()), []);
  const text = new TextEncoder().encode("Rust <script> 🌱");
  assert.equal(decodeCommands(frame([2, ...word(1), ...word(2), ...word(text.length), ...text], 1))[0].text, "Rust <script> 🌱");
  const bom = new TextEncoder().encode("\uFEFFtext");
  assert.equal(decodeCommands(frame([2, ...word(1), ...word(2), ...word(bom.length), ...bom], 1))[0].text, "\uFEFFtext");
  for (const data of [new Uint8Array(7), new Uint8Array(LIMITS.bytes + 1), frame([0]), frame([], 129), frame([99, ...word(1)], 1), frame([5, ...word(0)], 1), frame([4, ...word(1), ...word(1), 2], 1), frame([2, ...word(1), ...word(1), ...word(1), 255], 1)]) {
    assert.throws(() => decodeCommands(data));
  }
});

test("every truncated prefix fails and trailing bytes are rejected", () => {
  const valid = frame([1, ...word(1), ...word(2), ...word(0), 1], 1);
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
  assert.equal(e.ruvane_abi_version(), 2);
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
  assert.equal(e.ruvane_dispatch(view, 1), 1);
  assert.deepEqual(drain().filter(c => c.op === 2).map(c => c.text), ["6", "12"]);
  assert.equal(e.ruvane_dispatch(view, 3), 1); drain();
  assert.equal(e.ruvane_dispatch(view, 999), 0);
  assert.equal(e.ruvane_last_error(), 4);
  assert.equal(e.ruvane_dispose(view), 1);
  assert.deepEqual(drain(), [{ op: 5, view }]);
  assert.equal(e.ruvane_dispatch(view, 1), 0);
  const toggle = e.ruvane_mount(2, 2, 2, 1);
  assert.ok(toggle > view);
  assert.ok(drain().some(c => c.op === 2 && c.text === "Enabled"));
  assert.equal(e.ruvane_dispatch(toggle, 7), 1);
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
