// Secondary workspace views (Trash, Vault Health) and Pulse's actionable daily-review sections.
import { icon, typeIcon } from './icons.js';

function el(tag, className = '', text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text != null) node.textContent = text;
  return node;
}
function button(label, { variant = 'outline', size = 'sm', glyph, onClick, title, ariaLabel } = {}) {
  const b = document.createElement('aihio-button');
  b.setAttribute('variant', variant); b.setAttribute('size', size);
  if (glyph) b.append(icon(glyph));
  if (label) b.append(label);
  if (title) b.title = title;
  if (ariaLabel) b.setAttribute('aria-label', ariaLabel);
  if (onClick) b.addEventListener('click', onClick);
  return b;
}

// ── Trash view: list, restore, or permanently purge soft-deleted fragments ──
export async function renderTrashView(v, deps) {
  const { api, paths, scope, name, typeLabel, timeOf, emptyState, status, onRestored, onChanged } = deps;
  const out = v.ui.results, intro = v.ui.intro;
  intro.textContent = `Fragments moved to the trash in ${scope ? name(scope) : 'your opened vaults'}. Restore brings a fragment back with its history.`;
  const groups = await Promise.all(paths.map(async p => (await api.trashList(p)).map(r => ({ ...r, vault: p }))));
  if (!v.group) return;
  const items = groups.flat().sort((a, b) => b.updated_at.localeCompare(a.updated_at));
  out.replaceChildren();
  if (!items.length) {
    out.append(emptyState('Trash is empty', 'Fragments you move to the trash stay here until you restore or permanently delete them.'));
    return;
  }

  const bar = el('div', 'view-bar');
  bar.append(el('span', 'view-count', `${items.length.toLocaleString()} in trash`));
  const vaultsWithTrash = [...new Set(items.map(i => i.vault))];
  bar.append(button('Empty trash', {
    variant: 'outline', glyph: 'trash',
    onClick: async () => {
      let total = 0;
      for (const p of vaultsWithTrash) {
        const res = await api.trashPurge(p, null, '');
        if (res?.purged) total += res.purged_count || 0;
      }
      if (total) { status(`Permanently deleted ${total.toLocaleString()} ${total === 1 ? 'fragment' : 'fragments'} from trash.`); await onChanged(); }
    }
  }));
  out.append(bar);

  const ul = el('ul', 'results trash-list');
  for (const r of items) {
    const li = el('li', 'trash-row');
    const info = el('div', 'trash-info');
    const facts = [(!scope && name(r.vault)), typeLabel(r.type), ...(r.tags || []).map(t => `#${t}`)].filter(Boolean).join(' · ');
    const main = el('div', 'row-main');
    main.append(el('span', 'row-title' + (r.title ? '' : ' untitled'), r.title || 'Untitled'));
    const meta = el('span', 'row-meta');
    meta.append(el('span', 'row-facts', facts), timeOf(r));
    main.append(meta);
    info.append(typeIcon(r.type), main);

    const actions = el('div', 'trash-actions');
    actions.append(
      button('Restore', {
        variant: 'outline', glyph: 'back',
        onClick: async () => {
          const back = await api.untrash(r.vault, r.id);
          await onRestored(r.vault, back.id ?? r.id, back.title || r.title);
        }
      }),
      button('', {
        variant: 'ghost', size: 'icon', glyph: 'trash',
        title: `Delete “${r.title || 'Untitled'}” permanently`,
        ariaLabel: `Delete “${r.title || 'Untitled'}” permanently`,
        onClick: async () => {
          const res = await api.trashPurge(r.vault, r.id, r.title || '');
          if (res?.purged) { status(`Permanently deleted “${r.title || 'Untitled'}”.`); await onChanged(); }
        }
      })
    );
    li.append(info, actions);
    ul.append(li);
  }
  out.append(ul);
}

// ── Vault Health (vault.doctor + vault.reindex) ──
const FINDING_LABELS = {
  broken_link: 'Broken links',
  ambiguous_link: 'Ambiguous links',
  schema_violation: 'Schema violations',
  attachment_mismatch: 'Attachment mismatches',
  unreadable_fragment: 'Unreadable files',
  plugin_issue: 'Plugin issues',
  vault_size_warning: 'Vault size'
};

