// Run with Electron, not node. Isolates all writes and uses a synthetic mic and recognizer.
const { app, BrowserWindow } = require('electron');
const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const dir = process.argv[2];
if (!dir || !path.basename(dir).startsWith('parc-ui-test-')) throw new Error('Launch via npm run test:smoke');
app.setPath('userData', dir);
app.commandLine.appendSwitch('use-fake-device-for-media-stream');
app.commandLine.appendSwitch('password-store', 'basic'); // the ChatGPT sign-in stays out of the desktop's keyring
process.env.PARC_WHISPER_MODEL = path.join(dir, 'model');
process.env.PARC_WHISPER_BIN = path.join(dir, 'stub');
// Isolate Omarchy state so tests exercise live Omarchy theming deterministically without touching ~/.local/state/omarchy.
const omarchyState = path.join(dir, 'omarchy-state');
fs.mkdirSync(path.join(omarchyState, 'current', 'theme'), { recursive: true });
fs.mkdirSync(path.join(omarchyState, 'toggles', 'hypr'), { recursive: true });
fs.writeFileSync(path.join(omarchyState, 'current', 'theme.name'), 'Tokyo Night\n');
fs.writeFileSync(path.join(omarchyState, 'current', 'theme', 'colors.toml'),
  'mode = "dark"\naccent = "#7aa2f7"\nselection = "#292e42"\nmuted = "#414868"\nbackground = "#1a1b26"\ndark_background = "#13141c"\nlighter_background = "#24283b"\nforeground = "#a9b1d6"\ndark_foreground = "#565f89"\nred = "#f7768e"\nyellow = "#e0af68"\ngreen = "#9ece6a"\n');
