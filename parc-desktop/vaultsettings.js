// Vault settings: which types new fragments can use, which one captures start as, and what each type holds
// (its fields, a choice field's values, and what each status means). Everything here is saved in the vault
// (config.yml and schemas/), so everyone who opens the vault gets it. Turning a type off keeps its fragments.
import { icon, typeIcon } from './icons.js';

const CATEGORIES = [['open', 'Open'], ['active', 'Active'], ['done', 'Done'], ['closed', 'Closed']];
const KINDS = [['string', 'Text'], ['date', 'Date'], ['enum', 'Choice'], ['list', 'List']];
const NAME = /^[A-Za-z][A-Za-z0-9_-]{0,63}$/, FIELD = /^[A-Za-z_][A-Za-z0-9_-]{0,63}$/, VALUE = /^[\p{L}\p{N}][\p{L}\p{N}_-]{0,63}$/u;

function el(tag, className = '', text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text != null) node.textContent = text;
  return node;
}
const capital = s => s ? s[0].toUpperCase() + s.slice(1).replace(/[-_]/g, ' ') : '';
const plural = (n, one, many = `${one}s`) => `${n.toLocaleString()} ${n === 1 ? one : many}`;
function button(label, variant, onClick, glyph) {
  const b = document.createElement('aihio-button');
  b.setAttribute('variant', variant); b.setAttribute('size', 'sm');
  if (glyph) b.append(icon(glyph));
  b.append(label); b.onclick = onClick;
  return b;
}
function select(options, value, label, onChange) {
  const s = el('select', 'vs-select');
  s.setAttribute('aria-label', label);
  for (const [v, text] of options) s.append(new Option(text, v));
  s.value = value ?? '';
  s.onchange = () => onChange(s.value);
  return s;
}
function input(value, label, onInput, placeholder = '') {
  const i = el('input', 'vs-input');
  Object.assign(i, { type: 'text', value: value ?? '', placeholder, autocomplete: 'off', spellcheck: false });
  i.setAttribute('aria-label', label);
  i.oninput = () => onInput(i.value);
  return i;
}

/**
 * deps: { api, vaults() → [{ path, name }], schemasOf(vault) → schema.list, counts(vault) → Promise<Map type → n>,
 *         reload(vault) → Promise (schemas, rows, open fragment), notify(message) → the status bar }
 */
