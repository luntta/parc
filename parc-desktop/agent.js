// The Agent view: ask about your vaults and read the answer, with each search and read the agent made on the way
// to it. The model runs in the main process (agent.cjs) on the user's ChatGPT plan; this module only shows it.
// Answers are Markdown shown as the editor's preview shows a fragment, so [[ID]] citations read as titles and open
// their fragment. Each Agent tab holds one chat. A chat is saved (on this computer) once a question gets an answer,
// and then keeps the vaults it was started in; an empty chat lists the saved ones to go back to.
import { markdownPreview } from './editor.js';
import { icon, typeIcon } from './icons.js';

function el(tag, className = '', text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text != null) node.textContent = text;
  return node;
}
function button(label, { variant = 'ghost', size = 'sm', onClick, glyph } = {}) {
  const b = document.createElement('aihio-button');
  b.setAttribute('variant', variant); b.setAttribute('size', size);
  if (glyph) b.append(icon(glyph));
  b.append(label);
  if (onClick) b.addEventListener('click', onClick);
  return b;
}
const cleanError = err => String(err?.message ?? err).replace(/^Error invoking remote method '[^']+': Error: /, '');
const newId = () => crypto.randomUUID();

/**
 * host: where the view draws (its results element). api: window.parc. chatId: a saved chat to show, or none for a new one.
 * scope(): { vaults: [{ path, name }], label } in view now: what a new chat searches. vaultNames(paths): those vaults as
 * [{ path, name, open }]. subject(): the saved fragment used last ({ vault, id, title, type }) or null.
 * links(paths): how answers resolve and open [[ID]] citations in those vaults (see editor.js). notify(message, action):
 * the status line. open(id, how): show a saved chat. fresh(how): a new chat. remove(id, title): delete a saved chat.
 * changed(): the chat's title or vaults changed. clickHow(event): where a click asks to open something.
 */
