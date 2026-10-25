import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { decodeCommands, validateExports, validateProps, planCommands, LIMITS } from "../../web/bridge.mjs";

function frame(payload = [], count = 0) {
  return new Uint8Array([82, 86, 78, 54, ...word(count), ...payload]);
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
  assert.equal(e.ruvane_abi_version(), 11);
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

test("typed field commands are complete, bounded and monotonic", () => {
  const commands = [create(1, 1, 0, 9, 1), { op: 3, view: 1, node: 1, event: 1 },
    create(1, 2, 1, 10, 1), create(1, 3, 2, 4, 1), label(3, "Name"), create(1, 4, 2, 11, 2),
    { op: 3, view: 1, node: 4, event: 5 }, { op: 4, view: 1, node: 4, disabled: false },
    { op: 8, view: 1, node: 4, value: "Rust 🦀", epoch: 0 }, { op: 10, view: 1, node: 4, invalid: false }];
  const initial = planCommands(commands, 1, blank(), true);
  const changed = planCommands([{ op: 8, view: 1, node: 4, value: "", epoch: 1 }], 1, initial);
  assert.equal(changed.nodes.get(4).value, "");
  assert.equal(changed.nodes.get(4).seed, "Rust 🦀");
  assert.equal(initial.nodes.get(4).value, "Rust 🦀");
  for (const batch of [
    [{ op: 8, view: 1, node: 4, value: "x", epoch: 0 }],
    [{ op: 8, view: 1, node: 4, value: "🦀".repeat(257), epoch: 1 }],
    [{ op: 8, view: 1, node: 4, value: "line\nbreak", epoch: 1 }],
    [{ op: 9, view: 1, node: 4, checked: true, epoch: 1 }],
    [create(1, 5, 1, 9, 2)],
  ]) assert.throws(() => planCommands(batch, 1, changed));
  for (const index of [1, 6, 7, 8, 9]) assert.throws(() => planCommands(commands.filter((_, i) => i !== index), 1, blank(), true));
  const value = new TextEncoder().encode("🦀");
  const encoded = frame([8, ...word(1), ...word(4), ...word(7), ...word(value.length), ...value], 1);
  assert.deepEqual(decodeCommands(encoded), [{ op: 8, view: 1, node: 4, epoch: 7, value: "🦀" }]);
  for (let size = 0; size < encoded.length; size++) assert.throws(() => decodeCommands(encoded.slice(0, size)));
});

test("compiled forms validate staged UTF-8, typed events and stale field handles", async () => {
  const file = process.env.RUVANE_WASM_FILE || new URL("../../target/wasm32-unknown-unknown/release/ruvane_browser.wasm", import.meta.url);
  const { instance } = await WebAssembly.instantiate(await readFile(file), {});
  const e = validateExports(instance.exports), id = e.ruvane_mount(1, 4, 1, 0);
  let model = blank();
  const drain = initial => {
    const commands = decodeCommands(new Uint8Array(e.memory.buffer, e.ruvane_commands_ptr() >>> 0, e.ruvane_commands_len() >>> 0).slice());
    model = planCommands(commands, id, model, initial); e.ruvane_commands_clear(); return commands;
  };
  const binding = event => [...model.nodes].find(([, node]) => node.event === event)[0];
  const run = (event, value) => {
    const node = binding(event); let kind = 0, scalar = 0;
    if (typeof value === "string") {
      kind = 1; const bytes = new TextEncoder().encode(value);
      assert.equal(e.ruvane_event_begin(id, node, event, bytes.length), 1);
      for (let offset = 0; offset < bytes.length; offset += 4) {
        const count = Math.min(4, bytes.length - offset); let word = 0;
        for (let n = 0; n < count; n++) word |= bytes[offset + n] << (8 * n);
        assert.equal(e.ruvane_event_chunk(word >>> 0, count), 1);
      }
    } else if (typeof value === "boolean") { kind = 2; scalar = Number(value); }
    assert.equal(e.ruvane_dispatch(id, node, event, kind, scalar), 1); return drain(false);
  };
  drain(true); const input = binding(5);
  assert.equal(e.ruvane_event_begin(id, input, 5, 1025), 0);
  assert.equal(e.ruvane_last_error(), 9);
  assert.equal(e.ruvane_dispatch(id, input, 5, 2, 1), 0);
  assert.equal(e.ruvane_last_error(), 9);
  for (const [length, chunks] of [[2, [[0xc3, 1]]], [1, [[255, 1]]], [1, [[256, 1]]], [1, [[0, 0]]], [1, [[0, 5]]]]) {
    assert.equal(e.ruvane_event_begin(id, input, 5, length), 1);
    assert.equal(e.ruvane_dispose(id), 0); assert.equal(e.ruvane_last_error(), 5);
    for (const chunk of chunks) e.ruvane_event_chunk(...chunk);
    assert.equal(e.ruvane_dispatch(id, input, 5, 1, 0), 0); assert.equal(e.ruvane_last_error(), 9);
    e.ruvane_commands_clear(); assert.equal(e.ruvane_live_instances(), 1);
  }
  assert.equal(e.ruvane_event_begin(id, input, 5, 0), 1);
  e.ruvane_commands_clear(); run(1);
  assert.equal(model.nodes.get(input).invalid, true);
  run(5, "🦀".repeat(256)); run(1);
  assert.ok([...model.nodes.values()].some(node => node.text?.includes("80 UTF-8 bytes")));
  run(5, '<script>alert("x")</script>'); run(6, "Notes 🦀\nline two\t&"); run(1);
  assert.equal(model.nodes.get(input).value, "");
  assert.equal(model.nodes.get(input).invalid, false);
  assert.equal(model.nodes.get(input).epoch, 1);
  assert.ok([...model.nodes.values()].some(node => node.text?.includes('<script>alert("x")</script>')));
  assert.equal(e.ruvane_live_signals(), 1);
  const checked = binding(20); run(20, true);
  assert.equal(model.nodes.get(checked).checked, true);
  run(3); assert.equal(model.nodes.has(checked), false);
  assert.equal(e.ruvane_dispatch(id, checked, 20, 2, 0), 0); assert.equal(e.ruvane_last_error(), 4);
  run(3); assert.notEqual(binding(20), checked); run(21);
  assert.equal(e.ruvane_live_signals(), 0);
  for (let n = 0; n < 3; n++) { run(5, `Task ${n}`); run(1); }
  assert.equal(e.ruvane_live_signals(), 3); assert.ok(model.nodes.size <= 33);
  assert.equal(e.ruvane_dispose(id), 1); e.ruvane_commands_clear();
  assert.deepEqual([e.ruvane_live_instances(), e.ruvane_live_signals(), e.ruvane_live_effects()], [0, 0, 0]);
});

test("action commands reject external routes, invalid paths, oversized bodies and truncation", () => {
  const encode = (url, body = "q=1") => {
    const path = new TextEncoder().encode(url), bytes = new TextEncoder().encode(body);
    return frame([11, ...word(1), ...word(2), ...word(99), ...word(path.length), ...path, ...word(bytes.length), ...bytes], 1);
  };
  const bytes = encode("/actions/tasks/create", "q=Rust%20%2B%20HTML");
  assert.deepEqual(decodeCommands(bytes), [{ op: 11, view: 1, request: 2, action: 99, url: "/actions/tasks/create", body: "q=Rust%20%2B%20HTML" }]);
  for (let n = 0; n < bytes.length; n++) assert.throws(() => decodeCommands(bytes.slice(0, n)));
  for (const url of ["", "https://evil.example/a", "//evil.example/a", "/a/../b", "/a/%2e%2e/b", "/%2fother", "/%5cother", "/%FF", "/a?b=1", "/a#b", "/a\nb", "/" + "x".repeat(1024)]) assert.throws(() => decodeCommands(encode(url)));
  for (const body of ["q=raw space", "q=\r\n", "x".repeat(16385)]) assert.throws(() => decodeCommands(encode("/actions", body)));
});

test("compiled action callbacks validate bytes, recover from malformed replies and retire request tokens", async () => {
  const file = process.env.RUVANE_WASM_FILE || new URL("../../target/wasm32-unknown-unknown/release/ruvane_browser.wasm", import.meta.url);
  const { instance } = await WebAssembly.instantiate(await readFile(file), {}); const e = validateExports(instance.exports);
  const drain = () => { const batch = decodeCommands(new Uint8Array(e.memory.buffer, e.ruvane_commands_ptr() >>> 0, e.ruvane_commands_len() >>> 0).slice()); e.ruvane_commands_clear(); return batch; };
  const id = e.ruvane_mount(1, 6, 1, 0), initial = drain();
  const request = initial.find(command => command.op === 11);
  assert.equal(request.url, "/actions/tasks/list"); assert.equal(request.body, "");
  assert.equal(e.ruvane_action_begin(id, request.request + 1, 0), 0); assert.equal(e.ruvane_last_error(), 10);
  assert.equal(e.ruvane_action_begin(id, request.request, 65537), 0); assert.equal(e.ruvane_last_error(), 9);
  const complete = (token, bytes) => {
    assert.equal(e.ruvane_action_begin(id, token, bytes.length), 1);
    assert.equal(e.ruvane_dispose(id), 0); assert.equal(e.ruvane_last_error(), 5);
    for (let offset = 0; offset < bytes.length; offset += 4) {
      const count = Math.min(4, bytes.length - offset); let word = 0;
      for (let n = 0; n < count; n++) word |= bytes[offset + n] << (8 * n);
      assert.equal(e.ruvane_action_chunk(word >>> 0, count), 1);
    }
    return e.ruvane_action_complete(id, token, 0);
  };
  assert.equal(complete(request.request, new Uint8Array([82,86,65,49,0,1,0,1,97,3,2])), 2);
  const failed = drain(); assert.ok(failed.some(command => command.text?.includes("Invalid server response")));
  const refresh = initial.find(command => command.op === 3 && command.event === 4).node;
  assert.equal(e.ruvane_dispatch(id, refresh, 4, 0, 0), 1);
  const next = drain().find(command => command.op === 11); assert.ok(next.request > request.request);
  const reply = new Uint8Array([82,86,65,49,0,1,0,5,99,111,117,110,116,2,0,0,0,0,0,0,0,0]);
  assert.equal(complete(next.request, reply), 1); drain();
  assert.equal(e.ruvane_action_complete(id, next.request, 2), 0); assert.equal(e.ruvane_last_error(), 10);
  assert.equal(e.ruvane_dispatch(id, refresh, 4, 0, 0), 1); const last = drain().find(command => command.op === 11);
  assert.equal(e.ruvane_dispose(id), 1); drain();
  assert.equal(e.ruvane_action_complete(id, last.request, 0), 0); assert.equal(e.ruvane_last_error(), 10);
  assert.deepEqual([e.ruvane_live_instances(), e.ruvane_live_signals(), e.ruvane_live_effects()], [0,0,0]);
});

function pagePacket(url = "/app", component = 8, seed = new Uint8Array([82,86,65,49,0,1,0,5,99,111,117,110,116,2,0,0,0,0,0,0,0,0])) {
  const encode = text => { const bytes = new TextEncoder().encode(text); return [...word(bytes.length), ...bytes]; };
  return new Uint8Array([82,86,80,49,200,0,...encode(url),...encode("Home / Ruvane"),...word(component),...word(1),...word(0),...word(seed.length),...seed]);
}
test("page metadata and link targets reject ambiguous URLs, malformed packets and hostile schemes", async () => {
  const { decodePage, internalTarget } = await import("../../web/bridge.mjs");
  const bytes = pagePacket(); assert.equal(decodePage(bytes).url, "/app");
  for (let n = 0; n < bytes.length; n++) assert.throws(() => decodePage(bytes.slice(0, n)));
  assert.throws(() => decodePage(new Uint8Array([...bytes,0])));
  for (const url of ["//evil", "https://evil/app", "javascript:alert(1)", "/a#hash", "/a/../app", "/a/%2e%2E", "/a/%2f", "/a/%5c", "/app?q=%FF", "/app?q=1&%71=2", "/app?q=1&", "/app?q=%C2%85"]) {
    assert.throws(() => internalTarget(url)); assert.throws(() => decodePage(pagePacket(url)));
  }
  for (const url of ["/app", "/app?", "/app/tasks/1?q=a+b", "/app?q=%252F"]) assert.equal(internalTarget(url), url);
  const link = [{ op: 1, view: 1, node: 1, parent: 0, tag: 15, key: 1 }, { op: 2, view: 1, node: 1, text: "Open" }, { op: 12, view: 1, node: 1, url: "/app" }, { op: 13, view: 1, node: 1, prefetch: false }];
  const initial = planCommands(link, 1, blank(), true); assert.equal(initial.nodes.get(1).url, "/app");
  for (const url of ["//evil", "/a/..", "javascript:x"]) assert.throws(() => planCommands([{ op: 12, view: 1, node: 1, url }], 1, initial));
  assert.throws(() => planCommands(link.slice(0,2), 1, blank(), true));
  const encoded = frame([12,...word(1),...word(1),...word(4),47,97,112,112],1);
  assert.equal(decodeCommands(encoded)[0].url, "/app");
});
test("compiled page ingress validates the shared registry and seed before creating an owner", async () => {
  const file = process.env.RUVANE_WASM_FILE || new URL("../../target/wasm32-unknown-unknown/release/ruvane_browser.wasm", import.meta.url);
  const { instance } = await WebAssembly.instantiate(await readFile(file), {}), e = validateExports(instance.exports);
  const counter = e.ruvane_mount(1,1,1,5); assert.ok(counter); e.ruvane_commands_clear();
  const stage = bytes => {
    assert.equal(e.ruvane_page_begin(bytes.length), 1);
    for (let offset = 0; offset < bytes.length; offset += 4) { let word = 0; const count = Math.min(4, bytes.length - offset); for (let i = 0; i < count; i++) word |= bytes[offset+i] << (i*8); assert.equal(e.ruvane_page_chunk(word >>> 0,count), 1); }
    return e.ruvane_page_mount(2);
  };
  for (const bytes of [pagePacket("/other"), pagePacket("/app",9), pagePacket("/app",8,new Uint8Array([82,86,65,49,0,0,0]))]) {
    assert.equal(stage(bytes), 0); assert.equal(e.ruvane_last_error(),11); assert.equal(e.ruvane_live_instances(),1); assert.deepEqual(decodeCommands(new Uint8Array(e.memory.buffer,e.ruvane_commands_ptr(),e.ruvane_commands_len())), []);
  }
  const page = stage(pagePacket("/app/tasks",9)); assert.ok(page);
  const commands = decodeCommands(new Uint8Array(e.memory.buffer,e.ruvane_commands_ptr(),e.ruvane_commands_len()).slice());
  assert.ok(commands.some(c => c.op === 12 && c.url === "/app")); assert.ok(!commands.some(c => c.op === 11));
  planCommands(commands,page,blank(),true); e.ruvane_commands_clear();
  assert.equal(e.ruvane_live_instances(),2); assert.equal(e.ruvane_dispose(page),1); e.ruvane_commands_clear();
  assert.equal(e.ruvane_dispatch(counter,9,1,0,0),1); e.ruvane_commands_clear();
  assert.equal(e.ruvane_dispose(counter),1); e.ruvane_commands_clear(); assert.equal(e.ruvane_live_instances(),0);
});

test("cache expiration accepts only explicit public freshness and the versioned Vary profile", async () => {
  const { cacheFreshness } = await import("../../web/navigation.mjs");
  const began = performance.now(), base = { "Cache-Control":"public, max-age=30, must-revalidate", "Vary":"Accept, Ruvane-Application-Revision" };
  const response = (headers={},status=200) => new Response("packet",{status,headers:{...base,...headers}});
  assert.equal(cacheFreshness(response(),30000,began),began+30000);
  assert.equal(cacheFreshness(response({Age:"29"}),30000,began),began+1000);
  assert.equal(cacheFreshness(response({Age:"2"}),5000,began),began+5000);
  for(const headers of [{"Cache-Control":"no-store"},{"Cache-Control":"private, max-age=30"},{"Cache-Control":"public, max-age=30, must-revalidate, no-store"},{"Cache-Control":"public, max-age=0, must-revalidate"},{"Cache-Control":"public, max-age=301, must-revalidate"},{"Cache-Control":"public, max-age=30, max-age=60"},{Vary:"*"},{Vary:"Accept"},{Vary:"Accept, Cookie, Ruvane-Application-Revision"},{Age:"30"},{Age:"invalid"},{Age:"1, 2"}]) assert.equal(cacheFreshness(response(headers),30000,began),null);
  assert.equal(cacheFreshness(response({},404),30000,began),null);
});
test("page validation exports check schemas without consuming instance or node identities",async()=>{
  const file=process.env.RUVANE_WASM_FILE || new URL("../../target/wasm32-unknown-unknown/release/ruvane_browser.wasm",import.meta.url);
  const {instance}=await WebAssembly.instantiate(await readFile(file),{}),e=validateExports(instance.exports);
  const counter=e.ruvane_mount(1,1,1,5);e.ruvane_commands_clear();
  const validate=bytes=>{assert.equal(e.ruvane_page_begin(bytes.length),1);for(let offset=0;offset<bytes.length;offset+=4){let word=0;const count=Math.min(4,bytes.length-offset);for(let n=0;n<count;n++)word|=bytes[offset+n]<<(n*8);assert.equal(e.ruvane_page_chunk(word>>>0,count),1);}return e.ruvane_page_validate();};
  for(let n=0;n<1000;n++){assert.equal(validate(pagePacket()),1);assert.equal(e.ruvane_live_instances(),1);assert.equal(e.ruvane_live_signals(),2);assert.deepEqual(decodeCommands(new Uint8Array(e.memory.buffer,e.ruvane_commands_ptr(),e.ruvane_commands_len())),[]);}
  assert.equal(validate(pagePacket("/other")),0);assert.equal(e.ruvane_last_error(),11);assert.equal(e.ruvane_live_instances(),1);
  const next=e.ruvane_mount(2,1,1,0);assert.equal(next,counter+1);e.ruvane_commands_clear();e.ruvane_dispose(next);e.ruvane_commands_clear();e.ruvane_dispose(counter);e.ruvane_commands_clear();assert.equal(e.ruvane_live_instances(),0);
});
