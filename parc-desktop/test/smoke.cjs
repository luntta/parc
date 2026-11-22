// Run with Electron, not node. Isolates all writes and uses a synthetic mic and recognizer.
const { app, BrowserWindow } = require('electron');
const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const dir = process.argv[2];
if (!dir || !path.basename(dir).startsWith('parc-ui-test-')) throw new Error('Launch via npm run test:smoke');
app.setPath('userData', dir);
app.commandLine.appendSwitch('use-fake-device-for-media-stream');
process.env.PARC_WHISPER_MODEL = path.join(dir, 'model');
process.env.PARC_WHISPER_BIN = path.join(dir, 'stub');
fs.writeFileSync(process.env.PARC_WHISPER_MODEL, 'test');
fs.writeFileSync(process.env.PARC_WHISPER_BIN, '#!/bin/sh\nwhile [ "$1" != "-of" ]; do shift; done\nprintf "Synthetic transcript" > "$2.txt"\n', { mode: 0o700 });
const second = path.join(dir, 'Research', '.parc');
fs.writeFileSync(path.join(dir, 'vaults.json'), JSON.stringify([second]));
const { archive } = require('../backend.cjs');
// Native dialogs would block the run: each one must be expected, with the button index to press.
const answers = [];
require('electron').dialog.showMessageBox = async (_win, options) => {
  if (!answers.length) { console.error(`Unexpected dialog: ${options.message}`); return { response: 0 }; }
  return { response: answers.shift() };
};
require('../main.cjs');
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
async function check() {
  await app.whenReady();
  const win = BrowserWindow.getAllWindows()[0];
  const js = code => win.webContents.executeJavaScript(code, true);
  async function until(code) {
    for (let i = 0; i < 120; i++) { if (await js(code)) return; await delay(100); }
    throw new Error(`Timeout: ${code}. UI: ${await js('document.body.innerText')}`);
  }
  const idle = 'document.body.dataset.phase === "idle"';
  async function eventually(check) {
    for (let i = 0; i < 120; i++) { if (await check()) return; await delay(100); }
    throw new Error(`Timeout: ${check}`);
  }
  const prototype = archive(path.join(dir, 'prototype', '.parc'));
  const saveKey = 'document.dispatchEvent(new KeyboardEvent("keydown", { key: "s", ctrlKey: true, bubbles: true }))';
  await until(`document.querySelector("#empty")?.textContent.startsWith("No fragments yet") && ${idle}`);
  assert.equal(await js('document.querySelector("#save:defined > button").disabled'), true);
  assert.equal(await js('getComputedStyle(document.querySelector("#draft")).getPropertyValue("--aihio-color-page-bg").trim() !== ""'), true);
  await js('document.querySelector("#record").click()');
  await until('document.querySelector("#recordLabel").textContent.includes("Stop")');
  await delay(1100);
  await js('document.querySelector("#record").click()');
  await until('document.querySelector(".cm-content").textContent === "Synthetic transcript"');
  assert.equal(await js('document.querySelectorAll("#recent li").length'), 0);
  await js('document.querySelector("#title").value = "Ferry desk"; document.querySelector("#save").click()');
  await until(`document.querySelector("#recent button")?.textContent.includes("Ferry desk") && ${idle}`);
  await js('document.querySelector("[data-view=capture]").click()');
  await until('!document.querySelector("#title").value');
  await js('document.querySelector("#recent button").click()');
  await until('document.querySelector("#title").value === "Ferry desk"');
  await js('document.querySelector(".cm-content").focus()');
  await win.webContents.insertText('## Questions\n');
  await until('!!document.querySelector(".cm-line.md-h2")');
  await js('document.querySelector("#save").click()');
  await until(`document.querySelector("#revision").textContent === "Saved" && ${idle}`);
  assert.equal(await js('document.querySelectorAll("#recent li").length'), 1);
  await js('document.querySelector("[data-view=search]").click()');
  await until(idle);
  await js('document.querySelector("#query").value = "Questions"; document.querySelector("#searchForm").requestSubmit()');
  await until('document.querySelector("#results button")?.textContent.includes("Ferry desk")');

  // A typed capture: pick todo, save with Ctrl+S, then flip its status with the schema's toggle.
  await js('document.querySelector("[data-view=capture]").click()');
  await until(`!document.querySelector("#title").value && ${idle}`);
  await js('document.querySelector(".type-menu aihio-dropdown-item[value=todo]").click()');
  await until('document.querySelector(".crumb .here").textContent === "New todo"');
  await js('const t = document.querySelector("#title"); t.value = "Call the ferry office"; t.dispatchEvent(new Event("input"))');
  await js(saveKey);
  await until(`document.querySelector("#revision").textContent === "Saved" && ${idle}`);
  const todo = (await prototype.list()).find(r => r.title === 'Call the ferry office');
  assert.equal(todo.type, 'todo'); assert.equal(todo.status, 'open');
  await js('document.querySelector(".toggle-mark").click()');
  await eventually(async () => (await prototype.get(todo.id)).status === 'done');
  // A save from elsewhere reloads the clean open fragment.
  await prototype.update(todo.id, { fields: { priority: 'low' } });
  await until('document.querySelector(".prop[data-field=priority]")?.textContent.includes("Low priority")');
  // Unsaved text outlives a reload of the window, and leaving it asks first.
  await js('document.querySelector(".cm-content").focus()');
  await win.webContents.insertText('Unsent thought');
  await delay(500);
  win.webContents.reload();
  await until(`document.querySelector(".cm-content")?.textContent.includes("Unsent thought") && document.querySelector("#status").textContent.startsWith("Restored") && ${idle}`);
  assert.equal(await js('document.querySelector("#title").value'), 'Call the ferry office');
  const other = archive(second); await other.init();
  const old = await other.save({ title: 'Old observation', body: 'People asked privately.' });
  answers.push(1); // Don't save
  await js('document.querySelector(\'#vaultMenu aihio-dropdown-item[value="@all"]\').click()');
  await until(`document.querySelectorAll("#recent li").length === 3 && ${idle}`);
  assert.equal(await js('localStorage.getItem("parc.draft")'), null);
  await js('document.querySelector(".cm-content").focus()');
  // The [[ picker ranks fragments in memory and inserts the shortest unique ID on Enter.
  await win.webContents.insertText('See [[Old obs');
  await until('document.querySelector(".cm-linkoption")?.textContent.includes("Old observation")');
  await delay(120); // past the picker's interaction delay
  for (const type of ['keyDown', 'keyUp']) win.webContents.sendInputEvent({ type, keyCode: 'Return' });
  await until('!document.querySelector(".cm-tooltip-autocomplete") && document.querySelector(".cm-content").textContent.includes("Old observation")');
  await win.webContents.insertText(` and connection to [[${old.id}]]`);
  assert.equal(await js('document.querySelector("#save > button").disabled'), true);
  await js(`document.querySelector('#destination').value=${JSON.stringify(second)}; document.querySelector('#destination').dispatchEvent(new Event('change')); document.querySelector('#save').click()`);
  await until(`document.querySelectorAll("#recent li").length === 4 && ${idle}`);
  assert.equal((await other.list()).length, 2);
  // The saved fragment's own Graph section, then the whole scope as a graph.
  await until('document.querySelector("#docGraph:not([hidden]) .node:not(.center)")?.textContent.includes("Old observation")');
  await js('document.querySelector("[data-view=graph]").click()');
  await until('[...document.querySelectorAll("#results .vg-node")].some(n => n.getAttribute("aria-label").startsWith("Old observation"))');
  assert.equal(await js('document.querySelectorAll("#results .vg-edges line").length'), 1);
  // Go to… (Ctrl+K) finds a fragment by title and opens it with the caret in its text.
  await js('document.dispatchEvent(new KeyboardEvent("keydown", { key: "k", ctrlKey: true, bubbles: true }))');
  await until('!document.querySelector(".palette").hidden');
  await js('{ const i = document.querySelector(".palette-input"); i.value = "ferry desk"; i.dispatchEvent(new Event("input")); }');
  await until('document.querySelector(".palette-item[aria-selected=true]")?.textContent.includes("Ferry desk")');
  await js('document.querySelector(".palette-input").dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }))');
  await until(`document.querySelector("#title").value === "Ferry desk" && ${idle}`);
  await until('document.activeElement?.classList.contains("cm-content")');
  // The sidebar filters by words, by the search language, and to open work.
  const filter = async text => { await js(`{ const f = document.querySelector("#filter"); f.value = ${JSON.stringify(text)}; f.dispatchEvent(new Event("input", { bubbles: true })); }`); await delay(400); };
  const shown = () => js('[...document.querySelectorAll("#recent .row-title")].map(t => t.textContent)');
  await filter('ferry');
  assert.deepEqual((await shown()).sort(), ['Call the ferry office', 'Ferry desk']);
  await js('document.querySelector("#openOnly").click()');
  assert.deepEqual(await shown(), ['Ferry desk']);
  await js('document.querySelector("#openOnly").click()');
  await filter('type:todo');
  assert.deepEqual(await shown(), ['Call the ferry office']);
  await filter('');
  // History lists the version before "## Questions" was added; restoring it brings that text back.
  await js('document.querySelector("#fragmentMenu aihio-dropdown-item[value=history]").click()');
  await until(`document.querySelectorAll(".version").length >= 1 && ${idle}`);
  await js('[...document.querySelectorAll(".version")].at(-1).click()');
  await until('document.querySelector(".diff")?.textContent.includes("Questions")');
  await js('document.querySelector(".compare-head aihio-button").click()');
  await until(`document.querySelector("#title").value === "Ferry desk" && !document.querySelector(".cm-content").textContent.includes("Questions") && ${idle}`);
  // Archive, then trash with Undo.
  const ferry = (await prototype.list()).find(r => r.title === 'Ferry desk');
  await js('document.querySelector("#fragmentMenu aihio-dropdown-item[value=archive]").click()');
  await eventually(async () => (await prototype.get(ferry.id)).archived === true);
  await until(`!!document.querySelector(".status-action") && ${idle}`);
  await js('document.querySelector(".status-action").click()'); // Undo
  await eventually(async () => !(await prototype.get(ferry.id)).archived);
  await until(idle);
  answers.push(1); // Move to trash
  await js('document.querySelector("#fragmentMenu aihio-dropdown-item[value=trash]").click()');
  await until(`document.querySelector("#status").textContent.startsWith("Moved") && ${idle}`);
  await assert.rejects(prototype.get(ferry.id));
  await js('document.querySelector(".status-action").click()'); // Undo
  await until(`document.querySelector("#title").value === "Ferry desk" && ${idle}`);
  assert.equal((await prototype.get(ferry.id)).title, 'Ferry desk');
  // Live search as you type.
  await js('document.querySelector("[data-view=search]").click()'); await until(idle);
  await js('{ const q = document.querySelector("#query"); q.value = "ferry office"; q.dispatchEvent(new Event("input", { bubbles: true })); }');
  await until('document.querySelector("#results .row-title")?.textContent === "Call the ferry office"');
  // The quick capture window saves a typed fragment and closes.
  await js('document.querySelector("#quickCapture").click()');
  let quick; await eventually(async () => !!(quick = BrowserWindow.getAllWindows().find(w => w !== win)));
  await eventually(() => quick.webContents.executeJavaScript('document.querySelector("#qType")?.options.length > 1'));
  await quick.webContents.executeJavaScript('document.querySelector("#qVault").value = document.querySelector("#qVault").options[0].value; document.querySelector("#qType").value = "idea"; document.querySelector("#qBody").value = "Ferry timetable as a widget"; document.querySelector("#qBody").dispatchEvent(new Event("input"))');
  await quick.webContents.executeJavaScript('document.dispatchEvent(new KeyboardEvent("keydown", { key: "s", ctrlKey: true, bubbles: true }))');
  await eventually(async () => quick.isDestroyed());
  const idea = (await prototype.list()).find(r => r.title === 'Ferry timetable as a widget');
  assert.equal(idea?.type, 'idea');
  console.log('PASS: dictation, live Markdown, [[ link picker, search, typed capture and toggle, external reload, draft restore, aggregate vaults, scoped save, fragment and vault graphs, Go to…, sidebar filters, history restore, archive and trash with undo, live search, quick capture');
}
check().then(() => finish(0), err => { console.error(err); finish(1); });
function finish(code) {
  for (const win of BrowserWindow.getAllWindows()) win.destroy();
  app.exit(code);
}
