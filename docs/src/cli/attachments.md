---
layout: layouts/doc.njk
title: Attachment commands
eyebrow: CLI · §05
---

Attach binary files to a fragment. They live under `<vault>/attachments/<fragment-id>/` and can be referenced in the body via `![[attach:filename]]`.

## attach

```bash
parc attach <id> <file> [--mv]
```

Copies `<file>` into `<vault>/attachments/<id>/`. Use `--mv` to move it instead — useful when the file was just generated and you don't need it anywhere else.

```bash
parc attach 01JQ7V auth-flow.png
parc attach 01JQ7V ~/Downloads/postmortem.pdf --mv
```

If a file with the same name already exists in the fragment's attachment folder, parc errors out unless you pass `--force` to overwrite.

## attachments

```bash
parc attachments <id> [--json]
```

Lists every file attached to a fragment, with size and modification time.

```bash
parc attachments 01JQ7V
```

```text
auth-flow.png      82 KB    2026-02-21
postmortem.pdf    1.4 MB    2026-02-22
```

## detach

```bash
parc detach <id> <filename> [--force]
```

Removes an attachment. Without `--force`, parc prompts for confirmation. Detaching is destructive — there is no trash for attachments. If you want to keep the file, copy it out of the vault first.

```bash
parc detach 01JQ7V postmortem.pdf
```

## Referencing in body content

Inside the same fragment's body:

```markdown
The proposal flowchart:

![[attach:auth-flow.png]]

Full PDF: [[attach:postmortem.pdf|the post-mortem]]
```

`![[attach:...]]` produces an image; `[[attach:...]]` produces a link. The path is resolved against the *current* fragment's attachment folder — you can't reference another fragment's attachments this way (use a regular link to that fragment instead).
