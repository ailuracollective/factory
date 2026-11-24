# Factory execution label adoption (AI-assisted)

This guide defines a repeatable, auditable procedure for an AI coding agent to adopt, update, verify, and (optionally) remove the **Factory execution label catalog** in an authorized target repository.

The catalog is Factory-specific and independent of the Ailura organization-wide label taxonomy. It does not replace organization standards; it can be used alongside them, with custom conventions, or in repositories that adopt neither.

## Canonical references (pin before acting)

Adopt from a single pinned Factory source revision. Do not follow a moving branch for an adoption or upgrade.

- Canonical catalog manifest: [`.github/labels.factory-execution.yml`](../.github/labels.factory-execution.yml) in the Factory repository at the pinned revision.
- Canonical semantics and versioning: [`docs/factory-execution-label-standard.md`](./factory-execution-label-standard.md) in the Factory repository at the pinned revision.
- The manifest and the standard **must agree** at that revision.

When the Factory repository is the target, the files above are local. When adopting into another repository, obtain those files from the pinned Factory commit (for example by checking out the pinned revision in a temporary clone or by downloading the raw files at that commit). Record the pinned revision (commit SHA, and tag/release if used) in the adoption pull request.

External references (for context only; do not copy their internal validators into adopters):

- Ailura Standards installation guide: <https://github.com/ailuracollective/standards/blob/main/docs/install-standards.md>
- Ailura Standards customization guide: <https://github.com/ailuracollective/standards/blob/main/docs/customization.md>

## Non-goals and constraints

This procedure does **not**:

- implement an installer command, runtime synchronization, or a state machine;
- require mandatory adoption of organization standards;
- change unrelated target workflows, credentials, secrets, or labels outside the reconciliation rules below.

Constraints for adopters:

- Do **not** modify the organization standard in `ailuracollective/standards`.
- Do **not** copy the organization standard’s internal validator into adopters.
- Do **not** change unrelated target repository workflows, credentials, or labels.

## Definitions

- **Factory label catalog (canonical):** the standalone `ai` trigger plus eight execution-state identifiers defined in `factory-execution-label-standard.md` at the pinned revision: `queued`, `processing`, `waiting`, `blocked`, `needs-input`, `failed`, `cancelled`, `completed`. The state identifiers are canonical and independent of the configured prefix; they are never renamed per state.
- **Effective catalog:** the eight canonical state identifiers resolved into effective label names using the target repository’s configured `labels.prefix` (default `ai/`). With the default prefix the effective names are `ai/queued`, `ai/processing`, `ai/waiting`, `ai/blocked`, `ai/needs-input`, `ai/failed`, `ai/cancelled`, `ai/completed`. With `labels.prefix: "factory/"`, the same states resolve to `factory/queued` through `factory/completed`. The procedure never selects an alternative prefix automatically.
- **Catalog entry:** one label object `{ name, color, description }` as defined in the canonical manifest. The trigger remains `ai`; execution-state names resolve with the configured prefix.
- **Remote inventory:** the complete, paginated set of labels currently present on the target GitHub repository (via API), retrieved before any mutation.
- **State assignment:** an issue’s current catalog membership for the effective catalog (at most one effective catalog label, or none). A migration or adoption must preserve the semantic assignment unless a human explicitly authorizes a conflicting-assignment resolution.
- **Configuration drift:** any inconsistency between the configured prefix, local manifests/customization records, remote effective catalog labels, installed catalog state, or active execution configuration. Drift must be detected and reported before mutation.

## Preflight (required)

Complete all steps before making repository or remote changes.

