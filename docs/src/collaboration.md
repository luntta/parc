---
layout: layouts/doc.njk
title: Collaboration
eyebrow: Reference · §08
---

parc is single-user by design — no accounts, no sync service, no merge engine. But vaults are git-friendly, so a shared vault in a shared repository works well as long as you're comfortable resolving the occasional conflict by hand.

## Why it works

Three properties of parc's vault layout make git transport viable:

1. **One file per fragment.** Concurrent fragment creation never conflicts at the file level — each new fragment lands at a fresh ULID-named path under `fragments/`.
2. **The index is derivable.** `index.db` is git-ignored. Pulling someone else's changes doesn't replace your index; you just rebuild it from the current files.
3. **History is per-fragment, not per-vault.** Snapshots live under `history/<id>/` so two collaborators editing different fragments don't touch each other's history files.

## Setup

Create the vault inside the repository like any other directory:

```bash
cd ~/work/api
parc init
git add .parc
git commit -m "Add parc vault"
```

`parc init` writes a `.gitignore` inside `.parc/` that excludes `index.db`, `trash/`, and `server.sock` automatically.

## After a pull

Nothing to run. parc keeps its index in step with the files, so the first command after a pull sees whatever your collaborators landed:

```bash
git pull origin main
parc list todo                 # results include collaborators' new todos
```

`parc git-hooks install` adds a `post-merge` hook that runs `parc reindex` after each pull. It is no longer needed; the hook only moves the indexing work from your next command to the pull itself.

## Conflict scenarios

### New fragments from a collaborator

No conflict — different ULID, different filename. `git pull` brings the file in, and the next parc command adds it to your index.

### Same fragment edited by both of you

A textual merge conflict in `fragments/<id>.md`. Resolve it with whatever you'd use for any other markdown conflict — git's mergetool, the built-in markers, the GUI of your choice. Then:

```bash
git add fragments/<id>.md
git commit
```

While the conflict markers are in the file, parc can't read it as a fragment: it drops out of the index and the CLI warns about it once. Once resolved, it's picked up again. If the resolution leaves the fragment invalid against its schema, `parc doctor` will report it.

Sync tools such as Syncthing or Dropbox keep both versions instead, as a copy named like `<id>.sync-conflict-….md`. The copy carries the original's `id`, so parc doesn't index it (it would shadow the original) and reports it instead. Merge what you need into the original and delete the copy.

### Deleted on one side, edited on the other

git treats this as a "modify/delete" conflict. parc has no opinion — it's the same conflict you'd get with any text file. Decide whether the fragment lives or dies, then accept one side or the other.

### History snapshot collisions

Vanishingly rare in practice. Snapshots are timestamped to the second, so two collaborators editing the same fragment in the same second would write to the same snapshot path. If it happens, git will treat it as a regular file conflict — pick whichever snapshot wins, the other person's local edit history is unaffected.

## What parc does not do

- **No three-way merge for fragments.** parc doesn't try to merge frontmatter or body across edits — git does the textual merge, you resolve.
- **No remote sync server.** There is no `parc sync` or `parc push`. The vault is just a directory; how it gets to other machines is your problem (rsync, git, Syncthing, Dropbox, NFS — all of these work).
- **No live multi-user editing.** Two people opening the same fragment in `parc edit` will produce a conflict on save. Use git's locking conventions, an external coordinator, or just talk to each other.

## Vault-as-archive workflows

A different way to use git: keep fragments in a separate "vault" repo from your code, push it to a private remote, and clone it on every machine you work from. parc treats this exactly the same as a project-local vault — `cd` into the clone and you're in.

```bash
git clone git@github.com:alice/parc-vault.git ~/.parc
cd ~/notes
parc list                     # uses ~/.parc
```

This pattern gives you a single global vault that follows you across machines, with git as the transport and nothing extra to remember.
