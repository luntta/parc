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
// The stub recognizer records its arguments, then writes a transcript where whisper-cli would.
const whisperArgs = path.join(dir, 'whisper-args');
fs.writeFileSync(process.env.PARC_WHISPER_BIN, `#!/bin/sh\nprintf '%s\\n' "$@" > '${whisperArgs}'\nwhile [ "$1" != "-of" ]; do shift; done\nprintf "Synthetic transcript" > "$2.txt"\n`, { mode: 0o700 });
const lastArgs = () => fs.readFileSync(whisperArgs, 'utf8').trim().split('\n');
const argAfter = flag => { const args = lastArgs(); return args[args.indexOf(flag) + 1]; };
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
  // The renderer converted the recording to WAV itself (no ffmpeg); with no languages set, whisper detects one.
  assert.match(argAfter('-f'), /clip\.wav$/);
  assert.equal(argAfter('-l'), 'auto');
  assert.equal(await js('document.querySelector("#languageSlot").childElementCount'), 0);
  assert.equal(await js('document.querySelectorAll("#recent .row").length'), 0);
  await js('document.querySelector("#title").value = "Ferry desk"; document.querySelector("#save").click()');
  await until(`document.querySelector("#recent .row")?.textContent.includes("Ferry desk") && ${idle}`);
  await js('document.querySelector("[data-view=capture]").click()');
  await until('!document.querySelector("#title").value');
  await js('document.querySelector("#recent .row").click()');
  await until('document.querySelector("#title").value === "Ferry desk"');
  await js('document.querySelector(".cm-content").focus()');
  await win.webContents.insertText('## Questions\n');
  await until('!!document.querySelector(".cm-line.md-h2")');
  await js('document.querySelector("#save").click()');
  await until(`document.querySelector("#revision").textContent === "Saved" && ${idle}`);
  assert.equal(await js('document.querySelectorAll("#recent .row").length'), 1);
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
  await until(`document.querySelectorAll("#recent .row").length === 3 && ${idle}`);
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
  await until(`document.querySelectorAll("#recent .row").length === 4 && ${idle}`);
  assert.equal((await other.list()).length, 2);
  // The saved fragment's own Graph section, then the whole scope as a graph.
  await until('document.querySelector("#docGraph:not([hidden]) .node:not(.center)")?.textContent.includes("Old observation")');
  await js('document.querySelector("[data-view=graph]").click()');
  await until('[...document.querySelectorAll("#results .vg-node")].some(n => n.getAttribute("aria-label").startsWith("Old observation"))');
  assert.equal(await js('document.querySelectorAll("#results .vg-edges line").length'), 1);
  // Go to… (Ctrl+K) finds a fragment by title, shows it on the right as it reads, and opens it with the caret in its text.
  const goTo = async text => {
    await js('document.dispatchEvent(new KeyboardEvent("keydown", { key: "k", ctrlKey: true, bubbles: true }))');
    await until('!document.querySelector(".palette").hidden');
    await js(`{ const i = document.querySelector(".palette-input"); i.value = ${JSON.stringify(text)}; i.dispatchEvent(new Event("input")); }`);
  };
  const paletteKey = key => js(`document.querySelector(".palette-input").dispatchEvent(new KeyboardEvent("keydown", { key: "${key}", bubbles: true }))`);
  await goTo('ferry desk');
  await until('document.querySelector(".palette-item[aria-selected=true]")?.textContent.includes("Ferry desk")');
  await until('document.querySelector(".pv-title")?.textContent === "Ferry desk" && document.querySelector(".palette-preview .cm-content")?.textContent.includes("Questions")');
  assert.equal(await js('document.querySelector(".palette-preview .cm-content").contentEditable'), 'false');
  await paletteKey('Enter');
  await until(`document.querySelector("#title").value === "Ferry desk" && ${idle}`);
  await until('document.activeElement?.classList.contains("cm-content")');
  // "/" greps inside fragments: each matching line under its fragment with line:column, the match marked in the
  // preview, and Enter selects it in the editor.
  await goTo('/questions');
  await until('document.querySelector(".palette-file")?.textContent.includes("Ferry desk") && !!document.querySelector(".palette-hit[aria-selected=true]")');
  assert.equal(await js('document.querySelector(".palette-mode").textContent'), 'Grep');
  assert.match(await js('document.querySelector(".palette-hit .palette-pos").textContent'), /^\d+:\d+$/);
  assert.equal(await js('document.querySelector(".palette-hit mark").textContent'), 'Questions');
  await until('document.querySelector(".palette-preview .md-hit-current")?.textContent === "Questions"');
  await paletteKey('Enter');
  await until(`document.querySelector(".palette").hidden && getSelection().toString() === "Questions" && ${idle}`);
  await goTo('/Nothing like this');
  await until('document.querySelector(".palette-none")?.textContent.startsWith("No fragment contains")');
  await paletteKey('Escape');
  await until('document.querySelector(".palette").hidden');
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
  // The list groups by type unless told otherwise, or by tag with a fragment under each of its tags; groups fold;
  // it sorts by title and creation.
  const listView = value => js(`document.querySelector('#listView aihio-dropdown-item[value="${value}"]').click()`);
  const groupNames = 'JSON.stringify([...document.querySelectorAll("#recent .group-name")].map(t => t.textContent))';
  await prototype.update(todo.id, { tags: ['ferry', 'calls'] });
  assert.equal(await js(groupNames), '["Note","Todo"]');
  assert.equal(await js('document.querySelector("#listHeading").textContent'), 'Fragments by type');
  assert.equal(await js('document.querySelector("#listView").hasAttribute("data-active")'), false);
  await listView('group:tag');
  await until(`${groupNames} === '["#calls","#ferry","No tags"]'`);
  assert.equal((await shown()).filter(t => t === 'Call the ferry office').length, 2);
  await js('document.querySelector(\'#recent [data-group="tag:calls"] .group-head\').click()');
  assert.equal(await js('document.querySelector(\'#recent [data-group="tag:calls"] .row\')'), null);
  assert.equal(await js('document.querySelector(\'#recent [data-group="tag:calls"] .group-count\').textContent'), '1');
  assert.deepEqual(JSON.parse(await js('localStorage.getItem("parc.folded")')), ['tag:calls']);
  await listView('group:none'); await listView('sort:title');
  assert.equal(await js('document.querySelectorAll("#recent .group").length'), 0);
  assert.equal(await js('document.querySelector("#listView").hasAttribute("data-active")'), true);
  const byTitle = await shown();
  assert.equal(byTitle[0], 'Call the ferry office');
  await listView('dir:desc');
  assert.deepEqual(await shown(), [...byTitle].reverse());
  await listView('sort:created'); await listView('dir:asc');
  assert.equal((await shown())[0], 'Ferry desk');
  assert.match(await js('document.querySelector("#recent time").title'), /^Created /);
  await listView('sort:updated'); await listView('group:type');
  assert.equal(await js('document.querySelector("#listView").hasAttribute("data-active")'), false);
  assert.deepEqual(JSON.parse(await js('localStorage.getItem("parc.listView")')), { group: 'type', sort: 'updated', dir: 'desc' });
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
  // Settings: the model list recommends by language; two languages put a picker next to Dictate.
  await js('document.querySelector("[data-view=capture]").click()'); await until(idle);
  await js('document.querySelector("#openSettings").click()');
  await until('document.querySelectorAll("#modelList li").length === 5');
  const recommended = 'document.querySelector("#modelList aihio-badge")?.closest("li").dataset.id';
  assert.equal(await js(recommended), 'small.en');
  assert.equal(await js('[...document.querySelectorAll("#modelList li")].every(li => li.querySelector("aihio-button")?.textContent === "Download")'), true);
  for (const code of ['fi', 'en']) {
    await js(`{ const s = document.querySelector("#languageList select"); s.value = "${code}"; s.dispatchEvent(new Event("change")); }`);
    await until(`document.querySelectorAll("#languageList .tag").length === ${code === 'fi' ? 1 : 2}`);
  }
  assert.equal(await js('document.querySelector("#languageList .tag").textContent'), 'Finnish');
  await until(`${recommended} === "large-v3-turbo-q5_0"`);
  await js('document.querySelector("#settingsSave").click()');
  await until('!document.querySelector("#settings").hasAttribute("open") && document.querySelector("#languageSlot .language-code")?.textContent === "FI"');
  await js('document.querySelector("#record").click()');
  await until('document.querySelector("#recordLabel").textContent.includes("Stop")');
  await delay(600);
  await js('document.querySelector("#record").click()');
  await until(`document.querySelector(".cm-content").textContent.includes("Synthetic transcript") && ${idle}`);
  assert.equal(argAfter('-l'), 'fi');
  await js('document.querySelector("#languageSlot aihio-dropdown-item[value=en]").click()');
  await until('document.querySelector("#languageSlot .language-code")?.textContent === "EN"');
  assert.deepEqual(JSON.parse(fs.readFileSync(path.join(dir, 'settings.json'), 'utf8')).speechLanguages, ['fi', 'en']);
  // Vault settings: turn a type off, rename a status (its fragments follow), add a type.
  const invoice = await prototype.save({ title: 'Settle the invoice', body: '', type: 'todo', fields: { status: 'in-progress' } });
  await until(idle);
  await js('document.querySelector("#vaultMenu aihio-dropdown-item[value=\\"@settings\\"]").click()');
  await until('!!document.querySelector("#vaultSettings[open] .type-row")');
  await js(`{ const s = document.querySelector(".vs-vault select"), p = [...(s?.options || [])].find(o => o.text === "Prototype")?.value; if (p && s.value !== p) { s.value = p; s.dispatchEvent(new Event("change")); } }`);
  await until('document.querySelector("#vsDescription").textContent.includes("Prototype") && document.querySelectorAll(".type-row").length === 5');
  await js('document.querySelector("aihio-switch[aria-label=\\"Use risk for new fragments\\"] input").click()');
  await until('document.querySelector("#vsMessage")?.textContent.startsWith("Risk is off")');
  assert.deepEqual((await prototype.vaultSettings()).types.disabled, ['risk']);
  await js('document.querySelector("aihio-button[aria-label=\\"Edit todo\\"]").click()');
  await until('document.querySelectorAll(".field-card").length === 4');
  await js('{ const i = [...document.querySelectorAll(".value-row .vs-input")].find(x => x.value === "in-progress"); i.value = "doing"; i.dispatchEvent(new Event("input")); i.dispatchEvent(new Event("change")); }');
  await js('[...document.querySelectorAll("#vsFooter aihio-button")].find(b => b.textContent.includes("Save type")).click()');
  await until('document.querySelector("#vsMessage")?.textContent.includes("moved") && document.querySelectorAll(".type-row").length === 5');
  assert.equal((await prototype.get(invoice.id)).status, 'doing');
  await js('[...document.querySelectorAll("#vsBody aihio-button")].find(b => b.textContent.includes("New type")).click()');
  await until('!!document.querySelector("[data-key=type-name]")');
  await js('{ const i = document.querySelector("[data-key=type-name]"); i.value = "bug"; i.dispatchEvent(new Event("input")); }');
  await js('[...document.querySelectorAll("#vsFooter aihio-button")].find(b => b.textContent.includes("Create type")).click()');
  await until('document.querySelectorAll(".type-row").length === 6');
  const kinds = await prototype.schemas();
  assert.equal(kinds.find(k => k.name === 'risk').enabled, false);
  assert.deepEqual(kinds.find(k => k.name === 'bug').fields[0].categories, { open: 'open', done: 'done' });
  await js('[...document.querySelectorAll("#vsFooter aihio-button")].find(b => b.textContent.includes("Done")).click()');
  await until('!document.querySelector("#vaultSettings").hasAttribute("open")');
  console.log('PASS: dictation, live Markdown, [[ link picker, search, typed capture and toggle, external reload, draft restore, aggregate vaults, scoped save, fragment and vault graphs, Go to… with preview and grep, sidebar filters, grouping and sorting, history restore, archive and trash with undo, live search, quick capture, dictation languages and model list, vault settings');
}
check().then(() => finish(0), err => { console.error(err); finish(1); });
function finish(code) {
  for (const win of BrowserWindow.getAllWindows()) win.destroy();
  app.exit(code);
}