process.env.PARC_OMARCHY_STATE = omarchyState;
delete process.env.HYPRLAND_INSTANCE_SIGNATURE;
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
// Sign in with ChatGPT goes to a fake OpenAI; the "browser" follows its redirect back to parc.
const { fakeOpenAI, browser, message, completed } = require('./fake-openai.cjs');
// GitHub Copilot: a fake github.com for the device flow, and a fake Copilot SDK in place of the real one.
const { fakeGitHub, fakeSdk } = require('./fake-copilot.cjs');
const copilotSdk = fakeSdk(), sdkPath = require.resolve('@microsoft/copilot-sdk');
require.cache[sdkPath] = { id: sdkPath, filename: sdkPath, loaded: true, exports: copilotSdk.module };
const githubPages = [];
require('electron').shell.openExternal = url => url.includes('/login/device') ? (githubPages.push(url), Promise.resolve()) : browser(url);
// Copies land here, not on the desktop's clipboard.
let copied = '';
require('electron').clipboard.writeText = text => { copied = text; };
require('../main.cjs');
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
async function check() {
  await app.whenReady();
  const win = BrowserWindow.getAllWindows()[0];
  // A script that throws names itself, since the renderer's error doesn't say which one it was.
  const js = code => win.webContents.executeJavaScript(code, true).catch(err => { throw new Error(`${err.message}\n  in: ${code}`); });
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
  // Pulse is home: the first item in the nav, and the view the app opens on.
  assert.equal(await js('document.querySelector("nav button").dataset.view'), 'pulse');
  assert.equal(await js('document.querySelector("[data-current] .tool-title")?.textContent'), 'Pulse');
  assert.equal(await js('document.querySelector("[data-view=pulse]").getAttribute("aria-current")'), 'page');
  await js('document.querySelector("[data-view=capture]").click()');
  await until(`!!document.querySelector("[data-current] .doc") && ${idle}`);
  assert.equal(await js('document.querySelector("#save:defined > button").disabled'), true);
  assert.equal(await js('getComputedStyle(document.querySelector("[data-current] .draft")).getPropertyValue("--aihio-color-page-bg").trim() !== ""'), true);
  // The Related panel waits for words; a dictation is searched for as soon as it lands.
  await until('document.querySelector("[data-current] .related-empty")?.textContent === "Related notes appear here as you write."');
  await js('document.querySelector("#record").click()');
  await until('document.querySelector("#recordLabel").textContent.includes("Stop")');
  await delay(1100);
  await js('document.querySelector("#record").click()');
  await until('document.querySelector("[data-current] .cm-content")?.textContent === "Synthetic transcript"');
  await until('document.querySelector("[data-current] .related-empty")?.textContent === "Nothing in the vault is clearly related."');
  // The renderer converted the recording to WAV itself (no ffmpeg); with no languages set, whisper detects one.
  assert.match(argAfter('-f'), /clip\.wav$/);
  assert.equal(argAfter('-l'), 'auto');
  assert.equal(await js('document.querySelector("#languageSlot").childElementCount'), 0);
  assert.equal(await js('document.querySelectorAll("#recent .row").length'), 0);
  await js('document.querySelector("[data-current] .doc-title").value = "Ferry desk"; document.querySelector("#save").click()');
  await until(`document.querySelector("#recent .row")?.textContent.includes("Ferry desk") && ${idle}`);
  await js('document.querySelector("[data-view=capture]").click()');
  await until('document.querySelector("[data-current] .doc-title")?.value === ""');
  await js('document.querySelector("#recent .row").click()');
  await until('document.querySelector("[data-current] .doc-title")?.value === "Ferry desk"');
  await js('document.querySelector("[data-current] .cm-content").focus()');
  await win.webContents.insertText('## Questions\n');
  await until('!!document.querySelector(".cm-line.md-h2")');
  await js('document.querySelector("#save").click()');
  await until(`document.querySelector("#revision").textContent === "Saved" && ${idle}`);
  assert.equal(await js('document.querySelectorAll("#recent .row").length'), 1);
  await js('document.querySelector("[data-view=search]").click()');
  await until(idle);
  await js('document.querySelector("[data-current] .query").value = "Questions"; document.querySelector("[data-current] .search-form").requestSubmit()');
  await until('document.querySelector("[data-current] .view-out button")?.textContent.includes("Ferry desk")');

  // A typed capture: pick todo, save with Ctrl+S, then flip its status with the schema's toggle.
  await js('document.querySelector("[data-view=capture]").click()');
  await until(`document.querySelector("[data-current] .doc-title")?.value === "" && ${idle}`);
  await js('document.querySelector("[data-current] .type-menu aihio-dropdown-item[value=todo]").click()');
  await until('document.querySelector(".pane[data-active] .tab[aria-selected=true] .tab-label").textContent === "New todo"');
  await js('const t = document.querySelector("[data-current] .doc-title"); t.value = "Call the ferry office"; t.dispatchEvent(new Event("input"))');
  await js(saveKey);
  await until(`document.querySelector("#revision").textContent === "Saved" && ${idle}`);
  const todo = (await prototype.list()).find(r => r.title === 'Call the ferry office');
  assert.equal(todo.type, 'todo'); assert.equal(todo.status, 'open');
  // Metadata changes do not lock the editor.
  await js('document.querySelector("[data-current] .toggle-mark").click()');
  assert.equal(await js('document.querySelector("[data-current] .cm-content").contentEditable'), 'true');
  assert.equal(await js('document.querySelector("[data-current] .doc-title").readOnly'), false);
  await eventually(async () => (await prototype.get(todo.id)).status === 'done');
  // A save from elsewhere reloads the clean open fragment.
  await prototype.update(todo.id, { fields: { priority: 'low' } });
  await until('document.querySelector("[data-current] .prop[data-field=priority]")?.textContent.includes("Low priority")');
  // Unsaved text is stored on disk (not inline in localStorage) and outlives a reload of the window; closing it asks first.
  await js('document.querySelector("[data-current] .cm-content").focus()');
  await win.webContents.insertText('Unsent thought');
  await delay(500);
  assert.equal(await js('localStorage.getItem("parc.workspace").includes("Unsent thought")'), false, 'draft body lives on disk, not in localStorage');
  assert.ok(fs.readdirSync(path.join(dir, 'drafts')).some(f => f.endsWith('.json')), 'draft written to userData/drafts');
  win.webContents.reload();
  await until(`document.querySelector("[data-current] .cm-content")?.textContent.includes("Unsent thought") && document.querySelector("#status").textContent.startsWith("Restored") && ${idle}`);
  assert.equal(await js('document.querySelector("[data-current] .doc-title")?.value'), 'Call the ferry office');
  const tabNames = 'JSON.stringify([...document.querySelectorAll(".pane")].map(p => [...p.querySelectorAll(".tab")].map(t => t.getAttribute("aria-label"))))';
  assert.deepEqual(JSON.parse(await js(tabNames)), [['Ferry desk', 'Call the ferry office, unsaved']]);
  // Switching vaults keeps the tabs: a fragment stays in its own vault, and its draft stays unsaved.
  const other = archive(second); await other.init();
  const old = await other.save({ title: 'Old observation', body: 'People asked privately.' });
  await js('document.querySelector(\'#vaultMenu aihio-dropdown-item[value="@all"]\').click()');
  await until(`document.querySelectorAll("#recent .row").length === 3 && ${idle}`);
  assert.equal(await js('document.querySelector("#revision").textContent'), 'Unsaved');
  answers.push(1); // Don't save
  await js('document.dispatchEvent(new KeyboardEvent("keydown", { key: "w", ctrlKey: true, bubbles: true }))');
  await until(`${tabNames} === '[["Ferry desk"]]' && ${idle}`);
  assert.equal(answers.length, 0);
  await until('JSON.parse(localStorage.getItem("parc.workspace")).groups[0].tabs.every(t => !t.draft)');
  assert.equal(await js('localStorage.getItem("parc.draft")'), null);
  await js('document.querySelector("[data-view=capture]").click()');
  await until(`!!document.querySelector("[data-current] .doc") && ${idle}`);
  await js('document.querySelector("[data-current] .cm-content").focus()');
  // The [[ picker ranks fragments in memory and inserts the shortest unique ID on Enter.
  await win.webContents.insertText('See [[Old obs');
  await until('document.querySelector(".cm-linkoption")?.textContent.includes("Old observation")');
  await delay(120); // past the picker's interaction delay
  for (const type of ['keyDown', 'keyUp']) win.webContents.sendInputEvent({ type, keyCode: 'Return' });
  await until('!document.querySelector(".cm-tooltip-autocomplete") && document.querySelector("[data-current] .cm-content")?.textContent.includes("Old observation")');
  await win.webContents.insertText(` and connection to [[${old.id}]]`);
  assert.equal(await js('document.querySelector("#save > button").disabled'), true);
  await js(`document.querySelector('#destination').value=${JSON.stringify(second)}; document.querySelector('#destination').dispatchEvent(new Event('change')); document.querySelector('#save').click()`);
  await until(`document.querySelectorAll("#recent .row").length === 4 && ${idle}`);
  assert.equal((await other.list()).length, 2);
  // The saved fragment's own Graph section, then the whole scope as a graph.
  await until('document.querySelector("[data-current] .doc-graph:not([hidden]) .node:not(.center)")?.textContent.includes("Old observation")');
  await js('document.querySelector("[data-view=graph]").click()');
  await until('[...document.querySelectorAll("[data-current] .view-out .vg-node")].some(n => n.getAttribute("aria-label").startsWith("Old observation"))');
  assert.equal(await js('document.querySelectorAll("[data-current] .view-out .vg-edges line").length'), 1);
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
  await until(`document.querySelector("[data-current] .doc-title")?.value === "Ferry desk" && ${idle}`);
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
  await until(`${groupNames} === '["calls","ferry","No tags"]'`);
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
  // A type's header opens its own view: the todo board, a lane per status. Cards move by drag and drop and by
  // Shift+arrows, and Undo moves them back.
  const dragTo = (from, to) => js(`{
    const card = document.querySelector(${JSON.stringify(from)}), zone = document.querySelector(${JSON.stringify(to)}), dt = new DataTransfer();
    card.dispatchEvent(new DragEvent("dragstart", { bubbles: true, dataTransfer: dt }));
    zone.dispatchEvent(new DragEvent("dragover", { bubbles: true, cancelable: true, dataTransfer: dt }));
    zone.dispatchEvent(new DragEvent("drop", { bubbles: true, cancelable: true, dataTransfer: dt }));
    card.dispatchEvent(new DragEvent("dragend", { bubbles: true, dataTransfer: dt }));
  }`);
  const shiftKey = key => js(`document.activeElement.dispatchEvent(new KeyboardEvent("keydown", { key: "${key}", shiftKey: true, bubbles: true }))`);
  const lanes = 'JSON.stringify([...document.querySelectorAll(".lane")].map(l => [l.dataset.status, [...l.querySelectorAll(".card-title")].map(t => t.textContent)]))';
  await js('document.querySelector(\'#recent [data-group="type:todo"] .group-head\').click()');
  await until(`document.querySelector("[data-current] .tool-title")?.textContent === "Todos" && ${idle}`);
  assert.equal(await js('document.querySelector(\'#recent [data-group="type:todo"] .group-head\').getAttribute("aria-current")'), 'page');
  assert.deepEqual(JSON.parse(await js(lanes)), [['open', []], ['in-progress', []], ['done', ['Call the ferry office']], ['cancelled', []]]);
  await dragTo('.lane[data-status="done"] .card', '.lane[data-status="in-progress"]');
  await eventually(async () => (await prototype.get(todo.id)).status === 'in-progress');
  await until(`document.querySelector("#status .status-text").textContent === "Moved “Call the ferry office” to In progress." && ${idle}`);
  assert.equal(JSON.parse(await js(lanes))[1][1][0], 'Call the ferry office');
  await js('document.querySelector(\'.lane[data-status="in-progress"] .card\').focus()');
  await shiftKey('ArrowRight');
  await eventually(async () => (await prototype.get(todo.id)).status === 'done');
  await until(`document.activeElement?.closest(".lane")?.dataset.status === "done" && ${idle}`); // focus follows the card
  await js('document.querySelector(".status-action").click()'); // Undo
  await eventually(async () => (await prototype.get(todo.id)).status === 'in-progress');
  await until(`JSON.parse(${lanes})[1][1][0] === "Call the ferry office" && ${idle}`);
  // Risks open on their matrix: likelihood × impact, a tray of unassessed ones, and a ranking. Dropping a risk
  // on a cell sets both fields; Shift+↓ lowers its likelihood. The board lens is remembered.
  const strike = await prototype.save({ title: 'Ferry strike', body: '', type: 'risk', fields: { likelihood: 'high', impact: 'high' } });
  const fog = await prototype.save({ title: 'Fog delays', body: '', type: 'risk', fields: { mitigation: 'Check the forecast' } });
  await until(`[...document.querySelectorAll("#recent .row-title")].some(t => t.textContent === "Fog delays") && ${idle}`);
  await js('document.querySelector(\'#recent [data-group="type:risk"] .group-head\').click()');
  await until(`document.querySelector(".type-view")?.dataset.lens === "matrix" && ${idle}`);
  assert.equal(await js('document.querySelector(\'.cell[data-li="2"][data-ii="2"] .card-title\').textContent'), 'Ferry strike');
  assert.equal(await js('document.querySelector(".cell[data-li=\\"2\\"][data-ii=\\"2\\"]").dataset.level'), 'severe');
  assert.equal(await js('document.querySelector(".tray .card-title").textContent'), 'Fog delays');
  assert.equal(await js('document.querySelector(".rank-row .sev").textContent'), 'Severe');
  await dragTo('.tray .card', '.cell[data-li="1"][data-ii="2"]');
  await eventually(async () => { const f = await prototype.get(fog.id); return f.likelihood === 'medium' && f.impact === 'high' && f.mitigation === 'Check the forecast'; });
  await until(`document.querySelector('.cell[data-li="1"][data-ii="2"]')?.textContent.includes("Fog delays") && ${idle}`);
  assert.deepEqual(await js('[...document.querySelectorAll(".rank-title")].map(t => t.textContent)'), ['Ferry strike', 'Fog delays']);
  assert.equal(await js('document.querySelectorAll(".rank-mitigation")[1].textContent'), 'Check the forecast');
  await js('document.querySelector(\'.cell[data-li="1"] .card\').focus()');
  await shiftKey('ArrowDown');
  await eventually(async () => (await prototype.get(fog.id)).likelihood === 'low');
  await until(`document.querySelector('.cell[data-li="0"][data-ii="2"]')?.contains(document.activeElement) && ${idle}`);
  await js('document.querySelector(\'.lens-option[data-lens="board"]\').click()');
  await until('document.querySelector(".type-view")?.dataset.lens === "board"');
  assert.deepEqual(JSON.parse(await js(lanes))[0], ['identified', ['Fog delays', 'Ferry strike']]);
  assert.deepEqual(JSON.parse(await js('localStorage.getItem("parc.typeViews")')), { risk: { lens: 'board' } });
  // Pulse's counts of open work open the type's view.
  await js('document.querySelector("[data-view=pulse]").click()');
  await until(`document.querySelector("[data-current] .tool-title")?.textContent === "Pulse" && ${idle}`);
  await js('[...document.querySelectorAll(".figure")].find(f => f.textContent.startsWith("Open risks")).click()');
  await until(`document.querySelector("[data-current] .tool-title")?.textContent === "Risks" && ${idle}`);
  assert.equal((await prototype.get(strike.id)).likelihood, 'high');
  // History lists the version before "## Questions" was added; restoring it brings that text back.
  await js('[...document.querySelectorAll("#recent .row")].find(r => r.textContent.includes("Ferry desk")).click()');
  await until(`document.querySelector("[data-current] .doc-title")?.value === "Ferry desk" && ${idle}`);
  await js('document.querySelector("#fragmentMenu aihio-dropdown-item[value=history]").click()');
  await until(`document.querySelectorAll(".version").length >= 1 && ${idle}`);
  await js('[...document.querySelectorAll(".version")].at(-1).click()');
  await until('document.querySelector(".diff")?.textContent.includes("Questions")');
  await js('document.querySelector(".compare-head aihio-button").click()');
  await until(`document.querySelector("[data-current] .doc-title")?.value === "Ferry desk" && document.querySelector("[data-current] .cm-content")?.textContent.includes("Questions") === false && ${idle}`);
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
  await until(`document.querySelector("[data-current] .doc-title")?.value === "Ferry desk" && ${idle}`);
  assert.equal((await prototype.get(ferry.id)).title, 'Ferry desk');
  // Live search as you type.
  await js('document.querySelector("[data-view=search]").click()'); await until(idle);
  await js('{ const q = document.querySelector("[data-current] .query"); q.value = "ferry office"; q.dispatchEvent(new Event("input", { bubbles: true })); }');
  await until('document.querySelector("[data-current] .view-out .row-title")?.textContent === "Call the ferry office"');
  // The quick capture window saves a typed fragment and closes.
  await js('document.querySelector("#quickCapture").click()');
  let quick; await eventually(async () => !!(quick = BrowserWindow.getAllWindows().find(w => w !== win)));
  await eventually(() => quick.webContents.executeJavaScript('document.querySelector("#qType")?.options.length > 1'));
  await quick.webContents.executeJavaScript('document.querySelector("#qVault").value = document.querySelector("#qVault").options[0].value; document.querySelector("#qType").value = "idea"; document.querySelector("#qBody").value = "Ferry timetable as a widget"; document.querySelector("#qBody").dispatchEvent(new Event("input"))');
  await quick.webContents.executeJavaScript('document.dispatchEvent(new KeyboardEvent("keydown", { key: "s", ctrlKey: true, bubbles: true }))');
  await eventually(async () => quick.isDestroyed());
  const idea = (await prototype.list()).find(r => r.title === 'Ferry timetable as a widget');
  assert.equal(idea?.type, 'idea');
  // Settings: a theme shows as soon as it is picked; Cancel puts the saved one back.
  await js('document.querySelector("[data-view=capture]").click()'); await until(idle);
  const dark = 'matchMedia("(prefers-color-scheme: dark)").matches';
  const systemDark = await js(dark);
  await js('document.querySelector("#openSettings").click()');
  await until('document.querySelectorAll("#modelList li").length === 5');
  assert.equal(await js('document.querySelector("#themeChoice input:checked")?.value'), 'system');
  // The command to bind in a desktop's own shortcut settings copies with one click.
  assert.match(await js('document.querySelector("#captureCommand").textContent'), / --capture$/);
  await js('document.querySelector("#copyCaptureCommand").click()');
  await eventually(async () => copied === await js('document.querySelector("#captureCommand").textContent'));
  await js(`document.querySelector("#themeChoice input[value=${systemDark ? 'light' : 'dark'}]").click()`);
  await until(`${dark} === ${!systemDark}`);
  await js('document.querySelector("#settings aihio-button[command=\\"--close\\"]").click()');
  await until(`!document.querySelector("#settings").hasAttribute("open") && ${dark} === ${systemDark}`);
  // The model list recommends by language; two languages put a picker next to Dictate. Dark is saved with them.
  await js('document.querySelector("#openSettings").click()');
  await until('document.querySelectorAll("#modelList li").length === 5');
  await js('document.querySelector("#themeChoice input[value=dark]").click()');
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
  await until(`document.querySelector("[data-current] .cm-content")?.textContent.includes("Synthetic transcript") && ${idle}`);
  assert.equal(argAfter('-l'), 'fi');
  await js('document.querySelector("#languageSlot aihio-dropdown-item[value=en]").click()');
  await until('document.querySelector("#languageSlot .language-code")?.textContent === "EN"');
  const saved = JSON.parse(fs.readFileSync(path.join(dir, 'settings.json'), 'utf8'));
  assert.deepEqual(saved.speechLanguages, ['fi', 'en']);
  assert.equal(saved.theme, 'dark');
  assert.equal(await js(dark), true);
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
  // Related: as you write, fragments that share your words, tags or links appear beside the text, saying why and
  // quoting where. Typing on keeps those still related in their places; newcomers join below. One opens in place,
  // and Link puts it in the draft at the caret.
  await js('document.querySelector("[data-view=capture]").click()');
  await until(`!!document.querySelector("[data-current] .doc") && document.querySelector("[data-current] .doc-title")?.value === "" && ${idle}`);
  await until('document.querySelector("[data-current] .related-empty")?.textContent === "Related notes appear here as you write."');
  await js('document.querySelector("[data-current] .cm-content").focus()');
  await win.webContents.insertText('Read the synthetic transcript from the ferry');
  const ferryItem = '[...document.querySelectorAll("[data-current] .rel-item")].find(li => li.querySelector(".rel-title").textContent === "Ferry desk")';
  await until(`!!${ferryItem}`);
  assert.equal(await js(`${ferryItem}.querySelector(".rel-why").textContent`), 'Shares ferry, synthetic, transcript');
  assert.deepEqual(await js(`[...${ferryItem}.querySelectorAll(".rel-quote mark")].map(m => m.textContent)`), ['Synthetic', 'transcript']);
  const relatedTitles = () => js('[...document.querySelectorAll("[data-current] .rel-item .rel-title")].map(t => t.textContent)');
  const before = await relatedTitles();
  assert.ok(!before.includes('Call the ferry office'), 'one shared word is not enough');
  await win.webContents.insertText(', then call the office');
  await until(`document.querySelectorAll("[data-current] .rel-item").length > ${before.length}`);
  const after = await relatedTitles();
  assert.deepEqual(after.slice(0, before.length), before);
  assert.equal(after.at(-1), 'Call the ferry office');
  assert.equal(await js('!!document.activeElement?.closest(".cm-editor")'), true); // typing keeps the caret; the panel never takes it
  await js(`${ferryItem}.querySelector(".rel-head").click()`);
  await until(`${ferryItem}.querySelector(".rel-body")?.textContent.includes("Synthetic transcript")`);
  await js(`[...${ferryItem}.querySelectorAll(".rel-actions aihio-button")].find(b => b.textContent === "Link").click()`);
  await until(`${ferryItem}.querySelector(".rel-why").textContent.startsWith("Linked · ")`);
  // The link shows what it resolves to: as a chip, or as a hint beside its source while the caret is on its line.
  await until('[...document.querySelectorAll("[data-current] .cm-content :is(.md-wiki, .md-wikihint)")].some(n => n.textContent.includes("Ferry desk"))');
  // Tabs and groups, as in a code editor. Start from one empty group: Close all asks about each unsaved tab.
  assert.equal(await js('document.querySelectorAll(".pane").length'), 1);
  for (let n = await js('document.querySelectorAll(".tab[data-dirty]").length'); n > 0; n--) answers.push(1); // Discard / Don't save
  await js('document.querySelector(".pane[data-active] .tab[aria-selected=true]").dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 300, clientY: 60 }))');
  await until('!!document.querySelector(".tab-menu")');
  await js('[...document.querySelectorAll(".tab-menu-item")].find(b => b.textContent === "Close all").click()');
  await until(`!document.querySelector(".tab") && ${idle}`);
  assert.equal(answers.length, 0);
  assert.equal(await js('document.querySelector(".pane-empty:not([hidden]) .watermark-title")?.textContent'), 'Nothing open in this group');
  // A click in the list opens a fragment as the group's preview (~), which the next click replaces; a double-click
  // keeps it. Ctrl+click opens beside, in a new group that becomes current.
  const tabs = 'JSON.stringify([...document.querySelectorAll(".pane")].map(p => [...p.querySelectorAll(".tab")].map(t => (t.hasAttribute("data-preview") ? "~" : "") + t.querySelector(".tab-label").textContent)))';
  const rowNamed = t => `[...document.querySelectorAll("#recent .row")].find(r => r.querySelector(".row-title").textContent === ${JSON.stringify(t)})`;
  const tabsAre = async expected => {
    for (let i = 0; i < 120; i++) { if (await js(`${tabs} === ${JSON.stringify(JSON.stringify(expected))} && ${idle}`)) return; await delay(100); }
    assert.deepEqual(JSON.parse(await js(tabs)), expected);
    throw new Error('Timeout: the tabs are right but the app is still busy');
  };
  await js(`${rowNamed('Ferry desk')}.click()`);
  await tabsAre([['~Ferry desk']]);
  await js(`${rowNamed('Call the ferry office')}.click()`);
  await tabsAre([['~Call the ferry office']]);
  await js(`${rowNamed('Call the ferry office')}.dispatchEvent(new MouseEvent("dblclick", { bubbles: true }))`);
  await tabsAre([['Call the ferry office']]);
  await js(`${rowNamed('Ferry desk')}.click()`);
  await tabsAre([['Call the ferry office', '~Ferry desk']]);
  await js(`${rowNamed('Old observation')}.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, ctrlKey: true, detail: 1 }))`);
  await tabsAre([['Call the ferry office', '~Ferry desk'], ['Old observation']]);
  assert.equal(await js('document.querySelector(".pane[data-active]") === document.querySelectorAll(".pane")[1]'), true);
  assert.equal(await js('document.querySelector("[data-current] .doc-title").value'), 'Old observation');
  // With more than one vault open, the meta row says which one a fragment is in.
  assert.equal(await js('document.querySelector("[data-current] .meta-vault").textContent'), 'Research');
  // The tabs are the panel's top edge: nothing above them, and the shown fragment's actions end the current group's strip.
  assert.equal(await js('document.querySelector("main").firstElementChild.id'), 'groups');
  assert.equal(await js('document.querySelector("#docActions").closest(".tabstrip") === document.querySelector(".pane[data-active] .tabstrip")'), true);
  // Ctrl+\ hides the sidebar; Show sidebar then starts the first group's strip, and brings it back.
  await js('document.dispatchEvent(new KeyboardEvent("keydown", { key: "\\\\", ctrlKey: true, bubbles: true }))');
  await until('document.body.hasAttribute("data-collapsed") && !document.querySelector("#sidebarShow").hidden');
  assert.equal(await js('document.querySelector("#sidebarShow").closest(".tabstrip") === document.querySelectorAll(".tabstrip")[0]'), true);
  await js('document.querySelector("#sidebarShow > button").click()');
  await until('!document.body.hasAttribute("data-collapsed") && document.querySelector("#sidebarShow").hidden');
  // Ctrl+Alt+← moves the tab into the group on the left; the group it leaves empty goes.
  await js('document.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowLeft", ctrlKey: true, altKey: true, bubbles: true }))');
  await tabsAre([['Call the ferry office', '~Ferry desk', 'Old observation']]);
  // Tabs drag: onto a group's right third to open a new group there, and along a strip to a place in it.
  const dragTab = (label, target, fx) => js(`{
    const tab = [...document.querySelectorAll(".tab")].find(t => t.querySelector(".tab-label").textContent === ${JSON.stringify(label)});
    const zone = ${target}, r = zone.getBoundingClientRect(), at = { clientX: r.left + r.width * ${fx}, clientY: r.top + r.height / 2 }, dt = new DataTransfer();
    tab.dispatchEvent(new DragEvent("dragstart", { bubbles: true, dataTransfer: dt }));
    for (const type of ["dragenter", "dragover", "drop"]) zone.dispatchEvent(new DragEvent(type, { bubbles: true, cancelable: true, dataTransfer: dt, ...at }));
    tab.dispatchEvent(new DragEvent("dragend", { bubbles: true, dataTransfer: dt }));
  }`);
  await dragTab('Call the ferry office', 'document.querySelectorAll(".pane")[0].querySelector(".drop-overlay")', 0.9);
  await tabsAre([['~Ferry desk', 'Old observation'], ['Call the ferry office']]);
  await dragTab('Old observation', 'document.querySelectorAll(".pane")[1].querySelector(".tabs")', 0.01);
  await tabsAre([['~Ferry desk'], ['Old observation', 'Call the ferry office']]);
  // A fragment dragged from the list opens where it is dropped, and stays.
  await js(`{
    const row = ${rowNamed('Settle the invoice')}, zone = document.querySelectorAll(".pane")[0].querySelector(".drop-overlay"), dt = new DataTransfer();
    const r = zone.getBoundingClientRect(), at = { clientX: r.left + r.width / 2, clientY: r.top + r.height / 2 };
    row.dispatchEvent(new DragEvent("dragstart", { bubbles: true, dataTransfer: dt }));
    for (const type of ["dragenter", "dragover", "drop"]) zone.dispatchEvent(new DragEvent(type, { bubbles: true, cancelable: true, dataTransfer: dt, ...at }));
    row.dispatchEvent(new DragEvent("dragend", { bubbles: true, dataTransfer: dt }));
  }`);
  await tabsAre([['~Ferry desk', 'Settle the invoice'], ['Old observation', 'Call the ferry office']]);
  // A view opens in the current group too, and a fragment already open elsewhere is shown where it is.
  await js('document.body.dispatchEvent(new KeyboardEvent("keydown", { key: "p", bubbles: true }))');
  await tabsAre([['~Pulse', 'Settle the invoice'], ['Old observation', 'Call the ferry office']]);
  await js(`${rowNamed('Call the ferry office')}.click()`);
  await until(`document.querySelector("[data-current] .doc-title")?.value === "Call the ferry office" && ${idle}`);
  assert.equal(await js('document.querySelector(".pane[data-active]") === document.querySelectorAll(".pane")[1]'), true);
  // Ctrl+W closes the tab, Ctrl+Shift+T brings it back; Ctrl+1 goes to the first group.
  await js('document.dispatchEvent(new KeyboardEvent("keydown", { key: "w", ctrlKey: true, bubbles: true }))');
  await tabsAre([['~Pulse', 'Settle the invoice'], ['Old observation']]);
  await js('document.dispatchEvent(new KeyboardEvent("keydown", { key: "T", ctrlKey: true, shiftKey: true, bubbles: true }))');
  await tabsAre([['~Pulse', 'Settle the invoice'], ['Old observation', 'Call the ferry office']]);
  await js('document.dispatchEvent(new KeyboardEvent("keydown", { key: "1", code: "Digit1", ctrlKey: true, bubbles: true }))');
  await until(`document.querySelector(".pane[data-active]") === document.querySelectorAll(".pane")[0] && document.querySelector("[data-current] .tool-title")?.textContent === "Pulse"`);
  // Unsaved text in a tab that isn't current shows on its tab; the window's layout and the draft come back after a reload.
  await js('document.querySelectorAll(".pane")[1].querySelector(".tab[aria-selected=true]").click()');
  await until(`document.querySelector(".pane[data-active]") === document.querySelectorAll(".pane")[1] && ${idle}`);
  await js('document.querySelector("[data-current] .cm-content").focus()');
  await win.webContents.insertText('Unsent in a split. ');
  await js('document.dispatchEvent(new KeyboardEvent("keydown", { key: "1", code: "Digit1", ctrlKey: true, bubbles: true }))');
  await until('document.querySelector(".pane[data-active]") === document.querySelectorAll(".pane")[0]');
  assert.equal(await js('[...document.querySelectorAll(".tab[data-dirty]")].map(t => t.querySelector(".tab-label").textContent).join()'), 'Call the ferry office');
  await delay(400);
  win.webContents.reload();
  await until(`document.querySelectorAll(".pane").length === 2 && document.querySelector("#status").textContent.startsWith("Restored") && ${idle}`);
  await tabsAre([['~Pulse', 'Settle the invoice'], ['Old observation', 'Call the ferry office']]);
  assert.equal(await js('document.querySelector(".pane[data-active]") === document.querySelectorAll(".pane")[0]'), true);
  assert.equal(await js('document.querySelectorAll(".pane")[1].querySelector(".scroll:not([hidden]) .cm-content").textContent.includes("Unsent in a split.")'), true);
  // The Agent: signed out, it says what signing in means; Continue with ChatGPT goes through the browser and back.
  // A question shows the search the agent made and an answer whose citation reads as a title and opens the fragment.
  const openai = await fakeOpenAI();
  await js('document.querySelector("nav [data-view=agent]").click()');
  await until('!!document.querySelector("[data-current] .agent-continue") && document.querySelector("[data-current] .agent-compose").hidden');
  assert.match(await js('document.querySelector("[data-current] .agent-privacy").textContent'), /go to OpenAI/);
  await js('document.querySelector("[data-current] .agent-continue").click()');
  await until('document.querySelector("[data-current] .agent-provider")?.selectedOptions[0]?.textContent === "ChatGPT · reader@example.com"');
  assert.equal(await js('[...document.querySelectorAll("[data-current] .agent-model option")].map(o => o.textContent).join()'), 'GPT Test,GPT Other');
  assert.equal(openai.fake.seen.authorize[0].agent_name_hint, 'parc');
  openai.fake.respond.push(request => {
    const vault = request.tools[0].parameters.properties.vault?.enum?.[0];
    return [completed([{ type: 'function_call', id: 'fc_1', call_id: 'c1', name: 'search', arguments: JSON.stringify({ query: 'ferry', ...(vault ? { vault } : {}) }) }])];
  }, request => {
    const found = request.input.find(i => i.type === 'function_call_output').output.match(/\b[0-9A-HJKMNP-TV-Z]{26}\b/)[0];
    return [{ type: 'response.output_text.delta', delta: `It’s in [[${found}]].` }, completed([message(`It’s in [[${found}]].`)])];
  });
  await js('document.querySelector("[data-current] .agent-input").focus()');
  await win.webContents.insertText('Where did I note the ferry?');
  await js('document.querySelector("[data-current] .agent-input").dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }))');
  await until('document.querySelector("[data-current] .agent-step-label")?.textContent.startsWith("Searched “ferry”") && !!document.querySelector("[data-current] .agent-text .md-wiki") && !document.querySelector("[data-current] .agent-turn[aria-busy]")');
  assert.equal(await js('document.querySelector("[data-current] .agent-question").textContent'), 'Where did I note the ferry?');
  assert.match(await js('document.querySelector("[data-current] .agent-step-meta").textContent'), /found$/);
  assert.equal(openai.fake.seen.responses[0].store, false);
  const cited = await js('document.querySelector("[data-current] .agent-text .md-wiki").textContent');
  assert.match(cited, /ferry/i);
  await js('document.querySelector("[data-current] .agent-text .md-wiki").dispatchEvent(new MouseEvent("mousedown", { bubbles: true, cancelable: true, button: 0 }))');
  await until(`document.querySelector("[data-current] .doc-title")?.value === ${JSON.stringify(cited)}`);
  // Chats are saved. The chat's tab is named for its first question and kept; New chat opens another, which lists
  // the saved ones. Picking one shows the tab it is open in, or, once that is closed, shows it in place, as it was;
  // a follow-up goes on from it, and it comes back after a reload. Deleting it closes its tab; Undo brings it back.
  const question = 'Where did I note the ferry?', tabNamed = t => `[...document.querySelectorAll(".tab")].find(x => x.querySelector(".tab-label").textContent === ${JSON.stringify(t)})`;
  await until(`${tabs}.includes(${JSON.stringify(JSON.stringify(question))})`);
  await js(`${tabNamed(question)}.click()`);
  await until(`document.querySelector("[data-current] .tool-title")?.textContent === ${JSON.stringify(question)} && ${idle}`);
  await js('document.querySelector("[data-current] .agent-compose-row aihio-button").click()');
  await until('document.querySelector("[data-current] .tool-title")?.textContent === "Agent" && [...document.querySelectorAll("[data-current] .agent-chat-title")].map(n => n.textContent).join() === "Where did I note the ferry?"');
  const count = await js('document.querySelectorAll(".tab").length');
  await js('document.querySelector("[data-current] .agent-chat-open").click()');
  await until(`document.querySelector("[data-current] .tool-title")?.textContent === ${JSON.stringify(question)}`);
  assert.equal(await js('document.querySelectorAll(".tab").length'), count, 'an open chat is shown in its tab, not opened again');
  await js('document.dispatchEvent(new KeyboardEvent("keydown", { key: "w", ctrlKey: true, bubbles: true }))');
  await until(`!${tabNamed(question)}`);
  await js(`${tabNamed('Agent')}.click()`);
  await until('!!document.querySelector("[data-current] .agent-chat-open")');
  await js('document.querySelector("[data-current] .agent-chat-open").click()');
  await until(`document.querySelector("[data-current] .agent-question")?.textContent === ${JSON.stringify(question)} && !!document.querySelector("[data-current] .agent-text .md-wiki")`);
  assert.equal(await js('document.querySelectorAll(".tab").length'), count - 1, 'shown in place of the empty chat');
  assert.match(await js('document.querySelector("[data-current] .agent-step-label").textContent'), /^Searched “ferry”/);
  assert.equal(await js('document.querySelector("[data-current] .tool-title").textContent'), question);
  openai.fake.respond.push(() => [completed([message('Nowhere else.')])]);
  await js('document.querySelector("[data-current] .agent-input").focus()');
  await win.webContents.insertText('Anywhere else?');
  await js('document.querySelector("[data-current] .agent-input").dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }))');
  await until('document.querySelectorAll("[data-current] .agent-question").length === 2 && !document.querySelector("[data-current] .agent-turn[aria-busy]")');
  assert.ok(openai.fake.seen.responses.at(-1).input.some(i => i.type === 'function_call_output'), 'the follow-up carries the saved chat');
  await delay(400);
  win.webContents.reload();
  await until(`!!${tabNamed(question)} && ${idle}`);
  await js(`${tabNamed(question)}.click()`);
  await until('[...document.querySelectorAll("[data-current] .agent-question")].map(q => q.textContent).join("|") === "Where did I note the ferry?|Anywhere else?"');
  await js('document.querySelector("[data-current] .agent-compose-row aihio-button").click()');
  await until('!!document.querySelector("[data-current] .agent-chat-delete")');
  await js('document.querySelector("[data-current] .agent-chat-delete").click()');
  await until(`!${tabNamed(question)} && !document.querySelector("[data-current] .agent-chat-open") && !!document.querySelector("#status .status-action")`);
  await js('document.querySelector("#status .status-action").click()');
  await until('[...document.querySelectorAll("[data-current] .agent-chat-title")].map(n => n.textContent).join() === "Where did I note the ferry?"');
  // GitHub Copilot: a new chat switched to it offers its sign-in; the code to type at GitHub is shown while parc
  // waits; signed in, the plan's models are offered and a question runs parc's tools on Copilot.
  const github = await fakeGitHub();
  github.fake.polls = [];
  await until(`!!document.querySelector("[data-current] .agent-provider") && ${idle}`);
  assert.equal(await js('[...document.querySelectorAll("[data-current] .agent-provider option")].map(o => o.textContent).join("|")'), 'ChatGPT · reader@example.com|GitHub Copilot (sign in)');
  await js('(() => { const s = document.querySelector("[data-current] .agent-provider"); s.value = "copilot"; s.dispatchEvent(new Event("change")); })()');
  await until('!!document.querySelector("[data-current] .agent-continue-copilot")');
  assert.match(await js('document.querySelector("[data-current] .agent-privacy").textContent'), /go to GitHub/);
  await js('document.querySelector("[data-current] .agent-continue-copilot").click()');
  await until('document.querySelector("[data-current] .agent-code-value")?.textContent === "ABCD-1234"');
  await until('document.querySelector("[data-current] .agent-provider")?.selectedOptions[0]?.textContent === "GitHub Copilot · octo"');
  assert.equal(githubPages.length, 1);
  assert.deepEqual(github.fake.seen.device[0], { client_id: 'Ov23liparctest00000', scope: 'read:user' });
  await until('[...document.querySelectorAll("[data-current] .agent-model option")].map(o => o.textContent).join() === "Claude Sonnet X,GPT Y"');
  copilotSdk.fake.script.push(async session => {
    const vault = session.options.tools.definitions[0].parameters.properties.vault?.enum?.[0];
    const found = await session.tool('parc_search', { query: 'ferry', ...(vault ? { vault } : {}) });
    const id = found.match(/\b[0-9A-HJKMNP-TV-Z]{26}\b/)[0];
    session.say(`On Copilot: [[${id}]].`);
    session.emit('session.idle');
  });
  await js('document.querySelector("[data-current] .agent-input").focus()');
  await win.webContents.insertText('Ferry, on Copilot?');
  await js('document.querySelector("[data-current] .agent-input").dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }))');
  await until('document.querySelector("[data-current] .agent-step-label")?.textContent.startsWith("Searched “ferry”") && !!document.querySelector("[data-current] .agent-text .md-wiki") && !document.querySelector("[data-current] .agent-turn[aria-busy]")');
  assert.equal(await js('document.querySelector("[data-current] .tool-title").textContent'), 'Ferry, on Copilot?');
  assert.equal(await js('document.querySelector("[data-current] .agent-who-name").textContent'), 'GitHub Copilot · octo', 'a saved chat names its provider');
  assert.deepEqual(copilotSdk.fake.seen.sessions[0].options.tools.availableTools, ['custom:*']);
  // Chats on both providers are listed together, each marked.
  await js('document.querySelector("[data-current] .agent-compose-row aihio-button").click()');
  await until('document.querySelectorAll("[data-current] .agent-chat").length === 2');
  assert.equal(await js('[...document.querySelectorAll("[data-current] .agent-chat-vault")].map(n => n.textContent).join()'), 'Copilot,ChatGPT');
  github.close();
  openai.close();

  // Step 2 parity: templates, list & string field editors, promote/convert type, drop-to-attach with
  // inline parc-attach:// raster preview, Trash view restore, Vault health + reindex, Pulse daily review,
  // and removing a secondary vault without deleting disk files.
  const protoPath = path.join(dir, 'prototype', '.parc');
  await js(`document.querySelector('#vaultMenu aihio-dropdown-item[value=${JSON.stringify(protoPath)}]').click()`);
  await until(`document.querySelector("#vaultName").textContent === "Prototype" && ${idle}`);
  await js('document.querySelector("[data-view=capture]").click()');
  await until(`!!document.querySelector("[data-current] .doc") && document.querySelector("[data-current] .doc-title")?.value === "" && ${idle}`);
  await js('document.querySelector("[data-current] .type-menu aihio-dropdown-item[value=decision]").click()');
  await until('document.querySelector("[data-current] .cm-line.md-h2")?.textContent === "Context"');
  assert.equal(await js('document.querySelector("#revision").textContent'), '', 'untouched template is not marked dirty');
  assert.equal(await js('document.querySelector("#save > button").disabled'), true, 'untouched template cannot be saved empty');
  await js('document.querySelector("[data-current] .prop-list[data-field=deciders] .add-list").click()');
  await js('{ const i = document.querySelector("[data-current] .prop-list[data-field=deciders] .tag-input"); i.value = "Ada"; i.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true })); }');
  await until('document.querySelector("[data-current] .prop-list[data-field=deciders] .list-chip")?.textContent.includes("Ada")');
  await js('const t = document.querySelector("[data-current] .doc-title"); t.value = "Choose SQLite"; t.dispatchEvent(new Event("input"))');
  await js(saveKey);
  await until(`document.querySelector("#revision").textContent === "Saved" && ${idle}`);
  const dec = (await prototype.list()).find(r => r.title === 'Choose SQLite');
  assert.equal(dec?.type, 'decision');
  assert.deepEqual((await prototype.get(dec.id)).deciders, ['Ada']);
  assert.match((await prototype.get(dec.id)).body, /## Context/);

  // Convert a saved note into a todo, then edit its inline string field (assignee).
  const prom = await prototype.save({ title: 'Promote me', body: 'Stays intact.' });
  await until(`!!${rowNamed('Promote me')} && ${idle}`);
  await js(`${rowNamed('Promote me')}.click()`);
  await until(`document.querySelector("[data-current] .doc-title")?.value === "Promote me" && ${idle}`);
  await js('document.querySelector("[data-current] .saved-type aihio-dropdown-item[value=todo]").click()');
  await eventually(async () => (await prototype.get(prom.id)).type === 'todo');
  await until(`!!document.querySelector("[data-current] .prop-text[data-field=assignee]") && ${idle}`);
  await js('document.querySelector("[data-current] .prop-text[data-field=assignee] .prop-button").click()');
  await js('{ const i = document.querySelector("[data-current] .prop-text[data-field=assignee] .prop-input"); i.value = "Raine"; i.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true })); }');
  await eventually(async () => (await prototype.get(prom.id)).assignee === 'Raine');
  await until(`document.querySelector("[data-current] .prop-text[data-field=assignee]")?.textContent.includes("Assignee: Raine") && ${idle}`);

  // Drop a PNG onto the fragment: attaches, inserts ![[attach:dot.png]], renders inline via parc-attach://, then detaches.
  const pngB64 = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+ip1sAAAAASUVORK5CYII=';
  await js(`{
    const bin = atob(${JSON.stringify(pngB64)}), bytes = Uint8Array.from(bin, c => c.charCodeAt(0));
    const dt = new DataTransfer();
    dt.items.add(new File([bytes], "dot.png", { type: "image/png" }));
    document.querySelector("[data-current] .doc").dispatchEvent(new DragEvent("drop", { bubbles: true, cancelable: true, dataTransfer: dt }));
  }`);
  await until(`document.querySelector("[data-current] .doc-attachments:not([hidden]) .attach-name")?.textContent === "dot.png" && ${idle}`);
  await js('document.activeElement?.blur(); document.querySelector("[data-current] .doc-title").focus()');
  await until('(() => { const img = document.querySelector("[data-current] .cm-content .md-attach-img"); return !!img && img.src.startsWith("parc-attach://") && img.complete && img.naturalWidth === 1; })()');
  answers.push(1); // Remove attachment
  await js('document.querySelector("[data-current] .attach-remove").click()');
  await until(`document.querySelector("[data-current] .doc-attachments")?.hidden === true && ${idle}`);
  assert.equal((await prototype.attachments(prom.id)).length, 0);
  await js(saveKey);
  await until(`document.querySelector("#revision").textContent === "Saved" && ${idle}`);

  // Trash view lists trashed fragments and restores them.
  answers.push(1); // Move to trash
  await js('document.querySelector("#fragmentMenu aihio-dropdown-item[value=trash]").click()');
  await until(`document.querySelector("#status").textContent.startsWith("Moved") && ${idle}`);
  await js('document.querySelector("#vaultMenu aihio-dropdown-item[value=\\"@trash\\"]").click()');
  await until(`document.querySelector("[data-current] .tool-title")?.textContent === "Trash" && document.querySelector("[data-current] .trash-row")?.textContent.includes("Promote me") && ${idle}`);
  await js('document.querySelector("[data-current] .trash-actions aihio-button").click()');
  await until(`document.querySelector("[data-current] .doc-title")?.value === "Promote me" && ${idle}`);
  assert.equal((await prototype.get(prom.id)).title, 'Promote me');

  // Vault health detects broken links and rebuilds the SQLite index.
  await prototype.save({ title: 'Broken ref note', body: 'Points to [[01AAAAAAAAAAAAAAAAAAAAAAAA]].' });
  await until(`!!${rowNamed('Broken ref note')} && ${idle}`);
  await js('document.querySelector("#vaultMenu aihio-dropdown-item[value=\\"@doctor\\"]").click()');
  await until(`document.querySelector("[data-current] .tool-title")?.textContent === "Vault health" && document.querySelector("[data-current] .doctor-list")?.textContent.includes("01AAAAAAAAAAAAAAAAAAAAAAAA") && ${idle}`);
  await js('[...document.querySelectorAll("[data-current] .doctor-actions aihio-button")].find(b => b.textContent.includes("Rebuild index")).click()');
  await until(`document.querySelector("#status").textContent.startsWith("Rebuilt index") && ${idle}`);

  // Pulse daily review highlights overdue items in Needs attention.
  await prototype.save({ title: 'Overdue filing', body: 'Do now', type: 'todo', fields: { due: '2020-01-01' } });
  await until(`!!${rowNamed('Overdue filing')} && ${idle}`);
  await js('document.querySelector("[data-view=pulse]").click()');
  await until(`document.querySelector("[data-current] .tool-title")?.textContent === "Pulse" && document.querySelector("[data-current] .pulse-list")?.textContent.includes("Overdue filing") && ${idle}`);
  assert.match(await js('document.querySelector("[data-current] .pulse-pill[data-tone=alert]").textContent'), /^Overdue/);

  // Removing a secondary vault detaches it from parc without touching files on disk.
  await js(`document.querySelector('#vaultMenu aihio-dropdown-item[value=${JSON.stringify(second)}]').click()`);
  await until(`document.querySelector("#vaultName").textContent === "Research" && ${idle}`);
  answers.push(1); // Remove
  await js('document.querySelector("#vaultMenu aihio-dropdown-item[value=\\"@close\\"]").click()');
  await until(`document.querySelector("#vaultName").textContent === "Prototype" && document.querySelector("#status").textContent.includes("files on disk were not changed") && ${idle}`);
  assert.equal(fs.existsSync(path.join(second, 'config.yml')), true, 'removing a vault leaves its files on disk');

  // Omarchy System theme: switch back to System, verify Tokyo Night tokens apply, then swap
  // themes and toggle Glassy + Rounded corners live on disk and verify the window follows immediately.
  await js('document.querySelector("#openSettings").click()');
  await until('document.querySelector("#settings").hasAttribute("open")');
  assert.match(await js('document.querySelector("#themeSub").textContent'), /Omarchy theme \(Tokyo Night, transparent\)/);
  await js('document.querySelector("#themeChoice input[value=system]").click()');
  await until('document.documentElement.dataset.omarchy === "Tokyo Night" && document.documentElement.dataset.omarchyOpacity === "transparent" && document.documentElement.style.getPropertyValue("--aihio-color-primary-action-bg") === "#7aa2f7"');
  await js('document.querySelector("#settingsSave").click()');
  await until('!document.querySelector("#settings").hasAttribute("open")');
  const nextOmarchy = path.join(omarchyState, 'current', 'next-theme');
  fs.mkdirSync(nextOmarchy, { recursive: true });
  fs.writeFileSync(path.join(nextOmarchy, 'colors.toml'),
    'mode = "light"\naccent = "#1e66f5"\nselection = "#ccd0da"\nmuted = "#acb0be"\nbackground = "#eff1f5"\ndark_background = "#e3e4e8"\nlighter_background = "#dce0e8"\nforeground = "#4c4f69"\ndark_foreground = "#9ca0b0"\nred = "#d20f39"\nyellow = "#df8e1d"\ngreen = "#40a02b"\n');
  fs.rmSync(path.join(omarchyState, 'current', 'theme'), { recursive: true, force: true });
  fs.renameSync(nextOmarchy, path.join(omarchyState, 'current', 'theme'));
  fs.writeFileSync(path.join(omarchyState, 'current', 'theme.name'), 'Catppuccin Latte\n');
  fs.writeFileSync(path.join(omarchyState, 'toggles', 'hypr', 'window-glassy.lua'), '-- glassy\n');
  fs.writeFileSync(path.join(omarchyState, 'toggles', 'hypr', 'corners-rounded.lua'), '-- rounded\n');
  await until('document.documentElement.dataset.omarchy === "Catppuccin Latte" && document.documentElement.dataset.omarchyOpacity === "glassy" && document.documentElement.style.getPropertyValue("--aihio-radius-base") === "6px"');
  assert.equal(await js('matchMedia("(prefers-color-scheme: dark)").matches'), false);
  assert.match(await js('document.documentElement.style.getPropertyValue("--desk")'), /^rgba\(227, 228, 232, 0\.65\)$/);

  console.log('PASS: Pulse as home (with daily review), dictation, live Markdown, [[ link picker, search, typed capture (templates, string & list fields, toggle, promote), attachments (drop, strip, inline parc-attach:// preview, detach), external reload, draft restore, aggregate vaults, scoped save, fragment and vault graphs, Go to… with preview and grep, sidebar filters, grouping and sorting, type boards and the risk matrix (drag, keys, undo), history restore, archive and trash view with undo/restore, vault health and reindex, remove vault, Omarchy live System theme (colors, mode, rounding, glass), live search, quick capture, copying the capture command, theme preview and save, dictation languages and model list, vault settings, related notes as you write and dictate, tabs and the layout and drafts across a reload, and the Agent (ChatGPT + GitHub Copilot)');
}
check().then(() => finish(0), err => { console.error(err); finish(1); });
function finish(code) {
  for (const win of BrowserWindow.getAllWindows()) win.destroy();
  app.exit(code);
}