export async function renderDoctorView(v, deps) {
  const { api, paths, scope, name, openById, clickHow, section, status, onReindexed, rerender } = deps;
  const out = v.ui.results, intro = v.ui.intro;
  intro.textContent = `Checks links, schemas, attachments and files in ${scope ? name(scope) : 'your opened vaults'}.`;
  const reports = await Promise.all(paths.map(async p => ({ vault: p, report: await api.doctor(p) })));
  if (!v.group) return;

  const checked = reports.reduce((n, r) => n + (r.report.fragments_checked || 0), 0);
  const issues = [], orphans = [];
  for (const { vault, report } of reports) {
    for (const f of report.findings || []) {
      const entry = { ...f, vault };
      if (f.type === 'orphan') orphans.push(entry);
      else issues.push(entry);
    }
  }

  out.replaceChildren();
  const bar = el('div', 'view-bar');
  const summary = el('span', 'doctor-summary');
  summary.dataset.healthy = String(!issues.length);
  summary.append(icon(issues.length ? 'alert' : 'check'),
    issues.length
      ? `${issues.length.toLocaleString()} ${issues.length === 1 ? 'issue' : 'issues'} across ${checked.toLocaleString()} ${checked === 1 ? 'fragment' : 'fragments'}`
      : `All ${checked.toLocaleString()} ${checked === 1 ? 'fragment looks' : 'fragments look'} healthy`);
  bar.append(summary);
  const actions = el('div', 'doctor-actions');
  actions.append(
    button('Check again', { variant: 'ghost', onClick: () => rerender() }),
    button('Rebuild index', {
      variant: 'outline', glyph: 'history',
      onClick: async () => {
        let indexed = 0, problems = 0;
        for (const p of paths) {
          const r = await api.reindex(p);
          indexed += r.indexed || 0;
          problems += (r.problems || []).length;
        }
        status(`Rebuilt index (${indexed.toLocaleString()} ${indexed === 1 ? 'fragment' : 'fragments'}${problems ? `, ${problems} unreadable` : ''}).`);
        await onReindexed();
        rerender();
      }
    })
  );
  bar.append(actions);
  out.append(bar);

  if (!issues.length) {
    const ok = el('div', 'empty doctor-ok');
    ok.append(el('strong', '', 'No broken links, schema errors or missing attachments'),
      orphans.length ? `${orphans.length.toLocaleString()} unlinked ${orphans.length === 1 ? 'fragment is' : 'fragments are'} listed below.` : 'Everything in scope checked out cleanly.');
    out.append(ok);
  } else {
    const byType = new Map();
    for (const item of issues) {
      if (!byType.has(item.type)) byType.set(item.type, []);
      byType.get(item.type).push(item);
    }
    for (const [kind, list] of byType) {
      out.append(section(FINDING_LABELS[kind] || kind, `${list.length}`));
      const ul = el('ul', 'results doctor-list');
      for (const f of list) {
        const li = el('li');
        const targetId = f.source_id || f.id || f.fragment_id || null;
        const title = f.source_title || f.title || f.file || f.plugin_name || 'Vault';
        const detail = kind === 'broken_link' ? `Missing link [[${f.target_ref}]]`
          : kind === 'ambiguous_link' ? `Ambiguous [[${f.target_ref}]] matches ${(f.matches || []).length} fragments`
          : kind === 'schema_violation' ? f.message
          : kind === 'attachment_mismatch' ? f.detail
          : kind === 'unreadable_fragment' ? f.reason
          : kind === 'vault_size_warning' ? `Vault size is ${Math.round((f.total_bytes || 0) / 1e6)} MB`
          : f.detail || '';
        const row = targetId ? el('button', 'row doctor-item') : el('div', 'row doctor-item');
        if (targetId) {
          row.type = 'button';
          row.onclick = e => openById(f.vault, targetId, clickHow(e));
        }
        const main = el('span', 'row-main');
        main.append(el('span', 'row-title', title));
        const meta = el('span', 'row-meta');
        const facts = el('span', 'row-facts', [!scope && name(f.vault), detail].filter(Boolean).join(' · '));
        meta.append(facts);
        main.append(meta);
        row.append(icon('alert'), main);
        li.append(row);
        ul.append(li);
      }
      out.append(ul);
    }
  }

  if (orphans.length) {
    const details = el('details', 'doctor-orphans');
    const sum = el('summary', 'section-label', `Unlinked fragments`);
    sum.append(el('small', '', `${orphans.length.toLocaleString()} with no incoming or outgoing links`));
    details.append(sum);
    const ul = el('ul', 'results');
    for (const f of orphans.slice(0, 100)) {
      const li = el('li'), b = el('button', 'row doctor-item'); b.type = 'button';
      const main = el('span', 'row-main');
      main.append(el('span', 'row-title' + (f.title ? '' : ' untitled'), f.title || 'Untitled'));
      if (!scope) main.append(el('span', 'row-meta', name(f.vault)));
      b.append(icon('link'), main);
      b.onclick = e => openById(f.vault, f.id, clickHow(e));
      li.append(b); ul.append(li);
    }
    if (orphans.length > 100) details.append(el('p', 'setting-hint', `Showing the first 100 of ${orphans.length.toLocaleString()} unlinked fragments.`));
    details.append(ul);
    out.append(details);
  }
}