1. **Read target instructions.** Read the target repository’s `AGENTS.md` and any linked organization conventions (for example `.agents/organization-conventions.md`) from the default branch. Follow repository-specific rules and any recorded departures.
2. **Identify the default branch.** Determine the default branch from the repository metadata/API (do not assume `main`).
3. **Inspect repository eligibility and permissions.** Confirm the target is not archived and issues are enabled if your workflow depends on issue labels. Confirm the actor/token has permission to manage labels (typically administration/triage capabilities sufficient to create/update labels). If permissions are insufficient, **stop**: do not attempt partial remote mutations; record the blocker.
4. **Pin one Factory revision.** Choose and record a single Factory commit SHA (required). Optionally record a tag/release name. All catalog comparisons in this adoption use that pinned revision only.
5. **Resolve and validate the configured prefix.** Read `factory.json` (if present) and resolve `labels.prefix`; default to `ai/` when omitted. Validate the prefix before any mutation: it must be a single lowercase ASCII namespace followed by `/`, use only lowercase letters, digits, `-`, or `_` in the namespace, contain no nested namespaces, and produce effective names within GitHub’s 50-character label limit. If the prefix is invalid, **stop** and report the validation error; do not select an alternative prefix automatically.
6. **Detect reserved-family conflicts.** Confirm the configured prefix does not conflict with reserved families of applicable repository standards. For Standards adopters, organization core families (`type/`, `status/`, `scope/`, `meta/`, `github_actions/`, `release/`) are protected; `complexity/` is protected when adopted. A reserved-family conflict is a blocker: **stop**, report it, and obtain explicit maintainer approval for any standards-level resolution before proceeding. Do not auto-select a different prefix.
7. **Detect configuration drift.** Compare the configured prefix, any local manifest or customization record that declares catalog labels, any installed/active catalog state known to the target, and the remote effective catalog labels. If these sources disagree, record the drift as a blocker for mutation until it is explicitly resolved. Do not silently reconcile drift by overwriting one source.
8. **Work through a dedicated branch and pull request.** Create a dedicated branch from the default branch. Perform all file changes there. Open a pull request (draft first if your environment requires it) and follow the target repository’s PR policy.

If any preflight step fails (archived repository, disabled issues where required, insufficient permissions, cannot determine default branch, invalid or reserved prefix, unresolved configuration drift), stop and report the condition. Do not proceed with remote label mutations.

## Adoption modes (supported targets)

The procedure supports targets in these states. In every case, preserve existing project behavior, do not invent new label families, and never select an alternative prefix automatically.

- **Repository with Ailura organization standards adopted** (has organization conventions files as per that standard): treat the catalog as a **level-1 extension** concern where applicable. Record the exact effective catalog entries in the target’s standards customization record if the target uses `.github/standards.local.yml` (or the target’s documented equivalent). The customization record, any local manifest, and the remote effective catalog labels must use the same effective names. Follow that file’s rules (every override/drop/structural entry requires a `reason`; level-3 deviations require an approving issue recorded there). Do **not** copy the standards internal validator into the adopter.
- **Repository with custom conventions** (no organization standards, or customized): document the adoption in the repository’s `AGENTS.md` or an equivalent documented location if the repo requires it. Any local manifest or customization record that declares catalog labels must use the same effective names as the remote effective catalog. Do not create a parallel “shadow” manifest that contradicts the pinned catalog.
- **Repository with no standard**: add only the documented adoption notes/files the target repo expects (minimal). Still pin the Factory revision and record it in the PR. If a local manifest is added, it must use the effective catalog names.

When adding catalog labels to a target that tracks a local manifest (for example a copied `labels.yml` style file), preserve the **existing manifest shape** and formatting conventions of that file. Do **not** reorder unrelated entries unnecessarily. Record exact effective catalog entries as **level-1 extensions** in the target’s customization record when the target’s model requires it; otherwise document the canonical source, configured prefix, and pinned revision in-repo as agreed by the target’s conventions. Unrelated namespaces and labels are preserved in all modes.

## Remote label inventory and collision policy (required before mutations)

