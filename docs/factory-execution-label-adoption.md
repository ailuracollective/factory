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

- **Factory execution catalog (canonical):** exactly the eight labels listed in `factory-execution-label-standard.md` at the pinned revision (`ai/queued`, `ai/processing`, `ai/waiting`, `ai/blocked`, `ai/needs-input`, `ai/failed`, `ai/cancelled`, `ai/completed`) with canonical names, colors, and descriptions.
- **Catalog entry:** one label object `{ name, color, description }` as defined in the canonical manifest.
- **Remote inventory:** the complete, paginated set of labels currently present on the target GitHub repository (via API), retrieved before any mutation.

## Preflight (required)

Complete all steps before making repository or remote changes.

1. **Read target instructions.** Read the target repository’s `AGENTS.md` and any linked organization conventions (for example `.agents/organization-conventions.md`) from the default branch. Follow repository-specific rules and any recorded departures.
2. **Identify the default branch.** Determine the default branch from the repository metadata/API (do not assume `main`).
3. **Inspect repository eligibility and permissions.** Confirm the target is not archived and issues are enabled if your workflow depends on issue labels. Confirm the actor/token has permission to manage labels (typically administration/triage capabilities sufficient to create/update labels). If permissions are insufficient, **stop**: do not attempt partial remote mutations; record the blocker.
4. **Pin one Factory revision.** Choose and record a single Factory commit SHA (required). Optionally record a tag/release name. All catalog comparisons in this adoption use that pinned revision only.
5. **Work through a dedicated branch and pull request.** Create a dedicated branch from the default branch. Perform all file changes there. Open a pull request (draft first if your environment requires it) and follow the target repository’s PR policy.

If any preflight step fails (archived repository, disabled issues where required, insufficient permissions, cannot determine default branch), stop and report the condition. Do not proceed with remote label mutations.

## Adoption modes (supported targets)

The procedure supports targets in these states. In every case, preserve existing project behavior and do not invent new label families.

- **Repository with Ailura organization standards adopted** (has organization conventions files as per that standard): treat the catalog as a **level-1 extension** concern where applicable. Record the exact catalog entries in the target’s standards customization record if the target uses `.github/standards.local.yml` (or the target’s documented equivalent). Follow that file’s rules (every override/drop/structural entry requires a `reason`; level-3 deviations require an approving issue recorded there). Do **not** copy the standards internal validator into the adopter.
- **Repository with custom conventions** (no organization standards, or customized): document the adoption in the repository’s `AGENTS.md` or an equivalent documented location if the repo requires it. Do not create a parallel “shadow” manifest that contradicts the pinned catalog.
- **Repository with no standard**: add only the documented adoption notes/files the target repo expects (minimal). Still pin the Factory revision and record it in the PR.

When adding catalog labels to a target that tracks a local manifest (for example a copied `labels.yml` style file), preserve the **existing manifest shape** and formatting conventions of that file. Do **not** reorder unrelated entries unnecessarily. Record exact catalog entries as **level-1 extensions** in the target’s customization record when the target’s model requires it; otherwise document the canonical source and pinned revision in-repo as agreed by the target’s conventions.

## Remote label inventory and collision policy (required before mutations)

1. **Fetch a complete paginated remote inventory.** List all labels on the target repository using paginated API calls until every page is retrieved. Do not assume the first page is complete.
2. **Normalize names case-insensitively** for comparison (`ai/Queued` and `ai/queued` are the same logical name to GitHub).
3. **Classify each catalog name** against the remote inventory:
   - **Missing:** no remote label matches case-insensitively → candidate to create.
   - **Exact match (case-insensitively):** remote label exists with the same canonical name → candidate to update only if `color` or `description` differs from the pinned catalog (normalize color format as stored by GitHub: typically lowercase hex without a leading `#` in API responses; compare by the canonical six-digit hex value from the manifest).
   - **Collision / conflict:** an existing remote label that is **not** one of the eight canonical catalog names but would be problematic? The catalog owns only its exact names. However:
     - If any remote label’s case-insensitive name equals a canonical catalog name but differs in an unexpected way you cannot reconcile safely, treat it as a **conflict requiring explicit resolution** before mutation.
     - If other `ai/*` labels exist that are **not** in the catalog (e.g. `ai/custom-thing`), they are **outside catalog ownership**. **Preserve them.** Do not rename, delete, or repurpose them.
4. **Incompatible existing labels:** if a remote label has the same case-insensitive name as a canonical catalog label but you must not change it (policy, ownership, or explicit human direction), you must obtain **explicit authorization** to proceed with a different resolution (rename-away is rarely appropriate; usually the safe resolution is to **not** apply that catalog label and to report the conflict). Do not silently overwrite.

**Separate authorization for remote mutations:** any operation that creates, updates, or deletes labels on the remote repository is a **remote mutation** and requires **separate explicit authorization** from a human maintainer (or an explicitly granted automation policy) before execution. Planning and local file edits do not require this authorization; performing GitHub API/CLI label writes does. If authorization is not granted, stop after documenting the planned changes and conflicts.

## Reconciliation rules (idempotent)

When authorized to mutate remotes:

