import { canAdoptSourceTitle } from './source-revision.mjs';

// The editor owns this snapshot; attachment/field responses update metadata, never
// the text's baseline. A stale restored draft may know its revision but not its text.
export function textBase(note, revision = note?.revision) {
  if (!note) return null;
  return revision && revision === note.revision
    ? { id: note.id, type: note.type, revision, title: note.title, body: note.body }
    : { id: note.id, revision };
}

export const sameBaseText = (base, latest) => !!base && base.id === latest.id && Object.hasOwn(base, 'body')
  && base.body === latest.body && base.title === latest.title;

// Every update, even an explicitly confirmed overwrite, compares with the revision
// the decision was made about. Never retry a rejected/ambiguous transport write.
export async function saveDocumentText(api, vault, base, title, body) {
  const write = (expected, chosenTitle) => api.save(vault, { id: base.id, title: chosenTitle, body, expected });
  let result = base.revision ? await write(base.revision, title) : { conflict: true };
  while (result.conflict) {
    const latest = await api.note(vault, base.id);
    if (!latest.revision) throw new Error('Cannot verify this fragment’s revision.');
    const sameText = sameBaseText(base, latest);
    const adopt = canAdoptSourceTitle(base, base.revision, title, latest);
    if (!sameText && !adopt) {
      const choice = await api.conflict({ title: title || latest.title });
      if (choice !== 'overwrite') return { choice };
    }
    result = await write(latest.revision, adopt ? latest.title : title);
  }
  return { note: result };
}