1. **Fetch a complete paginated remote inventory.** List all labels on the target repository using paginated API calls until every page is retrieved. Do not assume the first page is complete.
2. **Normalize names case-insensitively** for comparison (`ai/Queued` and `ai/queued` are the same logical name to GitHub).
3. **Resolve the effective catalog.** Read `labels.prefix` from the target repository `factory.json` (default `ai/`). Resolve the eight canonical state identifiers into effective label names with that prefix and validate the prefix before mutation. Include the standalone `ai` trigger unchanged. Do not select an alternative prefix automatically, including when the default namespace is occupied.
4. **Detect pre-mutation blockers.** Before any mutation, confirm there are no unresolved blockers: insufficient permissions, invalid prefix, reserved-family conflict, configuration drift, case-insensitive collisions, incompatible existing labels, or an unapproved prefix migration. If any blocker exists, **stop** and report it; do not attempt partial remote mutations.
5. **Classify each catalog name** against the remote inventory:
   - **Missing:** no remote label matches case-insensitively → candidate to create.
   - **Exact match (case-insensitively):** remote label exists with the same logical canonical name → candidate to update only if `color` or `description` differs from the pinned catalog (normalize color format as stored by GitHub: typically lowercase hex without a leading `#` in API responses; compare by the canonical six-digit hex value from the manifest).
   - **Case-insensitive collision:** a remote label matches a resolved catalog name case-insensitively but not exactly, or otherwise cannot be reconciled safely. Treat it as a **conflict requiring explicit resolution** before mutation.
   - **Occupied or incompatible label:** a remote label already uses a resolved catalog name with incompatible meaning, ownership, or a human-directed constraint that prevents change. Treat it as a **conflict requiring explicit authorization**; do not silently overwrite.
