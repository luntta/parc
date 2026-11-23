---
layout: layouts/doc.njk
title: Custom types
eyebrow: Reference · §04
---

A type is a YAML schema in `<vault>/schemas/`. The five built-in types are schemas too, copied into each vault by `parc init`, so everything here applies to changing them as well as to adding your own. parc validates fragments against their type's schema on every save, and the search DSL knows every registered type.

The desktop app's **Vault settings** edit these files with a form: add a type, add or remove fields, rename a status (fragments holding it follow), and say what each status means. The rest of this page is the file format behind it.

## Schema format

```yaml
# .parc/schemas/bug.yml
name: bug
alias: b
editor_skip: false
fields:
  - name: status
    type: enum
    values: [new, triaged, fixing, fixed, wontfix]
    required: true
    default: new
    toggle: [new, fixed]
    categories:
      active: [triaged, fixing]
      done: [fixed]
      closed: [wontfix]
  - name: severity
    type: enum
    values: [low, medium, high]
    default: medium
  - name: found_in
    type: string
  - name: due
    type: date
  - name: reporters
    type: list
```

| Key | Meaning |
|-----|---------|
| `name` | The type's name: a letter, then letters, digits, `-` or `_`. Fragments store it in `type:`, so it shouldn't change once fragments use it |
| `alias` | Optional shortcut: `parc b "Crash on save"` creates a bug, and `b` works wherever a type name does |
| `editor_skip` | `true` skips `$EDITOR` when `parc new` is given a title (notes do this) |
| `fields` | A list of fields, in the order they're shown |

## Fields

Each field is `name`, `type`, and optionally `values`, `required`, `default`, `toggle` and `categories`.

| `type` | Holds | Notes |
|--------|-------|-------|
| `string` | One line of text | `default` is any line |
| `date` | `YYYY-MM-DD` | `default` must be a date; `parc new --due` also takes shorthands such as `tomorrow` |
| `enum` | One of `values:` | Values are single words (letters, digits, `-`, `_`), so the DSL can say `status:wontfix` |
| `list` | A list of strings | No default |

- `required: true` refuses a save without the field, unless it has a `default`, which new fragments get.
- A field name can't be one parc uses itself: `id`, `type`, `title`, `tags`, `links`, `attachments`, `created_at`, `updated_at`, `created_by`, `archived`, `body`, and a few names the JSON views use.

The search DSL filters `status:`, `priority:` and `due:` directly. Other fields are stored in the files and shown everywhere, but aren't filters yet.

## What each status means

The `status` field can say what each of its values means with `categories:`, listing the values that are `active` (under way), `done` (finished) or `closed` (dropped). A value listed nowhere is `open`. Done and closed fragments are settled: they leave open-work lists, `due`, `stale` and `review`, and the desktop's **Open** filter. See [What a status means]({{ '/concepts/types/' | url }}#what-a-status-means).

Each value can be in one category at most, and `categories:` is only for the field named `status`. A schema without it gets the built-in meanings of the familiar words (`done`, `cancelled`, `in-progress`…) and treats any other word as open. Give your own words a category.

## One-key toggle

An `enum` field can declare `toggle: [off, on]`, the two values the TUI's `t` key and the desktop's status mark flip between. On a fragment whose value is `on`, the toggle sets `off`; for any other value it sets `on`, so it finishes an in-progress todo rather than reopening it.

Both values must be in `values:`, and the first field that declares `toggle:` wins. The built-in todo, risk and decision types ship with one; idea and note don't. A schema without `toggle:` whose `status` enum has both `open` and `done` toggles between those two, so todo schemas created before `toggle:` existed keep working.

## Templates

The body a new fragment starts with in `$EDITOR` comes from `<vault>/templates/<type>.md`. A new type gets an empty one; edit it to give the type a standard structure:

```markdown
---
type: bug
title: ""
---

## Steps to reproduce

1.

## Expected

## Actual
```

## Adding a type

Write the schema and register it:

```bash
parc schema add ./bug.yml
```

`schema add` checks the file, copies it to `<vault>/schemas/bug.yml`, and creates `templates/bug.md` if there isn't one. After that:

```bash
parc new bug "Crash on save" --status triaged
parc b "Toolbar overlaps the title"
parc list bug
parc search 'type:bug status:!fixed'
```

A file you drop into `schemas/` yourself works too; parc reads every `.yml` and `.yaml` file there.

## Changing a type

Edit its file in `schemas/`. The change applies to the next save of each fragment, so think about the fragments it already has:

- **Renaming or removing a value** leaves fragments holding the old one invalid until they're changed. The desktop's Vault settings move them for you (renamed values follow; a removed value asks where its fragments go) and won't save a change that would leave a fragment invalid. Over JSON-RPC, `schema.save` does the same with `renames`.
- **Adding a required field** needs a `default`, or every existing fragment becomes invalid.
- **Removing a field** keeps its values in the fragments' files; parc stops showing and checking them.

`parc doctor` lists fragments that don't match their schema.

## Turning a type off, or removing it

A type you no longer want to create can be turned off (`types.disabled` in [`config.yml`]({{ '/configuration/' | url }}#types), or a switch in Vault settings). Its fragments stay readable and editable.

A type nothing uses can be removed: delete its file from `schemas/` (Vault settings offer **Delete type** when no fragment uses it). Fragments of a type without a schema are still read and saved, just not validated.
