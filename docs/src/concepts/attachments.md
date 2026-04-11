---
layout: layouts/doc.njk
title: Attachments
eyebrow: Concepts · §05
---

Fragments can carry binary files — screenshots, PDFs, diagrams, anything you'd otherwise paste into a wiki.

## Storage

Attachments live under `<vault>/attachments/<fragment-id>/`, one folder per fragment. Files keep their original names.

```
.parc/attachments/
└── 01JQ7V3XKP5GQZ2N8R6T1WBMVH/
    ├── auth-flow.png
    └── postmortem.pdf
```

There is no central blob store, no de-duplication, no rename — what you put in is what's there.

## Adding attachments

```bash
parc attach 01JQ7V auth-flow.png             # copy into the vault
parc attach 01JQ7V auth-flow.png --mv        # move into the vault
```

`--mv` is useful when you've just generated the file and don't need it anywhere else.

## Referencing them in body content

Use the `attach:` scheme inside a wiki-link from the fragment's body:

```markdown
The deuteranope view of the proposal:

![[attach:auth-flow.png]]

Full PDF: [[attach:postmortem.pdf|the post-mortem]]
```

parc resolves these against `<vault>/attachments/<this-fragment-id>/` at render time.

## Listing and removing

```bash
parc attachments 01JQ7V                # list a fragment's attachments
parc detach 01JQ7V auth-flow.png       # remove an attachment
```

`parc detach` deletes the file. If you want to keep it but unlink it from the fragment, just `mv` it out of the attachments folder by hand — parc treats the directory as the source of truth.

## Search

Find fragments that have any attachment:

```bash
parc search 'has:attachments'
parc search 'has:attachments type:decision'
```