- Apply changes so that running the same reconciliation again produces **no additional changes** (idempotent).
- For each canonical catalog label:
  - **Create** if missing (using canonical `name`, `color`, `description` from the pinned manifest).
  - **Update** only the remote label’s color and/or description to match the pinned catalog when they differ. Do **not** change the label name casing arbitrarily except to the canonical form only if the platform requires normalization and the logical name is identical; prefer leaving the existing canonical-name label in place and aligning color/description.
- **Preserve unrelated labels**, including all other `ai/*` names not in the catalog.
- **No wildcard deletion.** Never delete labels except as part of an explicitly authorized, documented **non-destructive removal** workflow (see below), and never via a wildcard “sync/delete all not in list” operation.
- **No silent overwrite.** If a needed change conflicts with policy or an unresolved collision, do not apply it silently; report and stop or request explicit resolution.

Recommended tooling (adapt to environment): use the GitHub CLI (`gh`) or API with pagination. Example pattern from the manifest (review diff intent first; do not blindly pipe unreviewed output in automated contexts without safeguards):

```bash
# Inspect first; do not run destructive loops. Example create/update pattern:
yq -r '.[] | "gh label create \"\(.name)\" --color \"\(.color)\" --description \"\(.description)\" --force"' \
  .github/labels.factory-execution.yml
```

The `--force` behavior updates existing labels with the same name in practice on many setups, but you must still have performed the inventory and collision analysis above and obtained separate authorization for remote mutations. **Always quote colors** (e.g. `CFD3D7`).

## Partial installation, failures, and recovery

- **Partial installation:** if some labels were created/updated and later steps fail (network error, rate limit, permission loss), **do not** pretend the catalog is fully adopted. Record exactly which canonical labels are present/absent and which differ on the remote after the attempt.
- **Insufficient permissions:** stop immediately on permission errors. Do not retry in a loop that could mask the issue. Report required permissions and observed failure.
- **Archived repositories:** do not mutate labels on archived repositories. Stop and report.
- **Disabled issues:** if the repository has issues disabled and your adoption requires issue labels, stop and report (state the constraint). If the organization policy allows labels in that state, document the decision explicitly in the PR.
- **Roll back remote changes independently of Git commits:** Git history does not undo GitHub label API changes. To “roll back” remote mutations you must apply compensating label operations (restore previous `color`/`description` from your recorded pre-change inventory, or delete labels that were **only** created in this adoption **and** you are explicitly authorized to remove). Never roll back by deleting unrelated labels.

Record a pre-mutation snapshot (at minimum: for each canonical name, whether it existed and its prior color/description) before applying changes, so rollback is possible.

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
- **Never** delete unrelated labels, including other `ai/*` labels not in the catalog.
- Prefer **leaving labels in place but unused** if removal risk is unclear. If removal is authorized, document each label action (delete vs. revert color/description) and obtain separate authorization for remote mutations.
- This does not delete issues, close workflows, or rewrite Git history except normal branch/PR operations.

## Verification (required)

Verification means checking **actual remote state** against the pinned catalog, not assuming success.

After (and only after) authorized remote operations, or during audit without mutations:

- Re-fetch the remote label inventory (paginated).
- For each of the eight canonical labels, verify **remote name** (case-insensitive match to canonical), **color**, and **description** equal the pinned manifest values.
- Confirm no unintended changes to unrelated labels (spot-check or compare against your pre-mutation snapshot).
- If any step could not be performed (permissions, API errors), record it as **unverified** and explain why. **Do not** claim the adoption is verified.

Report verification results explicitly in the adoption PR (see below).

## Adoption pull request requirements

The adoption PR (in the target repository) must record, at minimum:

- **Source revision:** pinned Factory commit SHA (and tag/release if any) used for the canonical manifest and standard.
- **Changes:** files changed in the target repo (e.g. added/updated manifest copies, `standards.local.yml` entries, `AGENTS.md` notes) and a summary of remote label operations performed (or planned, if mutations pending authorization).
- **Deviations:** any approved deviations from the target’s standard/conventions, with reasons, and for level-3 deviations the approving issue reference as required by the target’s process.
- **Observed verification results:** what was checked, against which remote snapshot/time, and pass/fail per canonical label (or explicit unverified items).
- **Outstanding approvals:** list any required human approvals not yet obtained (e.g. separate authorization for remote mutations, maintainer review of conflicts).

Use the target repository’s PR template(s) and link the related issue if the target requires it.

## Exercising the procedure (evidence to record)

When validating this procedure (including in authorized test repositories), exercise and record evidence for at least these cases:

- Empty target (no prior catalog labels).
- Target that follows organization standards.
- Target with custom conventions (and/or no standards).
- Repeat adoption (idempotency): run reconciliation again after a successful adoption.
- Interrupted adoption: simulate or record a failure mid-way (partial creates/updates) and the recovery/recording steps taken.
- Conflicts: at least one case-insensitive name collision scenario and one incompatible existing label scenario, with explicit resolution recorded.
- Insufficient permissions: observed failure and stop behavior.
- Upgrades: moving between pinned revisions (including at least one PATCH or MINOR path and noting MAJOR handling expectations).

For each exercise, record: repository (or test repo identifier), pinned revision, date, actions taken, verification output summary, and any deviations or blockers. Store evidence in the PR description or a linked artifact as the target repository allows.

## Related documents

- [Factory execution label standard](./factory-execution-label-standard.md)
- [Organization conventions](../.agents/organization-conventions.md) (this repository)
