# Factory execution label standard

Version: `1.1.0`

This document defines the canonical Factory label catalog: a distributable,
versionable set of GitHub labels containing the `ai` issue trigger and the
`ai/*` labels that represent observed Factory execution state. The catalog is
Factory-specific and independent of the organization-wide label taxonomy.

The canonical manifest is
[`.github/labels.factory-execution.yml`](../.github/labels.factory-execution.yml).
The manifest and this document must agree. The manifest declares the `ai`
trigger and default `ai/` execution-state names; repositories may configure a
different prefix for execution-state labels as described below. The trigger
remains `ai`.

## Catalog

| Label | Color | Description |
| --- | --- | --- |
| `ai` | `0E8A16` | Trigger AI factory agent |
| `ai/queued` | `CFD3D7` | Accepted work awaiting execution, including scheduled retries. |
| `ai/processing` | `1D76DB` | Active execution. |
| `ai/waiting` | `FBCA04` | Waiting for an external condition without human intervention. |
| `ai/blocked` | `B60205` | An unmet prerequisite prevents progress. |
| `ai/needs-input` | `D93F0B` | Human information or a decision is required. |
| `ai/failed` | `B60205` | Unsuccessful terminal outcome with no automatic retry pending. |
| `ai/cancelled` | `6E7781` | Confirmed cancellation. |
| `ai/completed` | `0E8A16` | Successful automated work, independent of merge or issue closure. |

The catalog contains the `ai` trigger and exactly eight execution-state labels.
Each label name is unique, each color is a quoted six-digit hexadecimal value in
the manifest, and each description is GitHub-compatible English of at most 100
characters.

## Semantics

### State distinctions

- **Waiting, blocked, and needs-input** are different conditions. `ai/waiting`
  means execution is waiting for an external condition without human
  intervention, such as a dependency, rate limit, or scheduled retry window.
  `ai/blocked` means an unmet prerequisite prevents progress. `ai/needs-input`
  means a human must provide information or make a decision before execution can
  continue.
- **Failure and retry** are different outcomes. `ai/failed` is an unsuccessful
  terminal outcome with no automatic retry pending. Work that has been accepted
  for another attempt, including a scheduled retry, is `ai/queued`, not
  `ai/failed`.
- **Cancellation and cancellation requests** are different states. `ai/cancelled`
  represents confirmed cancellation. A request to cancel work is not, by itself,
  `ai/cancelled`; the label changes only when cancellation is confirmed.
- **Successful automation and merge or closure** are independent. `ai/completed`
  means automated work completed successfully. It does not imply that a pull
  request merged, that an issue closed, or that human review is complete.

## Prefix configuration

Repositories may configure the execution label namespace prefix in
`factory.json`:

```json
{
  "labels": {
    "prefix": "factory/"
  }
}
```

- **Default.** Omitting `labels.prefix` resolves the existing `ai/*` catalog.
- **Format.** The prefix must be a single lowercase ASCII namespace followed by
  `/`. Valid characters in the namespace are lowercase letters, digits, `-`, and
  `_`. Empty values, invalid characters, nested namespaces, and effective names
  exceeding GitHub's 50-character label limit are rejected.
- **Reserved families.** Reserved families of applicable repository standards
  cannot be selected. Organization core families (`type/`, `status/`, `scope/`,
  `meta/`, `github_actions/`, `release/`) are protected in Standards adopters,
  including `complexity/` when adopted.
- **State identifiers.** State identifiers (`queued`, `processing`, `waiting`,
  `blocked`, `needs-input`, `failed`, `cancelled`, `completed`) are independent
  of label names. No per-state renaming or automatic alternative-prefix
  selection is introduced.
- **Resolution.** The eight canonical state identifiers resolve into effective
  label names using the configured prefix. Setting `factory/` resolves the same
  eight states under `factory/*` without changing colors or meanings.
- **Trigger.** The standalone `ai` label opts an issue into Factory automation;
  it is not an execution state and is not renamed when the execution prefix
  changes.
- **Ownership.** Execution-state ownership is limited to the eight resolved
  names; Factory also owns the standalone `ai` trigger label.
  Case-insensitive collisions with existing incompatible labels are reported
  rather than silently overwritten.
- **Independence.** Different repositories can resolve different prefixes
  independently.
- **Consistency.** Inconsistent effective configuration between components is
  detectable before label mutation.
- **Active configuration.** Invalid configuration is rejected without replacing
  the last valid active configuration. A prefix change is not silently applied
  to an installed catalog or active execution; explicit migration is required.

## Contract

- **Single state.** At most one catalog state applies to an issue at a time.
  Absence of a catalog label means no published automation state.
- **Initial scope.** The initial contract applies to GitHub issues, not linked
  pull requests.
- **Observational, not authoritative.** Labels are observational projections,
  not commands and not authoritative execution state. Manual label changes do
  not authorize execution transitions.
- **Exact ownership.** Factory owns only the exact resolved catalog names for the
  configured prefix. Unrelated labels under the same prefix namespace are not
  reserved and are not managed by this catalog.
- **Case-insensitive collisions.** GitHub label names are case-insensitive.
  Repositories must not define labels whose case-insensitive form collides with a
  resolved catalog name. A collision with an exact resolved catalog name is owned
  by the catalog; a collision with a different name in the same namespace is
  outside catalog ownership but can create ambiguous operator or automation
  behavior and should be avoided.

## Compatibility and use

The catalog is compatible with level-1 extensions as described by the
organization customization standard. It does not change organization standards or
the organization-wide label taxonomy. Repositories may adopt the catalog by
declaring its labels as extensions in `.github/standards.local.yml`, by copying
the manifest, or by another equivalent process that preserves the label names,
colors, and descriptions.

The catalog can also be used independently by external repositories that do not
adopt the Ailura organization standard. Independent use should preserve the
canonical names and semantics so observers can interpret the labels consistently.

## Versioning

The catalog uses semantic versioning (`MAJOR.MINOR.PATCH`).

- **MAJOR**: a breaking semantic or compatibility change, such as renaming or
  removing a label, changing a label meaning incompatibly, or changing ownership
  rules.
- **MINOR**: a backward-compatible addition, such as adding a new catalog label
  or documenting a clarified non-breaking scenario.
- **PATCH**: a backward-compatible editorial fix, such as clarifying text without
  changing meaning.

The manifest header and this document record the same catalog version. A change
to catalog labels or semantics must update both files in the same change.

## Migration

Migration is non-destructive. Adopters should preserve existing issue history and
avoid deleting or repurposing labels that may already be in use.

- **Adding the catalog**: create missing catalog labels with the canonical names,
  colors, and descriptions. Do not remove unrelated `ai/*` labels.
- **Renaming or removing a label**: requires a MAJOR version change and an
  explicit migration note. Create the replacement label before removing or
  relabeling old usage, and preserve auditability of prior state.
- **Changing meaning**: avoid semantic drift in MINOR or PATCH releases. If a
  meaning changes incompatibly, treat it as MAJOR and document the migration.
- **Manual changes**: because labels are observational projections, manual edits
  may make the displayed label inconsistent with execution reality. Migration and
  operations should treat manual changes as display drift, not as execution
  commands.

## Manifest format

The manifest is a YAML sequence of label objects with `name`, `color`, and
`description` keys, matching the shape used by `.github/labels.yml` and
level-1 extension entries. Colors are quoted six-digit hexadecimal strings.
