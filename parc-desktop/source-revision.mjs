// Saving source notes writes title + body. If only the title moved on disk
// (e.g. extraction just completed) and the author only edited the notes,
// the two edits are disjoint. Never guess the base of a restored/stale draft.
export function canAdoptSourceTitle(base, loadedRevision, localTitle, latest) {
  return base?.type === 'source' && latest?.type === 'source' && latest.id === base.id
    && loadedRevision === base.revision && !!latest.revision && latest.revision !== base.revision
    && localTitle === base.title && latest.title !== base.title && latest.body === base.body;
}