export function vaultSettings({ api, vaults, schemasOf, counts, reload, notify }) {
  const $ = id => document.getElementById(id);
  const dialog = $('vaultSettings'), body = $('vsBody'), foot = $('vsFooter');
  let vault = '', view = 'types', model = null, usage = null, tally = new Map(), busy = false, nextKey = 1;
  let message = { text: '', tone: '' }, focusKey = null;

  const vaultName = () => vaults().find(v => v.path === vault)?.name || 'this vault';
  // The built-in types in their usual order, then the vault's own.
  const BUILTIN = ['note', 'todo', 'decision', 'risk', 'idea'], rank = s => { const i = BUILTIN.indexOf(s.name); return i < 0 ? BUILTIN.length : i; };
  const types = () => [...(schemasOf(vault) || [])].sort((a, b) => rank(a) - rank(b) || a.name.localeCompare(b.name));
  const say = (text = '', tone = '') => { message = { text: String(text?.message ?? text).replace(/^Error invoking remote method '[^']+': Error: /, ''), tone }; const m = $('vsMessage'); if (m) { m.textContent = message.text; m.dataset.tone = tone; } };

  async function open(initial) {
    vault = initial && vaults().some(v => v.path === initial) ? initial : vaults()[0]?.path || '';
    view = 'types'; model = null; say();
    await showTypes();
    dialog.open();
  }
  async function showTypes() {
    view = 'types'; model = null;
    try { tally = await counts(vault); } catch { tally = new Map(); }
    render();
  }
  // One change at a time; the dialog keeps its place and says what happened.
  async function change(run, done) {
    if (busy) return;
    busy = true; dialog.setAttribute('aria-busy', 'true');
    try { const result = await run(); await reload(vault); say(typeof done === 'function' ? done(result) : done); return result; }
    catch (err) { say(err, 'error'); return null; }
    finally { busy = false; dialog.removeAttribute('aria-busy'); render(); }
  }

  function render() {
    body.replaceChildren(); foot.replaceChildren();
    // What just happened, or what's in the way, next to the buttons: the body may be scrolled anywhere.
    const m = el('p', 'vs-message', message.text); m.id = 'vsMessage'; m.setAttribute('role', 'status'); m.dataset.tone = message.tone;
    foot.append(m);
    if (view === 'types') renderTypes(); else renderEditor();
    if (focusKey) { body.querySelector(`[data-key="${focusKey}"]`)?.focus(); focusKey = null; }
  }

  // ── The vault's types ──
  function renderTypes() {
    $('vsTitle').textContent = 'Vault settings';
    $('vsDescription').textContent = `Saved in ${vaultName()} (config.yml and schemas/), so everyone who opens the vault gets them.`;
    if (vaults().length > 1) {
      const row = el('label', 'setting vs-vault');
      row.append(el('span', 'setting-text', 'Vault'), select(vaults().map(v => [v.path, v.name]), vault, 'Vault', async p => { vault = p; say(); await showTypes(); }));
      body.append(row);
    }
    const list = types(), off = list.filter(s => s.enabled === false).map(s => s.name);
    const section = el('section'); section.setAttribute('aria-labelledby', 'vsTypesHeading');
    const h = el('h3', '', 'Types'); h.id = 'vsTypesHeading';
    section.append(h, el('p', 'setting-hint vs-lead', 'A type that is off keeps its fragments: they open, change and turn up in searches as before. New captures can’t use it.'));
    const ul = el('ul', 'type-list');
    for (const s of list) {
      const li = el('li', 'type-row'), text = el('div', 'setting-text'), n = tally.get(s.name) || 0;
      const facts = [s.alias && `Shortcut ${s.alias}`, plural(n, 'fragment'), s.default && 'New captures start here'].filter(Boolean).join(' · ');
      text.append(el('strong', '', capital(s.name)), el('span', 'setting-sub', facts));
      const sw = document.createElement('aihio-switch');
      sw.setAttribute('aria-label', `Use ${s.name} for new fragments`);
      if (s.enabled !== false) sw.setAttribute('checked', '');
      if (s.default || busy) sw.setAttribute('disabled', '');
      if (s.default) sw.title = 'New captures start as this type. Choose another below to turn it off.';
      sw.addEventListener('aihio-change', e => change(
        () => api.updateVaultSettings(vault, { types: { disabled: e.detail.checked ? off.filter(t => t !== s.name) : [...off, s.name] } }),
        e.detail.checked ? `${capital(s.name)} is on.` : `${capital(s.name)} is off. Its ${plural(n, 'fragment')} stay${n === 1 ? 's' : ''} as they are.`));
      const edit = button('Edit', 'ghost', () => editType(s));
      edit.setAttribute('aria-label', `Edit ${s.name}`);
      li.dataset.off = s.enabled === false ? 'true' : 'false';
      li.append(typeIcon(s.name), text, sw, edit);
      ul.append(li);
    }
    section.append(ul, button('New type', 'outline', () => editType(null), 'plus'));
    body.append(section);

    const start = el('section'); start.setAttribute('aria-labelledby', 'vsStartHeading');
    const h2 = el('h3', '', 'New captures'); h2.id = 'vsStartHeading';
    const row = el('div', 'setting'), t = el('div', 'setting-text');
    t.append(el('strong', '', 'Start as'), el('span', 'setting-sub', 'The type a new capture or quick capture starts as. Each capture can still pick another.'));
    const current = list.find(s => s.default)?.name || '';
    row.append(t, select(list.filter(s => s.enabled !== false).map(s => [s.name, capital(s.name)]), current, 'New captures start as',
      name => change(() => api.updateVaultSettings(vault, { types: { default: name } }), `New captures start as ${name}.`)));
    start.append(h2, row);
    body.append(start);
    const done = button('Done', 'default', () => dialog.close());
    foot.append(done);
  }

  // ── One type: its fields and their values ──
  const valueRow = (name, original, category = 'open') => ({ key: nextKey++, original, name, category, removed: false, moveTo: '' });
  function fieldModel(f, existing, currentField = null) {
    const values = (f.values || []).map(v => valueRow(v, existing && (!currentField || currentField.values?.includes(v)) ? v : null, f.categories?.[v] || 'open'));
    const keyOf = v => values.find(x => x.name === v)?.key ?? '';
    return {
      key: nextKey++, original: existing ? f.name : null, name: f.name, kind: f.kind, required: !!f.required, values,
      default: f.kind === 'enum' ? keyOf(f.default) : f.default || '',
      toggle: f.toggle ? [keyOf(f.toggle[0]), keyOf(f.toggle[1])] : ['', ''],
    };
  }
  function blankType() {
    const status = fieldModel({ name: 'status', kind: 'enum', values: ['open', 'done'], required: true, default: 'open', toggle: ['open', 'done'], categories: { open: 'open', done: 'done' } }, false);
    return { create: true, name: '', alias: '', editor_skip: false, builtin: false, fields: [status] };
  }
  async function editType(s) {
    say();
    usage = { total: 0, values: {} };
    if (s) { try { usage = await api.schemaUsage(vault, s.name); } catch (err) { say(err, 'error'); return render(); } }
    model = s ? { create: false, name: s.name, alias: s.alias || '', editor_skip: !!s.editor_skip, builtin: !!s.builtin, default: !!s.default, fields: s.fields.map(f => fieldModel(f, true)) } : blankType();
    view = 'edit'; focusKey = s ? null : 'type-name';
    render();
  }
  // The built-in definition replaces the form; values in use that it lacks wait for a place to move to.
  async function restoreBuiltin() {
    let builtin;
    try { builtin = await api.builtinSchema(vault, model.name); } catch (err) { say(err, 'error'); return; }
    const current = types().find(s => s.name === model.name);
    model.alias = builtin.alias || ''; model.editor_skip = !!builtin.editor_skip;
    model.fields = builtin.fields.map(f => {
      const now = current?.fields.find(x => x.name === f.name);
      const field = fieldModel(f, !!now, now);
      if (now && f.kind === 'enum') {
        for (const [value, n] of Object.entries(usage.values[f.name] || {})) {
          if (n && !field.values.some(v => v.name === value)) field.values.push({ ...valueRow(value, value), removed: true });
        }
      }
      return field;
    });
    say('Built-in definition loaded. Save to use it.');
    render();
  }

  const live = f => f.values.filter(v => !v.removed);
  const isStatus = f => f.kind === 'enum' && f.name.trim() === 'status';
  const inUse = (f, v) => (f.original && v.original && usage.values[f.original]?.[v.original]) || 0;
  function toSchema() {
    return {
      name: model.name.trim(), alias: model.alias.trim() || null, editor_skip: model.editor_skip,
      fields: model.fields.map(f => {
        const values = live(f), nameOf = key => values.find(v => v.key === key)?.name.trim() || null, choice = f.kind === 'enum';
        const [off, on] = (f.toggle || ['', '']).map(nameOf);
        return {
          name: f.name.trim(), kind: f.kind, required: f.required,
          values: choice ? values.map(v => v.name.trim()) : null,
          default: choice ? nameOf(f.default) : f.kind === 'list' ? null : (f.default || '').trim() || null,
          toggle: choice && off && on ? [off, on] : null,
          categories: isStatus(f) ? Object.fromEntries(values.map(v => [v.name.trim(), v.category])) : null
        };
      })
    };
  }
  // Fragments follow renamed values, and removed values move where the form says.
  function renames() {
    const out = [];
    for (const f of model.fields) {
      if (!f.original || f.kind !== 'enum') continue;
      for (const v of f.values) {
        if (!v.original) continue;
        if (v.removed) { const to = live(f).find(x => x.key === Number(v.moveTo)); if (to) out.push({ field: f.original, from: v.original, to: to.name.trim() }); }
        else if (v.name.trim() !== v.original) out.push({ field: f.original, from: v.original, to: v.name.trim() });
      }
    }
    return out;
  }
  // The server checks everything; this catches what the form can point at.
  function problem() {
    if (model.create && !NAME.test(model.name.trim())) return 'Name the type with a letter, then letters, digits, - or _.';
    if (model.create && types().some(s => s.name === model.name.trim() || s.alias === model.name.trim())) return `A type or shortcut named “${model.name.trim()}” exists already.`;
    const names = new Set();
    for (const f of model.fields) {
      const name = f.name.trim();
      if (!FIELD.test(name)) return `Name every field with a letter, then letters, digits, - or _${name ? ` (“${name}”)` : ''}.`;
      if (names.has(name.toLowerCase())) return `Two fields are named “${name}”.`;
      names.add(name.toLowerCase());
      if (f.kind !== 'enum') continue;
      const values = live(f);
      if (!values.length) return `Give ${name} at least one value.`;
      const seen = new Set();
      for (const v of values) {
        if (!VALUE.test(v.name.trim())) return `Values of ${name} are single words: letters, digits, - or _${v.name.trim() ? ` (“${v.name.trim()}”)` : ''}.`;
        if (seen.has(v.name.trim())) return `${capital(name)} lists “${v.name.trim()}” twice.`;
        seen.add(v.name.trim());
      }
      const stranded = f.values.find(v => v.removed && inUse(f, v) && !values.some(x => x.key === Number(v.moveTo)));
      if (stranded) { const n = inUse(f, stranded); return `Choose where the ${plural(n, 'fragment')} with ${name} “${stranded.original}” ${n === 1 ? 'goes' : 'go'}.`; }
    }
    return '';
  }
  async function save() {
    const why = problem();
    if (why) { say(why, 'error'); return; }
    const schema = toSchema(), moves = renames(), create = model.create;
    const result = await change(() => api.saveSchema(vault, { schema, create, renames: moves }),
      r => `${create ? 'Created' : 'Saved'} ${schema.name}.${r.migrated ? ` ${plural(r.migrated, 'fragment')} moved to ${r.migrated === 1 ? 'its' : 'their'} new value.` : ''}${r.warnings?.length ? ` ${r.warnings.join('; ')}` : ''}`);
    if (!result) return;
    notify(message.text);
    await showTypes();
  }
  async function remove() {
    const name = model.name;
    const result = await change(() => api.deleteSchema(vault, name), r => r.deleted ? `Deleted ${name}.` : '');
    if (result?.deleted) { notify(`Deleted the type ${name}.`); await showTypes(); }
  }

  function renderEditor() {
    const m = model;
    $('vsTitle').textContent = m.create ? 'New type' : `Edit ${capital(m.name)}`;
    $('vsDescription').textContent = m.create
      ? `A new kind of fragment in ${vaultName()}. A status field is a good start: open work and the T toggle follow it.`
      : `${plural(usage.total, 'fragment')} in ${vaultName()} ${usage.total === 1 ? 'is' : 'are'} ${m.name}. Renamed values move them along; removed values ask where they go.`;
    const nav = el('div', 'vs-nav');
    nav.append(button('Types', 'ghost', () => { say(); showTypes(); }, 'back'));
    body.append(nav);

    const ident = el('section', 'vs-ident');
    const nameRow = el('label', 'vs-row'), aliasRow = el('label', 'vs-row');
    nameRow.append(el('span', 'vs-label', 'Name'));
    if (m.create) { const i = input(m.name, 'Type name', v => { m.name = v; }, 'e.g. bug'); i.dataset.key = 'type-name'; nameRow.append(i); }
    else nameRow.append(el('span', 'vs-fixed', m.name), el('span', 'setting-sub', 'Fragments and hooks refer to a type by its name, so it stays.'));
    aliasRow.append(el('span', 'vs-label', 'Shortcut'), input(m.alias, 'Shortcut', v => { m.alias = v; }, 'optional, e.g. b'),
      el('span', 'setting-sub', 'Lets the terminal create one with parc <shortcut> "title".'));
    ident.append(nameRow, aliasRow);
    body.append(ident);

    const fields = el('section'); fields.setAttribute('aria-labelledby', 'vsFieldsHeading');
    const h = el('h3', '', 'Fields'); h.id = 'vsFieldsHeading';
    fields.append(h);
    if (!m.fields.length) fields.append(el('p', 'setting-hint', 'No fields: fragments of this type have a title, tags and text, like notes.'));
    const ul = el('ul', 'field-list');
    for (const f of m.fields) ul.append(fieldCard(f));
    fields.append(ul, button('Add field', 'outline', () => {
      const f = fieldModel({ name: '', kind: 'string' }, false); m.fields.push(f); focusKey = `field-${f.key}`; render();
    }, 'plus'));
    if (!m.create) fields.append(el('p', 'setting-hint', 'Removing a field keeps its values in the fragments’ files; parc stops showing and checking them.'));
    body.append(fields);

    const left = el('div', 'vs-foot-start');
    if (!m.create && !usage.total && !m.default) left.append(button('Delete type', 'ghost', remove, 'trash'));
    if (!m.create && m.builtin) left.append(button('Restore built-in', 'ghost', restoreBuiltin, 'history'));
    const cancel = button('Cancel', 'outline', () => { say(); showTypes(); });
    const ok = button(m.create ? 'Create type' : 'Save type', 'default', save);
    if (busy) ok.setAttribute('loading', '');
    foot.prepend(left); foot.append(cancel, ok);
  }

  function fieldCard(f) {
    const li = el('li', 'field-card'), head = el('div', 'field-head');
    const label = f.name.trim() || 'new field';
    if (f.original) head.append(el('strong', 'field-name', f.name));
    else { const i = input(f.name, 'Field name', v => { f.name = v; }, 'field name'); i.dataset.key = `field-${f.key}`; i.onchange = () => render(); head.append(i); }
    head.append(select(KINDS, f.kind, `Kind of ${label}`, kind => {
      f.kind = kind; f.default = ''; f.toggle = ['', ''];
      if (kind === 'enum' && !f.values.length) { const v = valueRow('', null); f.values.push(v); focusKey = `value-${v.key}`; }
      render();
    }));
    const req = el('label', 'vs-check'), box = el('input');
    box.type = 'checkbox'; box.checked = f.required; box.onchange = () => { f.required = box.checked; };
    req.append(box, 'Required');
    if (f.kind === 'list') req.title = 'A list can be required, but has no default.';
    const drop = el('button', 'vs-icon'); drop.type = 'button'; drop.append(icon('trash'));
    drop.title = `Remove ${label}`; drop.setAttribute('aria-label', drop.title);
    drop.onclick = () => { model.fields = model.fields.filter(x => x !== f); render(); };
    head.append(req, drop);
    li.append(head);

    if (f.kind === 'enum') li.append(valueList(f), choiceExtras(f));
    else if (f.kind === 'string' || f.kind === 'date') {
      const row = el('label', 'vs-row');
      row.append(el('span', 'vs-label', 'Default'), input(f.default, `Default ${label}`, v => { f.default = v; }, f.kind === 'date' ? 'YYYY-MM-DD, or none' : 'none'));
      li.append(row);
    }
    return li;
  }

  function valueList(f) {
    const ul = el('ul', 'value-list'), status = isStatus(f), others = v => live(f).filter(x => x !== v);
    if (status) ul.setAttribute('aria-label', `Values of ${f.name}, and what each means`);
    for (const v of f.values) {
      const li = el('li', 'value-row'), n = inUse(f, v);
      if (v.removed) {
        li.dataset.removed = '';
        li.append(el('span', 'value-gone', v.original || v.name));
        if (n) li.append(el('span', 'value-move', `${plural(n, 'fragment')} ${n === 1 ? 'moves' : 'move'} to`), select([['', 'Choose…'], ...others(v).map(x => [String(x.key), x.name || '(unnamed)'])], v.moveTo, `Move fragments with ${v.original} to`, to => { v.moveTo = to; }));
        const undo = el('button', 'vs-link', 'Keep'); undo.type = 'button'; undo.onclick = () => { v.removed = false; v.moveTo = ''; render(); };
        li.append(undo);
        ul.append(li);
        continue;
      }
      const name = input(v.name, `Value of ${f.name || 'field'}`, text => { v.name = text; renamed.hidden = !(v.original && text.trim() !== v.original); }, 'value');
      name.dataset.key = `value-${v.key}`;
      name.onchange = () => render(); // default, toggle and move menus show the new name
      name.onkeydown = e => { if (e.key === 'Enter') { e.preventDefault(); const next = valueRow('', null); f.values.splice(f.values.indexOf(v) + 1, 0, next); focusKey = `value-${next.key}`; render(); } };
      li.append(name);
      const renamed = el('span', 'value-was', `was ${v.original}`); renamed.hidden = !(v.original && v.name.trim() !== v.original);
      if (status) {
        const cat = select(CATEGORIES, v.category, `What ${v.name || 'this value'} means`, c => { v.category = c; cat.dataset.tone = c; });
        cat.dataset.tone = v.category; cat.classList.add('vs-category');
        li.append(cat);
      }
      li.append(renamed);
      if (n) li.append(el('span', 'value-count', n.toLocaleString()));
      const x = el('button', 'vs-icon'); x.type = 'button'; x.append(icon('x'));
      x.title = `Remove ${v.name || 'value'}`; x.setAttribute('aria-label', x.title);
      x.onclick = () => {
        if (n) { v.removed = true; v.moveTo = ''; } else f.values = f.values.filter(y => y !== v);
        if (f.default === v.key) f.default = '';
        render();
      };
      li.append(x);
      ul.append(li);
    }
    const wrap = el('div', 'value-block');
    const add = el('button', 'vs-link', '+ Add value'); add.type = 'button';
    add.onclick = () => { const v = valueRow('', null); f.values.push(v); focusKey = `value-${v.key}`; render(); };
    wrap.append(ul, add);
    if (status) wrap.append(el('p', 'setting-hint', 'Done and Closed are finished: they leave open-work lists, due and stale reports, and the Open filter. Active is under way.'));
    return wrap;
  }

  function choiceExtras(f) {
    const row = el('div', 'vs-extras'), options = live(f).map(v => [String(v.key), v.name || '(unnamed)']);
    const def = el('label', 'vs-row');
    def.append(el('span', 'vs-label', 'Default'), select([['', 'None'], ...options], String(f.default ?? ''), `Default ${f.name}`, v => { f.default = v ? Number(v) : ''; }));
    row.append(def);
    if (isStatus(f)) {
      const t = el('div', 'vs-row'), [off, on] = f.toggle || ['', ''];
      t.append(el('span', 'vs-label', 'T toggles'),
        select([['', '—'], ...options], String(off ?? ''), 'Toggle from', v => { f.toggle = [v ? Number(v) : '', f.toggle?.[1] ?? '']; }),
        el('span', 'vs-arrow', '⇄'),
        select([['', '—'], ...options], String(on ?? ''), 'Toggle to', v => { f.toggle = [f.toggle?.[0] ?? '', v ? Number(v) : '']; }));
      row.append(t);
    }
    return row;
  }

  return { open, get isOpen() { return dialog.hasAttribute('open'); } };
}
