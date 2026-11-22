// Completes parc's search language at the caret: a filter's name, then its values (type:, status:, #tag…).
// ↑↓ choose, Tab accepts (Enter too, for a value: on a bare word Enter still searches), Esc closes. Values come from the vault's schemas and tags.
export const FILTERS = ['type', 'status', 'priority', 'tag', 'due', 'created', 'updated', 'has', 'is', 'by', 'linked'];

function el(tag, className = '', text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text != null) node.textContent = text;
  return node;
}

/** host: an aihio-input. values(filter): the values to offer for `filter:` (or 'tag' for #). */
export function searchAssist(host, { id, values, describe = () => '' }) {
  const list = el('ul', 'assist'), field = () => host.querySelector('input');
  list.id = id; list.setAttribute('role', 'listbox'); list.setAttribute('aria-label', 'Suggestions'); list.hidden = true;
  host.after(list);
  let items = [], active = -1, span = null;

  function token(input) {
    const end = input.selectionStart ?? input.value.length, start = input.value.lastIndexOf(' ', end - 1) + 1;
    return { start, end, text: input.value.slice(start, end) };
  }
  function suggest() {
    const input = field();
    if (!input || document.activeElement !== input || input.selectionStart !== input.selectionEnd || host.closest('[hidden]')) return hide();
    const t = token(input), text = t.text.toLowerCase();
    let next = [];
    const pair = text.match(/^([a-z]+):(!?)(.*)$/);
    if (text.startsWith('#')) next = values('tag').filter(v => v.toLowerCase().startsWith(text.slice(1))).map(v => ({ insert: `#${v}`, label: `#${v}` }));
    else if (pair && FILTERS.includes(pair[1])) next = values(pair[1]).filter(v => v.toLowerCase().startsWith(pair[3]) && v.toLowerCase() !== pair[3])
      .map(v => ({ insert: `${pair[1]}:${pair[2]}${v}`, label: v }));
    else if (/^[a-z]+$/.test(text)) next = FILTERS.filter(f => f.startsWith(text)).map(f => ({ insert: `${f}:`, label: `${f}:`, hint: describe(f), name: true }));
    items = next.slice(0, 8); span = t;
    active = items.length ? 0 : -1;
    render();
  }
  function render() {
    const input = field();
    input?.setAttribute('aria-autocomplete', 'list'); input?.setAttribute('aria-controls', list.id);
    list.replaceChildren(...items.map((item, i) => {
      const li = el('li', 'assist-item'); li.id = `${id}-${i}`; li.setAttribute('role', 'option'); li.setAttribute('aria-selected', String(i === active));
      li.append(el('code', '', item.label));
      if (item.hint) li.append(el('span', 'assist-hint', item.hint));
      li.onpointerdown = e => { e.preventDefault(); accept(i); };
      return li;
    }));
    list.hidden = !items.length;
    input?.setAttribute('aria-expanded', String(!list.hidden));
    if (active >= 0) input?.setAttribute('aria-activedescendant', `${id}-${active}`); else input?.removeAttribute('aria-activedescendant');
  }
  function hide() { items = []; active = -1; render(); }
  function accept(i) {
    const input = field(), item = items[i];
    if (!input || !item || !span) return;
    const before = input.value.slice(0, span.start), after = input.value.slice(span.end).replace(/^\S*/, '');
    const glue = item.insert.endsWith(':') ? '' : ' ';
    host.value = `${before}${item.insert}${glue}${after.trimStart()}`;
    const caret = before.length + item.insert.length + glue.length;
    input.setSelectionRange(caret, caret);
    host.dispatchEvent(new Event('input', { bubbles: true }));
    suggest(); // after "status:" go straight on to its values
  }
  host.addEventListener('input', () => requestAnimationFrame(suggest));
  host.addEventListener('focusin', () => requestAnimationFrame(suggest));
  host.addEventListener('focusout', () => setTimeout(hide));
  host.addEventListener('click', () => requestAnimationFrame(suggest));
  host.addEventListener('keydown', e => {
    if (list.hidden) return;
    const step = { ArrowDown: 1, ArrowUp: -1 }[e.key];
    if (step) { e.preventDefault(); active = (active + step + items.length) % items.length; render(); }
    else if ((e.key === 'Tab' || (e.key === 'Enter' && !items[active]?.name)) && active >= 0) { e.preventDefault(); e.stopPropagation(); accept(active); }
    else if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); hide(); }
  }, true);
  return { hide };
}