export function agentChat(host, deps) {
  const { api, scope, vaultNames, subject, notify, open, fresh, remove, changed, clickHow } = deps;
  const root = el('div', 'agent'), account = el('div', 'agent-account'), log = el('ol', 'agent-log');
  const start = el('div', 'agent-start'), compose = el('form', 'agent-compose');
  log.setAttribute('aria-label', 'Conversation');
  root.append(account, log, start, compose);
  host.replaceChildren(root);

  // The composer: what the question goes to and with, the question, and Ask (Stop while it runs).
  const context = el('div', 'agent-context'), where = el('span', 'agent-where');
  const about = el('button', 'chip agent-about'); about.type = 'button';
  const input = el('textarea', 'agent-input');
  input.rows = 1; input.placeholder = 'Ask about your notes…'; input.setAttribute('aria-label', 'Question for the agent');
  const row = el('div', 'agent-compose-row'), hint = el('span', 'agent-keys', 'Enter asks, Shift+Enter starts a new line');
  const another = button('New chat', { glyph: 'compose', onClick: e => fresh(clickHow(e)) });
  const ask = button('Ask', { variant: 'default' }); ask.setAttribute('type', 'submit');
  const stop = button('Stop', { variant: 'outline', glyph: 'stop', onClick: () => api.agentStop(chat.id) }); stop.hidden = true;
  context.append(where, about);
  row.append(another, hint, stop, ask);
  compose.append(context, input, row);

  // chat: { id, provider, title, vaults }. A new chat has neither provider nor vaults: it runs on the provider chosen
  // for new chats and searches the vaults in view. Once saved it keeps both for good.
  let chat = { id: deps.chatId || newId(), provider: null, title: '', vaults: null };
  let accounts = { chatgpt: {}, copilot: {} }, preferred = 'chatgpt', models = { chatgpt: null, copilot: null }, problem = '', code = null;
  let busy = false, sharing = true, turn = null, last = '', wantFocus = false;
  let loading = deps.chatId ? load(deps.chatId) : null, saved = [];
  const texts = []; // every answer's text parts, to paint again when citations resolve differently
  const searched = () => chat.vaults ? vaultNames(chat.vaults) : scope().vaults.map(v => ({ ...v, open: true }));
  const links = () => deps.links(searched().map(v => v.path));
  const share = () => { const s = subject(); return sharing && s && searched().some(v => v.path === s.vault) ? s : null; };
  const empty = () => !log.children.length;
  const current = () => chat.provider || preferred;
  const signedIn = () => !!accounts[current()]?.signedIn;
  const NAME = { chatgpt: 'ChatGPT', copilot: 'GitHub Copilot' };
  const who = p => p === 'copilot' ? accounts.copilot.login : accounts.chatgpt.email;

  // ── Accounts: a card that says what signing in means, or one quiet line for the provider in use ──
  function continueButton(p) {
    const label = `Continue with ${NAME[p]}`;
    if (p === 'copilot' && accounts.copilot.available === false) {
      const b = button(label, { variant: 'outline', size: 'md' }); b.setAttribute('disabled', '');
      b.title = 'This copy of parc has no GitHub app to sign in with yet. See the desktop README, “GitHub Copilot”.';
      return b;
    }
    const b = button(label, { variant: p === current() || !chat.provider ? 'default' : 'outline', size: 'md', onClick: () => (p === 'copilot' ? signInCopilot() : signIn()) });
    b.classList.add(p === 'copilot' ? 'agent-continue-copilot' : 'agent-continue');
    return b;
  }
  function paintAccount() {
    account.replaceChildren();
    const p = current();
    if (!signedIn()) {
      const card = el('div', 'agent-signin');
      // A saved chat stays on its provider; a new one can start on either.
      const offer = chat.provider ? [chat.provider] : accounts[preferred === 'chatgpt' ? 'copilot' : 'chatgpt']?.signedIn ? [p] : ['chatgpt', 'copilot'];
      card.append(
        el('h2', 'agent-signin-title', chat.provider ? `This chat runs on ${NAME[chat.provider]}` : offer.length > 1 ? 'Ask your notes, with your ChatGPT or GitHub Copilot plan' : `Ask your notes with ${NAME[p]}`),
        el('p', '', 'The agent searches and reads your fragments to answer, and cites the ones it used. It can’t change anything.'),
        el('p', 'agent-privacy', `Your questions, and the fragments the agent reads to answer them, go to ${offer.length > 1 ? 'OpenAI or GitHub, whichever you use,' : offer[0] === 'copilot' ? 'GitHub' : 'OpenAI'} and count against that plan’s usage. Nothing is sent until you ask.`),
        el('p', 'agent-privacy', offer.map(x => x === 'copilot' ? 'GitHub Copilot: Free, Pro, Pro+, Business and Enterprise.' : 'ChatGPT: Go, Plus, Pro and Business plans.').join(' ')));
      const actions = el('div', 'agent-signin-actions');
      for (const x of offer) {
        if (x === 'chatgpt' && accounts.chatgpt.signingIn) actions.append(el('span', 'agent-waiting-note', 'Finish signing in to ChatGPT in your browser…'), button('Cancel', { variant: 'outline', onClick: () => api.agentCancelSignIn() }));
        else if (x === 'copilot' && accounts.copilot.signingIn) actions.append(codeBox());
        else actions.append(continueButton(x));
      }
      // Switched a new chat to a provider that isn't signed in: back to the one that is.
      if (!chat.provider && offer.length === 1) {
        const other = p === 'chatgpt' ? 'copilot' : 'chatgpt';
        actions.append(button(`Use ${NAME[other]}`, { onClick: () => choose(other) }));
      }
      if (problem) actions.append(el('p', 'agent-error', problem));
      if (offer.some(x => accounts[x]?.remembered === false)) card.append(el('p', 'agent-privacy', 'This computer has no keyring parc can use, so you’ll sign in again each time parc starts.'));
      card.append(actions);
      account.append(card);
      return;
    }
    const line = el('div', 'agent-who');
    if (chat.provider) line.append(el('span', 'agent-who-name', `${NAME[p]}${who(p) ? ` · ${who(p)}` : ''}`));
    else {
      // A new chat: either provider, signed in or not.
      const pick = el('select', 'agent-provider'); pick.setAttribute('aria-label', 'Answer with');
      for (const x of ['chatgpt', 'copilot']) {
        const a = accounts[x];
        pick.append(new Option(a?.signedIn ? `${NAME[x]}${who(x) ? ` · ${who(x)}` : ''}` : `${NAME[x]} (sign in)`, x, false, x === p));
      }
      pick.onchange = () => choose(pick.value);
      line.append(pick);
    }
    const m = models[p];
    if (m?.models?.length) {
      const pick = el('select', 'agent-model'); pick.setAttribute('aria-label', 'Model');
      for (const x of m.models) pick.append(new Option(x.label || x.id, x.id, false, x.id === m.model));
      pick.onchange = async () => {
        try { await api.agentSetModel(pick.value, p); m.model = pick.value; notify(`${NAME[p]} answers with ${pick.selectedOptions[0].textContent} now.`); }
        catch (err) { notify(err); pick.value = m.model; }
      };
      line.append(pick);
    } else if (m?.error) line.append(el('span', 'agent-error', m.error));
    line.append(button('Sign out', { onClick: () => signOut(p) }));
    account.append(line);
  }
  // Signing in to Copilot: the code to type at GitHub, which parc has opened in the browser.
  function codeBox() {
    const box = el('div', 'agent-code');
    if (!code) { box.append(el('span', 'agent-waiting-note', 'Asking GitHub for a code…')); }
    else {
      const value = el('strong', 'agent-code-value', code.code);
      value.setAttribute('aria-label', `Code ${code.code.split('').join(' ')}`);
      box.append(el('span', 'agent-code-text', 'Enter this code on GitHub:'), value,
        button('Copy', { variant: 'outline', glyph: 'copy', onClick: () => { api.copyText(code.code); notify('Copied the code.'); } }),
        button('Open GitHub', { variant: 'outline', onClick: () => api.copilotOpen() }),
        el('span', 'agent-waiting-note', 'Waiting for GitHub…'));
    }
    box.append(button('Cancel', { variant: 'ghost', onClick: () => api.copilotCancelSignIn() }));
    return box;
  }
  async function choose(p) {
    preferred = p; problem = '';
    await api.agentSetProvider(p).catch(() => {});
    await refreshAccount();
  }
  // problem: why the last sign-in failed, shown on the card until the next one.
  async function refreshAccount(why = problem) {
    const s = await api.agentStatus();
    accounts = { chatgpt: s.chatgpt || {}, copilot: s.copilot || {} }; preferred = s.provider || 'chatgpt'; problem = why;
    const p = current();
    if (accounts[p].signedIn && !models[p]) {
      const reply = await api.agentModels(false, p);
      if (reply.signedOut) { const again = await api.agentStatus(); accounts = { chatgpt: again.chatgpt || {}, copilot: again.copilot || {} }; }
      models[p] = reply.error ? (reply.signedOut ? null : { error: reply.error }) : reply;
    }
    paint();
  }
  async function signIn() {
    accounts.chatgpt = { ...accounts.chatgpt, signingIn: true }; problem = ''; paint();
    const reply = await api.agentSignIn().catch(err => ({ error: cleanError(err) }));
    models.chatgpt = null;
    if (!reply.error && !chat.provider) await api.agentSetProvider('chatgpt').catch(() => {});
    await refreshAccount(reply.error && !reply.cancelled ? reply.error : '');
    if (accounts.chatgpt.signedIn && !reply.error) { notify(`Signed in to ChatGPT${accounts.chatgpt.email ? ` as ${accounts.chatgpt.email}` : ''}.`); input.focus(); }
  }
  async function signInCopilot() {
    accounts.copilot = { ...accounts.copilot, signingIn: true }; problem = ''; code = null; paint();
    const reply = await api.copilotSignIn().catch(err => ({ error: cleanError(err) }));
    code = null; models.copilot = null;
    if (!reply.error && !chat.provider) await api.agentSetProvider('copilot').catch(() => {});
    await refreshAccount(reply.error && !reply.cancelled ? reply.error : '');
    if (accounts.copilot.signedIn && !reply.error) { notify(`Signed in to GitHub${accounts.copilot.login ? ` as ${accounts.copilot.login}` : ''}.`); input.focus(); }
  }
  const unsubscribeCode = api.onCopilotCode(c => { if (accounts.copilot.signingIn) { code = c; paint(); } });
  async function signOut(p) {
    if (p === 'copilot') await api.copilotSignOut(); else await api.agentSignOut();
    models[p] = null;
    await refreshAccount('');
    notify(`Signed out of ${NAME[p]}. Your chats stay on this computer.`);
  }

  // ── What the agent may see: the chat's vaults (the ones in view, for a new chat), and the fragment used last ──
  function paintContext() {
    const vaults = searched(), gone = vaults.filter(v => !v.open), used = subject();
    const label = chat.vaults ? vaults.map(v => v.name).join(', ') : scope().label;
    where.replaceChildren(icon('layers'), `Searches ${label}`);
    where.title = chat.vaults ? 'A chat keeps searching the vaults it started in.' : 'A new chat searches the vaults in view.';
    where.toggleAttribute('data-gone', gone.length > 0);
    if (gone.length) where.append(` (${gone.map(v => v.name).join(', ')} isn’t open)`);
    about.hidden = !used || !vaults.some(v => v.path === used.vault);
    if (!about.hidden) {
      about.replaceChildren(el('span', 'agent-about-verb', sharing ? 'With' : 'Add'), typeIcon(used.type), el('span', 'agent-about-title', used.title || 'Untitled'));
      about.setAttribute('aria-pressed', String(sharing));
      about.title = sharing ? 'Shared with your next question (its saved version). Select to leave it out.' : 'Not shared. Select to share it with your next question.';
    }
  }
  about.onclick = () => { sharing = !sharing; paintContext(); };

  // ── An empty chat: questions to start from, and the saved chats to go back to ──
  const day = new Intl.DateTimeFormat(undefined, { month: 'short', day: 'numeric' });
  const when = iso => {
    const t = new Date(iso), mins = (Date.now() - t) / 60000;
    if (mins < 1) return 'now';
    if (mins < 60) return `${Math.floor(mins)}m`;
    if (mins < 1440) return `${Math.floor(mins / 60)}h`;
    if (mins < 7 * 1440) return `${Math.floor(mins / 1440)}d`;
    return day.format(t);
  };
  function paintStart() {
    start.replaceChildren();
    start.hidden = !signedIn() || !empty() || !!loading;
    if (start.hidden) return;
    const with_ = share(), ideas = [
      'What’s still open, and what’s overdue?',
      'What did I decide this month, and why?',
      'Which risks have no mitigation yet?',
      with_ ? `What else in my notes relates to “${with_.title || 'this fragment'}”?` : 'What have I been thinking about lately?'
    ];
    start.append(el('h2', 'section-label', 'Try asking'));
    const list = el('div', 'agent-ideas');
    for (const text of ideas) {
      const b = el('button', 'agent-idea', text); b.type = 'button';
      b.onclick = () => { input.value = text; grow(); input.focus(); };
      list.append(b);
    }
    start.append(list);
    const others = saved.filter(c => c.id !== chat.id);
    if (!others.length) return;
    const heading = el('h2', 'section-label', 'Recent chats');
    heading.id = `${chat.id}-recent`;
    // Which vaults and which provider, only where the chats listed differ in them.
    const many = new Set(others.map(c => c.vaults.join('\n'))).size > 1, mixed = new Set(others.map(c => c.provider)).size > 1;
    const chats = el('ul', 'agent-chats'); chats.setAttribute('aria-labelledby', heading.id);
    for (const c of others.slice(0, 30)) {
      const li = el('li', 'agent-chat'), go = el('button', 'agent-chat-open'), del = el('button', 'agent-chat-delete');
      go.type = 'button'; del.type = 'button';
      go.title = `${c.title}\n${c.questions} question${c.questions === 1 ? '' : 's'}, last ${new Date(c.updated).toLocaleString()}`;
      go.append(icon('agent'), el('span', 'agent-chat-title', c.title || 'Untitled chat'));
      const meta = [...(many ? vaultNames(c.vaults).map(v => v.name) : []), ...(mixed ? [c.provider === 'copilot' ? 'Copilot' : 'ChatGPT'] : [])];
      if (meta.length) go.append(el('span', 'agent-chat-vault', meta.join(' · ')));
      go.append(el('span', 'agent-chat-when', when(c.updated)));
      go.onclick = e => open(c.id, clickHow(e));
      del.append(icon('trash')); del.setAttribute('aria-label', `Delete “${c.title}”`); del.title = 'Delete this chat';
      del.onclick = () => remove(c.id, c.title);
      li.append(go, del);
      chats.append(li);
    }
    start.append(heading, chats);
  }
  async function refreshSaved() {
    saved = await api.agentChats().catch(() => saved);
    if (empty()) paintStart();
  }
  const unsubscribeChats = api.onAgentChats(refreshSaved);

  function paint() {
    paintAccount(); paintContext(); paintStart();
    compose.hidden = !signedIn() || !!loading;
    ask.toggleAttribute('disabled', busy); stop.hidden = !busy; another.toggleAttribute('disabled', busy);
    root.toggleAttribute('data-busy', busy);
  }

  // ── A turn: the question, then the answer in parts: text, the steps it took, more text ──
  function question(text, shared) {
    const li = el('li', 'agent-turn'); li.dataset.role = 'user';
    const with_ = el('p', 'agent-shared');
    with_.hidden = !shared;
    if (shared) with_.textContent = `With “${shared}”`;
    li.append(with_, el('p', 'agent-question', text));
    log.append(li);
    return with_;
  }
  function answer() {
    const li = el('li', 'agent-turn'); li.dataset.role = 'agent';
    const parts = el('div', 'agent-parts'), waiting = el('p', 'agent-waiting', 'Thinking…'), foot = el('div', 'agent-foot');
    waiting.hidden = true;
    li.append(parts, waiting, foot);
    log.append(li);
    return { li, parts, waiting, foot, steps: new Map(), current: null };
  }
  function textPart(t) {
    if (t.current?.kind === 'text') return t.current;
    const box = el('div', 'agent-text');
    t.parts.append(box);
    t.current = { kind: 'text', box, text: '', view: markdownPreview(box), frame: 0 };
    texts.push(t.current);
    return t.current;
  }
  function stepsPart(t) {
    if (t.current?.kind === 'steps') return t.current;
    const list = el('ul', 'agent-steps'); list.setAttribute('aria-label', 'What the agent looked at');
    t.parts.append(list);
    t.current = { kind: 'steps', list };
    return t.current;
  }
  // Repaints the streaming text at most once a frame.
  function showText(part) {
    if (part.frame) return;
    part.frame = requestAnimationFrame(() => { part.frame = 0; part.view.show(part.text, { links: links() }); });
  }
  function describeStep(name, args) {
    let a = {};
    try { a = JSON.parse(args || '{}') || {}; } catch { /* shown by name */ }
    const ref = prefix => { const target = prefix && links().resolve(String(prefix).toUpperCase()); return target ? `“${target.title || 'Untitled'}”` : String(prefix || '').slice(0, 10); };
    const where = a.vault ? ` in ${a.vault}` : '';
    if (name === 'search') return { glyph: 'search', label: a.query ? `Searched “${a.query}”${where}` : `Listed recent fragments${where}` };
    if (name === 'get_fragment') return { glyph: 'note', label: `Read ${ref(a.id)}${where}` };
    if (name === 'recall') return { glyph: 'link', label: a.id ? `Looked for what relates to ${ref(a.id)}${where}` : `Looked for related fragments${where}` };
    if (name === 'list_types') return { glyph: 'layers', label: `Listed the types${where}` };
    if (name === 'list_tags') return { glyph: 'tag', label: `Listed the tags${where}` };
    return { glyph: 'agent', label: name };
  }
  // A short verdict for a step: how many it found, or that it failed.
  function verdict(name, ok, text) {
    if (!ok) return 'failed';
    const n = text.match(/^(\d+) fragments?/);
    if (name === 'search' || name === 'recall') return n ? `${n[1]} found` : /^No /.test(text) ? 'none found' : '';
    return '';
  }
  function step(t, { id, name, arguments: args }) {
    const { list } = stepsPart(t), { glyph, label } = describeStep(name, args);
    const li = el('li', 'agent-step'), head = el('button', 'agent-step-head'), meta = el('span', 'agent-step-meta', '…'), body = el('pre', 'agent-step-body');
    head.type = 'button'; head.setAttribute('aria-expanded', 'false'); body.hidden = true;
    head.append(icon(glyph), el('span', 'agent-step-label', label), meta);
    head.onclick = () => { body.hidden = !body.hidden; head.setAttribute('aria-expanded', String(!body.hidden)); };
    li.dataset.state = 'running';
    li.append(head, body); list.append(li);
    const s = { li, meta, body, name };
    t.steps.set(id, s);
    return s;
  }
  function stepDone(s, ok, text) {
    s.li.dataset.state = ok ? 'done' : 'failed';
    s.meta.textContent = verdict(s.name, ok, text);
    s.body.textContent = text;
  }

  function onEvent(e) {
    if (e.chat !== chat.id || !turn) return;
    turn.waiting.hidden = true;
    if (e.type === 'text') { const part = textPart(turn); part.text += e.delta; showText(part); return; }
    if (e.type === 'shared') { turn.shared.hidden = false; turn.shared.textContent = `With “${e.title || 'Untitled'}”`; return; }
    if (e.type === 'tool') { step(turn, e); turn.waiting.hidden = false; turn.waiting.textContent = 'Looking…'; return; }
    if (e.type === 'tool-done') {
      const s = turn.steps.get(e.id);
      if (s) stepDone(s, e.ok, e.text);
      turn.waiting.hidden = false;
      turn.waiting.textContent = [...turn.steps.values()].some(x => x.li.dataset.state === 'running') ? 'Looking…' : 'Thinking…';
      return;
    }
    if (e.type === 'note') turn.foot.append(el('p', 'agent-note', e.text));
  }
  const unsubscribe = api.onAgentEvent(onEvent);

  async function send(text) {
    text = text.trim();
    if (!text || busy || !signedIn() || loading) return;
    const vaults = searched();
    if (!vaults.length) { notify('Open a vault first.'); return; }
    if (vaults.some(v => !v.open)) { notify('This chat searches a vault that isn’t open in parc. Open it to go on, or start a new chat.'); return; }
    busy = true; last = text;
    const shared = share(), mark = question(text, null);
    turn = answer();
    turn.shared = mark; turn.li.setAttribute('aria-busy', 'true'); turn.waiting.hidden = false;
    input.value = ''; grow(); paint();
    turn.li.scrollIntoView({ block: 'nearest' });
    const t = turn;
    let reply;
    try { reply = await api.agentAsk(chat.id, { text, vaults: vaults.map(v => v.path), subject: shared ? { vault: shared.vault, id: shared.id } : null, provider: current() }); }
    catch (err) { reply = { error: cleanError(err) }; }
    busy = false; turn = null;
    t.li.removeAttribute('aria-busy'); t.waiting.hidden = true;
    // The final text, painted now rather than on the next frame.
    if (t.current?.kind === 'text') { cancelAnimationFrame(t.current.frame); t.current.frame = 0; t.current.view.show(t.current.text, { links: links() }); }
    if (reply?.error) {
      t.li.dataset.failed = '';
      t.foot.append(el('p', 'agent-error', reply.cancelled ? 'Stopped. This question isn’t part of the chat.' : reply.error));
      if (!reply.cancelled && !reply.signedOut) t.foot.append(button('Try again', { variant: 'outline', onClick: () => { t.li.previousElementSibling?.remove(); t.li.remove(); send(text); } }));
      if (reply.signedOut) { models[current()] = null; await refreshAccount(); }
    } else {
      // The answer as saved, in place of what streamed: a late event can't leave it short.
      if (reply.chat.turn?.parts) fill(t, reply.chat.turn.parts);
      if (!t.parts.querySelector('.agent-text')) t.foot.append(el('p', 'agent-note', 'The agent gave no answer.'));
      const before = JSON.stringify([chat.title, chat.vaults]);
      chat = { ...chat, provider: reply.chat.provider, title: reply.chat.title, vaults: reply.chat.vaults };
      if (JSON.stringify([chat.title, chat.vaults]) !== before) changed();
    }
    paint();
    if (document.activeElement === document.body || !document.activeElement) input.focus({ preventScroll: true });
  }

  // A saved chat as it was: each question, its steps and its answer.
  async function load(id) {
    const shown = await api.agentChat(id).catch(() => null);
    loading = null;
    if (!shown) { notify('That chat was deleted.'); chat = { id: newId(), provider: null, title: '', vaults: null }; changed(); paint(); return; }
    clear();
    chat = { id: shown.id, provider: shown.provider, title: shown.title, vaults: shown.vaults };
    for (const entry of shown.turns) {
      if (entry.role === 'user') { question(entry.text, entry.shared); last = entry.text; continue; }
      const t = answer();
      fill(t, entry.parts);
      if (!entry.parts.some(p => p.kind === 'text')) t.foot.append(el('p', 'agent-note', 'The agent gave no answer.'));
    }
    changed(); paint();
    log.lastElementChild?.scrollIntoView({ block: 'end' });
  }
  function clear() { log.replaceChildren(); texts.length = 0; turn = null; last = ''; }
  // An answer's parts as saved: its steps and its text, drawn afresh.
  function fill(t, parts) {
    t.parts.replaceChildren(); t.steps.clear(); t.current = null;
    for (const part of parts) {
      if (part.kind === 'steps') { for (const s of part.steps) stepDone(step(t, s), s.ok, s.text); }
      else { const p = textPart(t); p.text = part.text; p.view.show(p.text, { links: links() }); }
    }
    for (let i = texts.length - 1; i >= 0; i--) if (!texts[i].box.isConnected) texts.splice(i, 1);
  }

  // The box grows with the question, up to a third of the window.
  function grow() { input.style.height = 'auto'; input.style.height = `${Math.min(input.scrollHeight, Math.round(window.innerHeight / 3))}px`; }
  input.addEventListener('input', grow);
  input.addEventListener('keydown', e => {
    if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) { e.preventDefault(); send(input.value); }
    else if (e.key === 'ArrowUp' && !input.value && last) { e.preventDefault(); input.value = last; grow(); }
  });
  compose.addEventListener('submit', e => { e.preventDefault(); send(input.value); });

  return {
    /** Shown, or the scope changed: account, saved chats and citations, drawn again. */
    async render() {
      await Promise.all([loading, refreshSaved()]);
      await refreshAccount().catch(err => { problem = cleanError(err); paint(); });
      this.relink();
      if (wantFocus && signedIn() && root.isConnected) input.focus();
      wantFocus = false;
    },
    /** Shown again: the fragment used last may have changed. */
    shown() { paintContext(); paintStart(); },
    /** Citations resolve again: the fragments in view changed. */
    relink() { for (const part of texts) if (!part.frame) part.view.show(part.text, { links: links() }); },
    /** Puts the caret in the question box; true when it could (signed in), else after the next render. */
    focus() { if (signedIn() && !loading) { input.focus(); return true; } wantFocus = true; return false; },
    /** Shows a saved chat here, in place of this empty one. */
    load(id) { if (busy) return; loading = load(id); paint(); return loading; },
    id: () => chat.id,
    /** Saved: has a title and keeps its vaults. */
    saved: () => !!chat.vaults,
    title: () => chat.title,
    vaults: () => chat.vaults || [],
    empty: () => empty() && !loading && !busy,
    destroy() { unsubscribe(); unsubscribeChats(); unsubscribeCode(); if (busy) api.agentStop(chat.id); api.agentForget(chat.id).catch(() => {}); }
  };
}
