# External-reference YAML parsing through `openapi-typescript` / `@redocly/openapi-core`

Bounded, harmless proof that the dependency chain `ui/scripts/gen-api.mjs`
→ `openapi-typescript` → `@redocly/openapi-core` really does parse YAML
with merge-key (`<<:`) support — not through `gen-api.mjs`'s own JSON
parsing of its root document (that part of the previous revision's
rejection was correct), but through `@redocly/openapi-core`'s own
**external `$ref` resolution**, which runs regardless of whether the root
document was JSON or YAML.

This does **not** reproduce `GHSA-2883-xcg3-v3hh`'s specific
CPU-exhaustion behavior (an empty merge source that `maxTotalMergeKeys`
fails to bound) — it proves the merge-key YAML parser is reached at all,
nothing more. No resource-exhaustion payload is included or was run.

## What it proves

A plain JavaScript OpenAPI object (the same shape `gen-api.mjs` builds from
`JSON.parse`/`res.json()`) with `components.schemas.Proof.$ref` pointing at
a local YAML file's JSON-pointer fragment (`./external.yaml#/schema`). The
script runs it through `openapiTS()` — the exact function `gen-api.mjs:97`
calls — twice, each time with a fresh YAML file:

- **With the merge.** `base: &base` holds `type: string` and
  `description: from_yaml_merge`; `schema` is only `<<: *base`. Neither
  field is written under `schema` itself. The generated type is
  `schema: string` with the `@description from_yaml_merge` comment — both
  inherited solely through the merge key.
- **Control, without the merge.** The same `base`, but `schema: {}`. The
  generated type is `schema: unknown` with no description.

The script asserts both: the merged run carries `type: string` and the
description, and the control carries neither. The control is what makes
the assertion mean something — a marker written directly under `schema`
would appear with or without merge-key processing. Together they prove
`@redocly/openapi-core`'s YAML parser ran on the external file with
merge-key support enabled
(`node_modules/@redocly/openapi-core/lib/js-yaml/index.js`:
`DEFAULT_SCHEMA_WITHOUT_TIMESTAMP` extends `JSON_SCHEMA` with
`implicit: [js_yaml_1.types.merge]`).

It does not exercise `brace-expansion`. `@redocly/openapi-core` calls
`minimatch` (and so `brace-expansion`) only in `lib/utils.js:106`, matching
a remote URL against the `header.matches` patterns of a configured Redocly
resolve header (`lib/utils.js:85-88`). The pattern comes from Redocly
configuration, not from YAML content, and `openapi-typescript`'s own
`createConfig` call sets no headers. Nothing in this reproduction reaches
it.

## Prerequisite this narrows the finding to

`gen-api.mjs`'s default target is the daemon's own self-generated
OpenAPI document (`GET /api-docs/openapi.json`), which is a single,
self-contained JSON document utoipa produces — it has no external `$ref`s
today, so this path is not live against the default developer workflow.
It becomes live only if:

- a developer runs `npm run gen:api -- <url-or-path>` against a target
  whose OpenAPI document (or anything the daemon's/a target's document
  `$ref`s) is not self-contained and points at an external file the
  developer does not control or has not reviewed, or
- the daemon's own generated document ever gains an external `$ref` (not
  the case today).

## Running it

```
node run.mjs
```

Requires `ui/node_modules` to already be installed (`npm ci` in `ui/`).
Writes nothing outside its own temp directory; deletes that directory on
exit. No network access, no CPU-exhaustion payload.
