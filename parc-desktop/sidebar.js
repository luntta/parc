// Sidebar list renderer: direct DOM for small vaults (<= VIRTUAL_THRESHOLD rows) and windowed virtual
// rendering for large vaults (up to 10,000+ fragments), preserving sticky group headers, exact scrollbar
// geometry, keyboard navigation, and O(visible) tab-selection updates without rebuilding the list on tab switches.
const VIRTUAL_THRESHOLD = 200;
const OVERSCAN = 12;

export function sidebarList(host, hooks) {
  let mode = 'none';       // 'none' | 'grouped'
  let virtual = false;
  let flat = [];           // ungrouped rows
  let sections = [];       // grouped sections: { g, open, li, rowsEl, top, rowsTop, count, range }
  let options = {};
  let rowH = 28, gapH = 8;
  let raf = 0, revealed = null, flatRange = null;

  function measure() {
    const cs = getComputedStyle(host);
    const rem = parseFloat(getComputedStyle(document.documentElement).fontSize) || 16;
    const raw = cs.getPropertyValue('--side-line').trim();
    if (raw.endsWith('rem')) rowH = parseFloat(raw) * rem || 28;
    else if (raw.endsWith('px')) rowH = parseFloat(raw) || 28;
    else rowH = 28;
    gapH = Math.round(rem * 0.5);
  }

  function clearPadding(el) { el.style.paddingTop = ''; el.style.paddingBottom = ''; }

  function renderFlatWindow(force = false) {
    if (!virtual || mode !== 'none') return;
    const total = flat.length;
    const viewH = host.clientHeight || 600, top = host.scrollTop;
    const start = Math.max(0, Math.floor(top / rowH) - OVERSCAN);
    const end = Math.min(total, Math.ceil((top + viewH) / rowH) + OVERSCAN);
    if (!force && flatRange && flatRange[0] === start && flatRange[1] === end) return;
    flatRange = [start, end];
    host.style.paddingTop = `${start * rowH}px`;
    host.style.paddingBottom = `${(total - end) * rowH}px`;
    const frag = document.createDocumentFragment();
    for (let i = start; i < end; i++) {
      const li = hooks.item(flat[i], options);
      li.firstElementChild.dataset.vidx = String(i);
      frag.append(li);
    }
    host.replaceChildren(frag);
  }

  function renderGroupedWindow(force = false) {
    if (!virtual || mode !== 'grouped') return;
    const viewH = host.clientHeight || 600;
    const winTop = host.scrollTop - OVERSCAN * rowH;
    const winBottom = host.scrollTop + viewH + OVERSCAN * rowH;
    for (const s of sections) {
      if (!s.open || !s.rowsEl) continue;
      const count = s.g.items.length;
      const rowsBottom = s.rowsTop + count * rowH;
      let start = 0, end = 0;
      if (rowsBottom >= winTop && s.rowsTop <= winBottom) {
        start = Math.max(0, Math.floor((winTop - s.rowsTop) / rowH));
        end = Math.min(count, Math.ceil((winBottom - s.rowsTop) / rowH));
      }
      if (!force && s.range && s.range[0] === start && s.range[1] === end) continue;
      s.range = [start, end];
      s.rowsEl.style.paddingTop = `${start * rowH}px`;
      s.rowsEl.style.paddingBottom = `${(count - end) * rowH}px`;
      const frag = document.createDocumentFragment();
      for (let i = start; i < end; i++) {
        const li = hooks.item(s.g.items[i], options);
        li.firstElementChild.dataset.vidx = String(s.baseIdx + 1 + i);
        frag.append(li);
      }
      s.rowsEl.replaceChildren(frag);
    }
  }

  function renderWindow(force = false) {
    raf = 0;
    if (!virtual) return;
    if (mode === 'none') renderFlatWindow(force);
    else renderGroupedWindow(force);
  }

  host.addEventListener('scroll', () => {
    if (virtual && !raf) raf = requestAnimationFrame(() => renderWindow(false));
  }, { passive: true });

  // Update active/open states on already-mounted DOM rows without rebuilding the list.
  function markSelection({ scroll = true } = {}) {
    const currentKey = hooks.currentKey(), currentType = hooks.currentType();
    for (const b of host.querySelectorAll('.row')) {
      const k = b.dataset.key;
      if (currentKey && k === currentKey) {
        b.setAttribute('aria-current', 'page');
        delete b.dataset.open;
      } else {
        b.removeAttribute('aria-current');
        if (k && hooks.isOpenTab(k)) b.dataset.open = ''; else delete b.dataset.open;
      }
    }
    for (const h of host.querySelectorAll('.group-head[data-type]')) {
      if (currentType && h.dataset.type === currentType) h.setAttribute('aria-current', 'page');
      else h.removeAttribute('aria-current');
    }
    if (!scroll || currentKey === revealed) return;
    revealed = currentKey;
    if (!currentKey) return;
    if (virtual) scrollToKey(currentKey);
    else host.querySelector('.row[aria-current]')?.scrollIntoView({ block: 'nearest' });
  }

  function scrollToOffset(y) {
    const viewH = host.clientHeight || 600, top = host.scrollTop;
    const padTop = mode === 'grouped' ? rowH + 4 : 4;
    if (y < top + padTop) host.scrollTop = Math.max(0, y - padTop);
    else if (y + rowH > top + viewH) host.scrollTop = Math.max(0, y + rowH - viewH + 4);
  }

  function scrollToKey(targetKey) {
    if (!virtual) { host.querySelector(`.row[data-key="${CSS.escape(targetKey)}"]`)?.scrollIntoView({ block: 'nearest' }); return; }
    if (mode === 'none') {
      const i = flat.findIndex(r => hooks.key(r) === targetKey);
      if (i < 0) return;
      scrollToOffset(i * rowH);
      renderFlatWindow(true);
      return;
    }
    for (const s of sections) {
      if (!s.open) continue;
      const i = s.g.items.findIndex(r => hooks.key(r) === targetKey);
      if (i < 0) continue;
      scrollToOffset(s.rowsTop + i * rowH);
      renderGroupedWindow(true);
      return;
    }
  }

  // Render the whole sidebar view (grouped or flat) from scratch.
  function render({ grouped: isGrouped, items, groups, opts }) {
    measure();
    options = opts;
    flatRange = null;
    clearPadding(host);
    if (!isGrouped) {
      mode = 'none'; flat = items; sections = [];
      virtual = items.length > VIRTUAL_THRESHOLD;
      if (!virtual) {
        host.replaceChildren(...items.map(r => hooks.item(r, opts)));
      } else {
        renderFlatWindow(true);
      }
    } else {
      mode = 'grouped'; flat = [];
      const total = groups.reduce((n, g) => n + (hooks.isFolded(g.id) ? 0 : g.items.length), 0);
      virtual = total > VIRTUAL_THRESHOLD;
      sections = [];
      if (!virtual) {
        host.replaceChildren(...groups.map(g => hooks.groupItem(g, opts)));
      } else {
        const frag = document.createDocumentFragment();
        let y = 0, flatIdx = 0;
        for (let gi = 0; gi < groups.length; gi++) {
          const g = groups[gi], open = !hooks.isFolded(g.id);
          if (gi > 0) y += gapH;
          const { li, head, rowsEl } = hooks.groupShell(g, open);
          head.dataset.vidx = String(flatIdx);
          const rowsTop = y + rowH;
          sections.push({ g, open, li, head, rowsEl, top: y, rowsTop, baseIdx: flatIdx, range: null });
          y = rowsTop + (open ? g.items.length * rowH : 0);
          flatIdx += 1 + (open ? g.items.length : 0);
          frag.append(li);
        }
        host.replaceChildren(frag);
        renderGroupedWindow(true);
      }
    }
    markSelection({ scroll: true });
  }

  // Focus item at flat virtual index (headers + unfolded rows), mounting its window first if virtualized.
  function focusIndex(target) {
    if (!virtual) {
      const all = [...host.querySelectorAll('.group-head, .row')];
      if (target < 0) return false;
      all[Math.min(all.length - 1, target)]?.focus();
      return true;
    }
    if (mode === 'none') {
      if (target < 0) return false;
      const idx = Math.min(flat.length - 1, target);
      if (idx < 0) return false;
      scrollToOffset(idx * rowH);
      renderFlatWindow(true);
      host.querySelector(`[data-vidx="${idx}"]`)?.focus();
      return true;
    }
    if (target < 0) return false;
    const total = sections.reduce((n, s) => n + 1 + (s.open ? s.g.items.length : 0), 0);
    if (!total) return false;
    const idx = Math.min(total - 1, target);
    for (const s of sections) {
      if (idx === s.baseIdx) {
        scrollToOffset(s.top);
        renderGroupedWindow(true);
        s.head.focus();
        return true;
      }
      if (s.open && idx > s.baseIdx && idx <= s.baseIdx + s.g.items.length) {
        const off = idx - s.baseIdx - 1;
        scrollToOffset(s.rowsTop + off * rowH);
        renderGroupedWindow(true);
        s.rowsEl.querySelector(`[data-vidx="${idx}"]`)?.focus();
        return true;
      }
    }
    return false;
  }

  function totalEntries() {
    if (!virtual) return host.querySelectorAll('.group-head, .row').length;
    if (mode === 'none') return flat.length;
    return sections.reduce((n, s) => n + 1 + (s.open ? s.g.items.length : 0), 0);
  }

  function indexOf(node) {
    if (virtual) {
      const v = node?.dataset?.vidx;
      return v == null ? -1 : Number(v);
    }
    return [...host.querySelectorAll('.group-head, .row')].indexOf(node);
  }

  function firstRow() {
    if (virtual) {
      if (mode === 'none') { if (!flat.length) return null; scrollToOffset(0); renderFlatWindow(true); return host.querySelector('.row'); }
      for (const s of sections) {
        if (s.open && s.g.items.length) { scrollToOffset(s.rowsTop); renderGroupedWindow(true); return s.rowsEl.querySelector('.row'); }
      }
      return null;
    }
    return host.querySelector('.row');
  }

  function firstGroupRow(groupId) {
    if (!virtual) return host.querySelector(`[data-group="${CSS.escape(groupId)}"] .row`);
    const s = sections.find(x => x.g.id === groupId);
    if (!s || !s.open || !s.g.items.length) return null;
    scrollToOffset(s.rowsTop);
    renderGroupedWindow(true);
    return s.rowsEl.querySelector('.row');
  }

  return { render, markSelection, focusIndex, totalEntries, indexOf, firstRow, firstGroupRow, get isVirtual() { return virtual; } };
}