6. **Preserve namespace neighbors.** If other labels exist under the configured prefix namespace that are **not** in the resolved catalog (e.g. `ai/custom-thing` when using `ai/`), they are **outside catalog ownership**. **Preserve them.** Do not rename, delete, or repurpose them. Unrelated namespaces and labels outside the resolved catalog are also preserved.
7. **Prefix changes require explicit migration.** If the configured prefix differs from an installed catalog, a local manifest/customization record, or active execution, do not silently apply the new prefix. Follow the [Prefix migration](#prefix-migration-explicit-state-preserving) procedure.

**Separate authorization for remote mutations:** any operation that creates, updates, or deletes labels on the remote repository is a **remote mutation** and requires **separate explicit authorization** from a human maintainer (or an explicitly granted automation policy) before execution. Planning and local file edits do not require this authorization; performing GitHub API/CLI label writes does. If authorization is not granted, stop after documenting the planned changes and conflicts.

## Reconciliation rules (idempotent)

When authorized to mutate remotes:

- Apply changes so that running the same reconciliation again produces **no additional changes** (idempotent).
- Resolve the effective catalog names from the configured `labels.prefix` before mutation. The canonical manifest declares default `ai/` names; if a custom prefix is configured, substitute the configured prefix for `ai/` when deriving effective names, while preserving the canonical state identifiers, colors, and descriptions.
- For each effective catalog label:
  - **Create** if missing (using the effective `name`, and canonical `color` and `description` from the pinned manifest).
  - **Update** only the remote label’s color and/or description to match the pinned catalog when they differ. Do **not** change the label name casing arbitrarily except to the effective canonical form only if the platform requires normalization and the logical name is identical; prefer leaving the existing effective-name label in place and aligning color/description.
- **Preserve unrelated labels**, including all other labels in the configured prefix namespace that are not in the resolved catalog.
- **No wildcard deletion.** Never delete labels except as part of an explicitly authorized, documented **non-destructive removal** workflow (see below), and never via a wildcard “sync/delete all not in list” operation.
- **No silent overwrite.** If a needed change conflicts with policy or an unresolved collision, do not apply it silently; report and stop or request explicit resolution.

Recommended tooling (adapt to environment): use the GitHub CLI (`gh`) or API with pagination. Example pattern from the manifest (review diff intent first; do not blindly pipe unreviewed output in automated contexts without safeguards). This example assumes the default `ai/` prefix; for a custom prefix, replace the label name prefix with the configured effective prefix before running:

```bash
# Inspect first; do not run destructive loops. Example create/update pattern:
yq -r '.[] | "gh label create \"\(.name)\" --color \"\(.color)\" --description \"\(.description)\" --force"' \
  .github/labels.factory-execution.yml
```

The `--force` behavior updates existing labels with the same name in practice on many setups, but you must still have performed the inventory, prefix validation, collision/config-drift analysis, and obtained separate authorization for remote mutations. **Always quote colors** (e.g. `CFD3D7`).

## Partial installation, failures, and recovery

- **Partial installation:** if some labels were created/updated and later steps fail (network error, rate limit, permission loss), **do not** pretend the catalog is fully adopted. Record exactly which effective catalog labels are present/absent and which differ on the remote after the attempt.
- **Insufficient permissions:** stop immediately on permission errors. Do not retry in a loop that could mask the issue. Report required permissions and observed failure.
- **Archived repositories:** do not mutate labels on archived repositories. Stop and report.
- **Disabled issues:** if the repository has issues disabled and your adoption requires issue labels, stop and report (state the constraint). If the organization policy allows labels in that state, document the decision explicitly in the PR.
- **Roll back remote changes independently of Git commits:** Git history does not undo GitHub label API changes. To “roll back” remote mutations you must apply compensating label operations (restore previous `color`/`description` from your recorded pre-change inventory, or delete labels that were **only** created in this adoption **and** you are explicitly authorized to remove). Never roll back by deleting unrelated labels.

Record a pre-mutation snapshot (at minimum: for each effective catalog name, whether it existed and its prior color/description) before applying changes, so rollback is possible. Also record the configured prefix and any existing state assignments when migration may be involved.

## Prefix migration (explicit, state-preserving)

A prefix change is a migration, not a normal reconciliation. It applies when the target’s configured `labels.prefix` differs from an installed catalog, from local manifests/customization records, or from labels used by active execution. Never apply a prefix change silently, never select an alternative prefix automatically, and never delete old remote labels by default.

### Migration preconditions

Before mutating anything, obtain and record **explicit authorization** for the migration. The authorization must name the old prefix, new prefix, pinned Factory revision, target repository, and actor. Then verify all preconditions:

1. **Verified safe state.** Confirm no conflicting active execution or label writers are operating against the target repository. This includes Factory runs, automation jobs, bots, or humans actively applying or mutating execution labels. If safe state cannot be verified, **stop**.
2. **Valid target configuration.** Validate the new prefix (format, length, reserved families) and resolve the new effective catalog. If invalid or reserved, **stop**; do not auto-select an alternative.
3. **Inventory old and new catalogs.** Fetch the complete paginated remote label inventory. Resolve both the old effective catalog and the new effective catalog.
4. **Inventory existing assignments and ownership.** Record every issue currently assigned any old effective catalog label, including issue number, current old label, and any conflicting catalog assignment. Also record label ownership evidence: which old labels exist remotely, their color/description, whether they were installed by Factory/catalog adoption, and any unrelated labels in either namespace.
5. **Detect conflicts.** Identify case-insensitive collisions, incompatible existing labels in the new namespace, conflicting assignments (an issue carrying more than one catalog state, or old/new mixed state that cannot be resolved safely), and configuration drift. Conflicting assignments require explicit human resolution; do not guess.

If any precondition fails, stop, report the blocker, and do not mutate.

### Migration ordering

Perform migration in this order. Do not reorder steps in a way that leaves active execution pointing at an uninstalled catalog.

1. **Record pre-mutation snapshot.** Save the remote inventory, old/new effective catalogs, issue assignment inventory, ownership evidence, configuration sources, and authorization record.
2. **Prepare local configuration and manifests.** On the migration branch, update `factory.json` `labels.prefix`, local manifests, and applicable level-1 customization entries so they all use the same new effective names. Keep the old catalog entries documented in the migration record; do not delete old remote labels.
3. **Install the new effective catalog remotely.** After separate authorization for remote mutations, create missing new effective catalog labels and update color/description where needed. Do not remove old labels yet.
4. **Migrate state assignments.** For each issue with an old catalog label, add the corresponding new effective catalog label for the same state identifier, then remove the old catalog label from that issue. Handle one issue at a time or in small auditable batches. If a conflicting assignment is found, stop that issue, record it, and require explicit human resolution; continue only with unambiguous assignments unless instructed otherwise.
5. **Verify before retiring old labels.** Re-fetch remote inventory and verify every new effective catalog label name, color, and description. Verify migrated issue assignments: each migrated issue has the expected new label and no old catalog label. Confirm unrelated labels and unrelated namespace labels are unchanged.
6. **Retire old catalog labels only when explicitly authorized.** Default behavior is to **leave old remote labels in place**. Only after verification passes and a human explicitly authorizes retirement may you remove old catalog labels that are no longer assigned and are confirmed catalog-owned. Never delete old remote labels by default, never delete unrelated namespace labels, and never use wildcard deletion.
7. **Activate configuration.** Merge/apply the configuration change only after the new catalog is installed and verified (or as explicitly ordered in the authorized migration plan). If configuration activation and remote changes cannot be atomic, document the ordering, the window where drift may exist, and the recovery procedure.

### Recovery and rollback

Interrupted migrations must be safely resumable. Keep the pre-mutation snapshot and a migration progress record (which labels created, which issues migrated, which conflicts remain).

- **Resume:** re-fetch remote inventory and assignment state, compare against the progress record, and continue from the next incomplete unambiguous step. Do not duplicate already-migrated assignments in a harmful way; reconciliation should be idempotent.
- **Rollback remote changes:** Git history does not undo label mutations. To roll back, restore prior label colors/descriptions from the snapshot, remove newly created catalog labels only if explicitly authorized and no longer needed, and restore old issue assignments from the recorded inventory. Do not delete unrelated labels.
- **Rollback configuration:** revert `factory.json`, local manifests, and customization entries to the old effective names in a follow-up change if the new catalog is not being kept. Do not leave configuration pointing at a catalog that is not installed.
- **Partial failure:** if migration fails mid-way, report exact state: new labels created, assignments migrated/unmigrated/conflicting, old labels still present, configuration activated or not, and required next action. Do not claim success.

## Upgrades (pinned revision changes)

When moving from an older pinned Factory revision to a newer one:

1. Pin the new revision and review `factory-execution-label-standard.md` and `labels.factory-execution.yml` diffs at that revision.
2. Classify changes by semver intent documented upstream:
   - **PATCH:** editorial only; remote label values should remain unchanged unless text drift existed—verify descriptions/colors still match the new canonical files.
   - **MINOR:** backward-compatible additions (e.g. new catalog labels). Adoption adds the new labels only; do **not** remove or repurpose existing labels.
   - **MAJOR:** breaking changes (renames/removals/semantic changes). Follow the migration notes in the standard. Require explicit human authorization and an explicit migration plan in the PR (including handling of historical labels). Prefer additive transitions (create replacement, document relabeling policy) over destructive deletion.
3. Re-run full inventory, collision checks, and reconciliation as in initial adoption.
4. In the PR, state the old and new pinned revisions and summarize label-level changes.

## Non-destructive removal (voluntary)

Removing the catalog from a target is **optional** and must be **non-destructive** to unrelated data:

- Only remove or revert **Factory catalog labels** that the adoption introduced or that you are explicitly authorized to change.
- **Never** delete unrelated labels, including other labels in the configured prefix namespace that are not in the resolved catalog.
- Prefer **leaving labels in place but unused** if removal risk is unclear. If removal is authorized, document each label action (delete vs. revert color/description) and obtain separate authorization for remote mutations.
- This does not delete issues, close workflows, or rewrite Git history except normal branch/PR operations.

## Verification (required)

Verification means checking **actual remote state** against the pinned catalog, not assuming success.

After (and only after) authorized remote operations, or during audit without mutations:

- Re-fetch the remote label inventory (paginated).
- For the standalone `ai` trigger and each of the eight effective catalog labels, verify **remote name** (case-insensitive match to the effective catalog name), **color**, and **description** equal the pinned manifest values.
- Confirm local manifests, applicable level-1 customization entries, and remote effective catalog labels use the same effective names.
- Confirm no unintended changes to unrelated labels, including unrelated labels in the configured prefix namespace (spot-check or compare against your pre-mutation snapshot).
- For migrations, verify issue state assignments: every migrated issue has the expected new effective catalog label, no unexpected old catalog label, and no conflicting assignment was silently guessed. Verify old catalog labels were not deleted unless explicitly authorized and recorded.
- If any step could not be performed (permissions, API errors), record it as **unverified** and explain why. **Do not** claim the adoption or migration is verified.

Report verification results explicitly in the adoption PR (see below).

## Adoption pull request requirements

The adoption PR (in the target repository) must record, at minimum:

- **Source revision:** pinned Factory commit SHA (and tag/release if any) used for the canonical manifest and standard.
- **Configured prefix:** the resolved `labels.prefix`, whether default or custom, and confirmation that no alternative prefix was auto-selected.
- **Changes:** files changed in the target repo (e.g. added/updated manifest copies, `standards.local.yml` entries, `AGENTS.md` notes) and a summary of remote label operations performed (or planned, if mutations pending authorization).
- **Migration record (when applicable):** old/new prefixes, authorization, safe-state check, assignment/ownership inventory, conflict resolutions, ordering, recovery/rollback plan, and resume point for any interruption.
- **Deviations:** any approved deviations from the target’s standard/conventions, with reasons, and for level-3 deviations the approving issue reference as required by the target’s process.
- **Observed verification results:** what was checked, against which remote snapshot/time, and pass/fail per effective catalog label (or explicit unverified items). For migrations, include assignment verification and confirmation that old labels were not deleted by default.
- **Outstanding approvals:** list any required human approvals not yet obtained (e.g. separate authorization for remote mutations, migration authorization, maintainer review of conflicts).

Use the target repository’s PR template(s) and link the related issue if the target requires it. also fix the previous big edit failure? No, wait - I included a broken third edit in the previous failed call? Let us look up - the previous call failed at parse time, so none applied. I re-did verification as a single edit and it also failed "Could not find" because I had the wrong text. Now I'm retrying with correct text. The third edit is Exercising - I need to include it. I'll add it here.

## Exercising the procedure (evidence to record)

When validating this procedure (including in authorized test repositories), exercise and record evidence for at least these cases:

- Empty target (no prior catalog labels).
- Target that follows organization standards.
- Target with custom conventions (and/or no standards).
- Custom prefix: adoption using a configured non-default `labels.prefix`, with local manifests, customization entries, and remote labels using the same effective names.
- Occupied default namespace: the default `ai/` namespace contains incompatible or conflicting existing labels; confirm no alternative prefix is auto-selected and the conflict is reported/resolved explicitly.
- Invalid or reserved prefix: confirm invalid format, nested namespace, overlong effective name, or reserved-family prefix is detected before mutation and no alternative is auto-selected.
- Repeat adoption (idempotency): run reconciliation again after a successful adoption.
- Successful migration: authorized prefix migration preserves state assignments, handles conflicting assignments without guessing, verifies new catalog and assignments before retiring old labels, and leaves old remote labels in place by default.
- Interrupted migration: simulate or record a failure mid-way and confirm the migration can be safely resumed with the recorded snapshot/progress.
- Collision handling: at least one case-insensitive name collision scenario and one incompatible existing label scenario, with explicit resolution recorded.
- Insufficient permissions: observed failure and stop behavior.
- Upgrades: moving between pinned revisions (including at least one PATCH or MINOR path and noting MAJOR handling expectations).

For each exercise, record: repository (or test repo identifier), pinned revision, configured prefix(es), date, actions taken, verification output summary, and any deviations or blockers. Store evidence in the PR description or a linked artifact as the target repository allows.

## Related documents

- [Factory execution label standard](./factory-execution-label-standard.md)
- [Organization conventions](../.agents/organization-conventions.md) (this repository)
