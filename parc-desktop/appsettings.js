// The app's Settings dialog (#settings in index.html): the theme, dictation (languages, speech models, whisper-cli),
// the quick capture shortcut and command, each vault's inbox folder, and the meaning model recall uses. Choices wait
// for Save; downloading or deleting a model happens at once. main.cjs keeps the settings; settings.cjs checks them.
import { icon } from './icons.js';
import { LANGUAGES } from './languages.cjs';

/**
 * speech(): dictation's status as checkSpeech() last found it; checkSpeech(): look again. vaults(): the opened
 * vaults. el, status, cleanError, languageName: the workspace's helpers.
 */
export function appSettings({ api, el, status, cleanError, speech, checkSpeech, languageName, vaults }) {
  const $ = id => document.getElementById(id);
  // Choices wait for Save (`pending`); downloading and deleting a model happen at once.
  let pending = null, models = null, settingsView = null;
  function renderSettings(view = settingsView) {
    settingsView = view;
    const custom = pending.whisperModel && !models?.models.some(m => m.path === pending.whisperModel);
    $('themeSub').textContent = view.omarchy
      ? `System follows your Omarchy theme (${view.omarchy.name}${view.omarchy.opacity && view.omarchy.opacity !== 'opaque' ? `, ${view.omarchy.opacity}` : ''}).`
      : 'System follows your desktop’s light or dark setting.';
    $('modelPath').textContent = custom ? pending.whisperModel : 'None'; $('modelPath').title = custom ? pending.whisperModel : '';
    $('binPath').textContent = pending.whisperBin || 'whisper-cli from PATH'; $('binPath').title = pending.whisperBin;
    $('clearBin').hidden = !pending.whisperBin;
    const where = speech().language && speech().language !== 'auto' ? ` in ${languageName(speech().language)}` : '';
    $('speechState').textContent = speech().ready ? speech().warning || `Ready to dictate${where}.` : speech().problem || 'Not set up.';
    $('speechState').dataset.tone = speech().ready && !speech().warning ? 'ok' : 'warn';
    const asked = `Asked the desktop for ${view.quickCaptureShortcut}. Wayland desktops don’t say whether they agreed: if it does nothing, bind the command below instead.`;
    $('shortcutState').textContent = view.shortcut?.error || (view.shortcut?.asked ? asked : view.shortcut?.active ? `${view.quickCaptureShortcut} opens quick capture.` : 'Leave empty for none.');
    $('shortcutState').dataset.tone = view.shortcut?.error ? 'error' : view.shortcut?.asked ? 'warn' : '';
    $('captureCommand').textContent = view.captureCommand;
    renderLanguages(); renderModels(); renderMeaning(); renderInboxFolders();
  }
  async function renderInboxFolders() {
    const box = $('inboxFolders');
    if (!box) return;
    const entries = await Promise.all(vaults().map(async v => ({
      vault: v,
      dir: await api.inboxPath(v.path).catch(() => `${v.path}/inbox`)
    })));
    box.replaceChildren();
    for (const { vault: v, dir } of entries) {
      const row = el('div', 'setting inbox-folder-row');
      const txt = el('div', 'setting-text');
      txt.append(el('strong', '', v.name), el('span', 'path', dir));
      txt.querySelector('.path').title = dir;
      const copy = smallButton('Copy', 'outline', () => {
        api.copyText(dir).then(() => status(`Copied ${v.name} inbox path.`), status);
      });
      const open = smallButton('Open folder', 'outline', () => {
        api.openInboxFolder(v.path).catch(status);
      });
      row.append(txt, copy, open);
      box.append(row);
    }
  }
  function renderLanguages() {
    const box = $('languageList'), chosen = pending.speechLanguages;
    box.replaceChildren();
    for (const code of chosen) {
      const chip = el('span', 'tag'), remove = el('button', 'tag-x');
      remove.type = 'button'; remove.title = `Remove ${languageName(code)}`; remove.setAttribute('aria-label', remove.title);
      remove.append(icon('x'));
      remove.onclick = () => { pending.speechLanguages = chosen.filter(c => c !== code); refreshModels().catch(modelMessage); };
      chip.append(languageName(code), remove);
      box.append(chip);
    }
    const add = el('select', 'language-add');
    add.setAttribute('aria-label', 'Add a language');
    add.append(new Option(chosen.length ? 'Add a language…' : 'Choose a language…', ''));
    for (const [code, label] of LANGUAGES.filter(c => !chosen.includes(c)).map(c => [c, languageName(c)]).sort((a, b) => a[1].localeCompare(b[1]))) {
      add.append(new Option(label, code));
    }
    add.disabled = chosen.length >= 12;
    add.onchange = () => {
      if (!add.value) return;
      pending.speechLanguages = [...chosen, add.value];
      refreshModels().then(() => $('languageList').querySelector('select')?.focus()).catch(modelMessage);
    };
    box.append(add);
  }
  const megabytes = bytes => `${Math.round(bytes / 1e6)} MB`;
  function smallButton(label, variant, onClick) {
    const b = document.createElement('aihio-button');
    b.setAttribute('variant', variant); b.setAttribute('size', 'sm'); b.append(label); b.onclick = onClick;
    return b;
  }
  function renderModels() {
    const list = $('modelList');
    list.replaceChildren();
    if (!models) return;
    const busy = models.models.some(m => m.downloading);
    for (const m of models.models) {
      const li = el('li', 'model'), text = el('div', 'model-text'), actions = el('div', 'model-actions');
      li.dataset.id = m.id;
      const title = el('strong', '', m.label);
      if (m.id === models.recommended) {
        const badge = document.createElement('aihio-badge'); badge.setAttribute('variant', 'secondary'); badge.append('Recommended');
        title.append(' ', badge);
      }
      text.append(title, el('span', 'model-note', `${m.note} ${megabytes(m.size)}.`));
      if (m.downloading) {
        const bar = el('progress'); bar.max = m.size; bar.value = m.downloading.received;
        bar.setAttribute('aria-label', `Downloading ${m.label}`);
        actions.append(bar, el('span', 'model-percent', `${Math.floor(100 * m.downloading.received / m.size)}%`),
          smallButton('Cancel', 'ghost', () => api.cancelDownload().catch(modelMessage)));
      } else if (m.installed) {
        actions.append(pending.whisperModel === m.path ? el('span', 'model-in-use', 'In use')
          : smallButton('Use', 'outline', () => { pending.whisperModel = m.path; renderSettings(); }));
        const remove = smallButton(icon('trash'), 'ghost', () => deleteModel(m));
        remove.setAttribute('size', 'icon'); remove.setAttribute('aria-label', `Delete ${m.label}`); remove.title = remove.getAttribute('aria-label');
        actions.append(remove);
      } else {
        const get = smallButton('Download', 'outline', () => downloadModel(m));
        get.toggleAttribute('disabled', busy);
        actions.append(get);
      }
      li.append(text, actions);
      list.append(li);
    }
  }
  function modelMessage(message) { $('modelMessage').textContent = message ? cleanError(message) : ''; }
  async function refreshModels() {
    models = await api.speechModels(pending.speechLanguages);
    renderSettings();
  }
  async function downloadModel(m) {
    modelMessage(`Downloading ${m.label}…`);
    const done = api.downloadModel(m.id);
    refreshModels().catch(() => {});
    try {
      const result = await done;
      if (result.cancelled) modelMessage('Download cancelled. Download again to resume where it stopped.');
      else {
        if (result.inUse || !pending.whisperModel) pending.whisperModel = result.path;
        modelMessage(`${m.label} downloaded${result.inUse ? ' and in use' : ''}.`);
        if (!$('settings').hasAttribute('open')) status(`Speech model downloaded${result.inUse ? '. Dictation is ready' : ''}.`);
      }
    } catch (err) { modelMessage(err); }
    await checkSpeech();
    if (pending) await refreshModels().catch(() => {});
  }
  async function deleteModel(m) {
    try {
      const result = await api.deleteModel(m.id);
      if (!result.deleted) return;
      if (pending.whisperModel === result.path) pending.whisperModel = '';
      modelMessage(`${m.label} deleted.`);
      await checkSpeech(); await refreshModels();
    } catch (err) { modelMessage(err); }
  }
  // ── The meaning model (recall): one model, downloaded for every vault and the parc command ──
  let meaning = null, meaningPoll = null;
  function meaningMessage(message) { $('meaningMessage').textContent = message ? cleanError(message) : ''; }
  const count = n => n.toLocaleString();
  function renderMeaning() {
    const state = $('meaningState'), list = $('meaningList');
    list.replaceChildren();
    if (!meaning) { state.textContent = 'Recall isn’t available right now.'; state.dataset.tone = 'warn'; return; }
    const { embedded, total } = meaning.coverage;
    if (meaning.error) { state.textContent = `The meaning model didn’t load: ${meaning.error}`; state.dataset.tone = 'error'; }
    else if (!meaning.installed) { state.textContent = 'Related finds notes by shared words, tags and links.'; state.dataset.tone = ''; }
    else if (embedded < total) { state.textContent = `Reading your notes for meaning: ${count(embedded)} of ${count(total)}.`; state.dataset.tone = ''; }
    else { state.textContent = 'Related finds notes by meaning too, across languages.'; state.dataset.tone = 'ok'; }
    const li = el('li', 'model'), text = el('div', 'model-text'), actions = el('div', 'model-actions');
    li.dataset.id = 'meaning';
    text.append(el('strong', '', meaning.label), el('span', 'model-note', `${meaning.license}. ${megabytes(meaning.size)}.`));
    if (meaning.downloading) {
      const bar = el('progress'); bar.max = meaning.size; bar.value = meaning.downloading.received;
      bar.setAttribute('aria-label', `Downloading ${meaning.label}`);
      actions.append(bar, el('span', 'model-percent', `${Math.floor(100 * meaning.downloading.received / meaning.size)}%`),
        smallButton('Cancel', 'ghost', () => api.cancelMeaningDownload().catch(meaningMessage)));
    } else if (meaning.installed) {
      const remove = smallButton(icon('trash'), 'ghost', () => deleteMeaning());
      remove.setAttribute('size', 'icon'); remove.setAttribute('aria-label', `Delete ${meaning.label}`); remove.title = remove.getAttribute('aria-label');
      actions.append(el('span', 'model-in-use', 'In use'), remove);
    } else actions.append(smallButton('Download', 'outline', () => downloadMeaning()));
    li.append(text, actions);
    list.append(li);
    // While notes are being read, show it move.
    const reading = meaning.installed && !meaning.error && embedded < total;
    if (reading && !meaningPoll) meaningPoll = setInterval(() => refreshMeaning().catch(() => {}), 2000);
    if (!reading && meaningPoll) { clearInterval(meaningPoll); meaningPoll = null; }
  }
  async function refreshMeaning() {
    if (!$('settings').hasAttribute('open')) { clearInterval(meaningPoll); meaningPoll = null; return; }
    meaning = await api.meaningModel();
    renderMeaning();
  }
  async function downloadMeaning() {
    meaningMessage(`Downloading ${meaning.label}…`);
    const done = api.downloadMeaningModel();
    refreshMeaning().catch(() => {});
    try {
      const result = await done;
      if (result.cancelled) meaningMessage('Download cancelled. Download again to resume where it stopped.');
      else {
        meaningMessage(`${meaning.label} downloaded. Your notes are read in the background.`);
        if (!$('settings').hasAttribute('open')) status('Meaning model downloaded. Related now finds notes by meaning too.');
      }
    } catch (err) { meaningMessage(err); }
    await refreshMeaning().catch(() => {});
  }
  async function deleteMeaning() {
    try {
      const result = await api.deleteMeaningModel();
      if (!result.deleted) return;
      meaningMessage(`${meaning.label} deleted. Recall stops using it when parc restarts.`);
      await refreshMeaning();
    } catch (err) { meaningMessage(err); }
  }
  api.onMeaningProgress(p => {
    if (!meaning) return;
    if (p.done) { meaning.downloading = null; return; }
    meaning.downloading = { received: p.received, size: p.size };
    const row = $('meaningList').querySelector('[data-id="meaning"]'), bar = row?.querySelector('progress');
    if (!bar) return renderMeaning();
    bar.value = p.received;
    row.querySelector('.model-percent').textContent = `${Math.floor(100 * p.received / p.size)}%`;
  });
  // Moves the open dialog's progress bar in place, so the Cancel button keeps focus.
  api.onModelProgress(p => {
    const m = models?.models.find(x => x.id === p.id);
    if (!m) return;
    if (p.done) { m.downloading = null; return; }
    m.downloading = { received: p.received, size: p.size };
    const row = $('modelList').querySelector(`[data-id="${CSS.escape(p.id)}"]`), bar = row?.querySelector('progress');
    if (!bar) return renderModels();
    bar.value = p.received;
    row.querySelector('.model-percent').textContent = `${Math.floor(100 * p.received / p.size)}%`;
  });
  async function openSettings() {
    const view = await api.settings();
    pending = { whisperModel: view.whisperModel, whisperBin: view.whisperBin, speechLanguages: [...(view.speechLanguages || [])], theme: view.theme };
    for (const radio of $('themeChoice').querySelectorAll('input')) radio.checked = radio.value === view.theme;
    $('shortcutInput').value = view.quickCaptureShortcut;
    modelMessage(''); meaningMessage('');
    [models, meaning] = await Promise.all([api.speechModels(pending.speechLanguages).catch(() => null),
      api.meaningModel().catch(() => null), checkSpeech()]);
    renderSettings(view);
    $('settings').open();
  }
  async function pickFor(kind) {
    const path = await api.pickFile(kind);
    if (!path) return;
    pending[kind === 'model' ? 'whisperModel' : 'whisperBin'] = path;
    renderSettings(await api.settings());
  }
  // A theme shows at once; closing without Save puts the saved one back (a no-op after Save).
  $('themeChoice').addEventListener('change', e => { pending.theme = e.target.value; api.previewTheme(pending.theme).catch(status); });
  $('settings').addEventListener('aihio-close', () => api.previewTheme().catch(() => {}));
  $('pickModel').addEventListener('click', () => pickFor('model').catch(status));
  $('pickBin').addEventListener('click', () => pickFor('binary').catch(status));
  $('clearBin').addEventListener('click', async () => { pending.whisperBin = ''; renderSettings(await api.settings()); });
  $('copyCaptureCommand').addEventListener('click', () => {
    api.copyText(settingsView.captureCommand).then(() => status('Copied the quick capture command. Bind a key to it in your desktop’s shortcut settings.'), status);
  });
  $('settingsSave').addEventListener('click', async () => {
    try {
      const before = settingsView?.quickCaptureShortcut;
      const view = await api.saveSettings({ ...pending, quickCaptureShortcut: $('shortcutInput').value });
      await checkSpeech(); renderSettings(view);
      if (!view.shortcut?.error) {
        $('settings').close();
        // Wayland desktops don't confirm a shortcut, so say what to do if a new one doesn't work.
        status(view.shortcut?.asked && view.quickCaptureShortcut !== before
          ? `Settings saved. If ${view.quickCaptureShortcut} does nothing, bind the quick capture command in your desktop instead.`
          : speech().ready ? 'Settings saved. Dictation is ready.' : 'Settings saved.');
      }
    } catch (err) { $('shortcutState').textContent = cleanError(err); $('shortcutState').dataset.tone = 'error'; }
  });

  return { open: openSettings };
}