// ── Pulse daily-review sections: Needs attention, Stale open work, From the archive ──
export function pulseDailySections(rows, deps) {
  const { scope, name, settled, overdue, dayLabel, age, section, openRow, clickHow, openTypeView, searchFor, settledValues } = deps;
  const out = [];
  const active = rows.filter(r => !r.archived && !settled(r));
  const today = new Date().toLocaleDateString('sv');
  const nextWeekDate = new Date(); nextWeekDate.setDate(nextWeekDate.getDate() + 7);
  const nextWeek = nextWeekDate.toLocaleDateString('sv');
  const staleDate = new Date(); staleDate.setDate(staleDate.getDate() - 30);
  const staleIso = staleDate.toISOString();
  const staleDay = staleDate.toLocaleDateString('sv');

  // 1. Needs attention: overdue, due in next 7 days, or unassessed risks.
  const attention = [];
  for (const r of active) {
    const due = r.fields?.due;
    if (due && overdue({ ...r, due })) {
      attention.push({ r, rank: 0, badge: `Overdue · ${dayLabel(due)}`, tone: 'alert', sort: due });
    } else if (due && due >= today && due <= nextWeek) {
      attention.push({ r, rank: 1, badge: due === today ? 'Due today' : `Due ${dayLabel(due)}`, tone: 'soon', sort: due });
    } else if (r.type === 'risk' && !(r.fields?.likelihood && r.fields?.impact)) {
      attention.push({ r, rank: 2, badge: 'Risk not assessed', tone: 'muted', sort: r.updated_at });
    }
  }
  attention.sort((a, b) => a.rank - b.rank || a.sort.localeCompare(b.sort));
  if (attention.length) {
    const head = section('Needs attention', `${attention.length.toLocaleString()}`);
    const ul = el('ul', 'pulse-list');
    for (const item of attention.slice(0, 8)) {
      const { r, badge, tone } = item;
      const li = el('li'), b = el('button', 'pulse-row'); b.type = 'button';
      b.append(typeIcon(r.type), el('span', 'pulse-row-title' + (r.title ? '' : ' untitled'), r.title || 'Untitled'));
      if (!scope) b.append(el('span', 'pulse-row-where', name(r.vault)));
      const pill = el('span', 'pulse-pill', badge); pill.dataset.tone = tone;
      b.append(pill);
      b.onclick = e => openRow(r, clickHow(e));
      li.append(b); ul.append(li);
    }
    if (attention.length > 8) {
      const more = el('button', 'pulse-more', `View all ${attention.length} items`); more.type = 'button';
      more.onclick = () => attention[0].rank < 2 ? searchFor(`due:this-week ${settledValues(scope)}`.trim()) : openTypeView('risk');
      head.append(more);
    }
    out.push(head, ul);
  }

  // 2. Stale open work: stateful fragments still open/active, untouched for 30+ days.
  const stale = active.filter(r => r.status && r.updated_at < staleIso)
    .sort((a, b) => a.updated_at.localeCompare(b.updated_at));
  if (stale.length) {
    const head = section('Stale open work', `${stale.length.toLocaleString()} untouched 30+ days`);
    const review = el('button', 'pulse-more', 'Review all'); review.type = 'button';
    review.onclick = () => searchFor(`updated:<${staleDay} ${settledValues(scope)}`.trim());
    head.append(review);
    const ul = el('ul', 'pulse-list');
    for (const r of stale.slice(0, 5)) {
      const li = el('li'), b = el('button', 'pulse-row'); b.type = 'button';
      b.append(typeIcon(r.type), el('span', 'pulse-row-title' + (r.title ? '' : ' untitled'), r.title || 'Untitled'));
      if (!scope) b.append(el('span', 'pulse-row-where', name(r.vault)));
      b.append(el('span', 'pulse-pill', `Edited ${age(r.updated_at)}`));
      b.onclick = e => openRow(r, clickHow(e));
      li.append(b); ul.append(li);
    }
    out.push(head, ul);
  }

  // 3. From the archive: resurface one older non-archived fragment, with a shuffle button.
  const pool = rows.filter(r => !r.archived);
  if (pool.length >= 4) {
    const weekAgo = new Date(); weekAgo.setDate(weekAgo.getDate() - 7);
    const older = pool.filter(r => r.updated_at < weekAgo.toISOString());
    const candidates = older.length ? older : pool.slice(Math.floor(pool.length / 2));
    let pickIdx = Math.floor(Math.random() * candidates.length);
    const head = section('From the archive', older.length ? 'Resurfaced' : 'Older capture');
    const cardWrap = el('div', 'resurface-wrap');
    const renderPick = () => {
      const r = candidates[pickIdx % candidates.length];
      const b = el('button', 'pulse-row resurface-card'); b.type = 'button';
      b.append(typeIcon(r.type), el('span', 'pulse-row-title' + (r.title ? '' : ' untitled'), r.title || 'Untitled'));
      if (r.tags?.length) b.append(el('span', 'pulse-row-where', r.tags.slice(0, 3).map(t => `#${t}`).join(' ')));
      b.append(el('span', 'pulse-pill', age(r.updated_at)));
      b.onclick = e => openRow(r, clickHow(e));
      cardWrap.replaceChildren(b);
    };
    if (candidates.length > 1) {
      const shuffle = el('button', 'pulse-more'); shuffle.type = 'button';
      shuffle.append(icon('shuffle'), 'Another');
      shuffle.onclick = () => { pickIdx = (pickIdx + 1) % candidates.length; renderPick(); };
      head.append(shuffle);
    }
    renderPick();
    out.push(head, cardWrap);
  }

  return out;
}
