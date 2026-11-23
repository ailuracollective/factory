# Organization conventions

Mandatory for every repository in `ailuracollective`. Each repository keeps a
copy at `.agents/organization-conventions.md`, and its `AGENTS.md` links here
instead of restating any of it.

These are the rules shared across repositories. A repository's own language,
layout, build and test commands are not here; they belong in its `AGENTS.md`. A
repository that departs from a rule below records the departure in its `AGENTS.md`,
with the reason it was taken. An exception nobody wrote down is not an exception.

## The standard is copied by hand

The shared standard originates in `ailuracollective/standards` and is copied into
each repository's `.github/`:

- `.github/ISSUE_STANDARD.md` — how an issue is written.
- `.github/labels.yml` — the label manifest.
- `.github/ISSUE_TEMPLATE/` — the issue forms.
- `.github/PULL_REQUEST_TEMPLATE/` — the pull-request forms.
- `.github/CODEOWNERS` — who reviews what.

Copy this file to `.agents/organization-conventions.md`, and `templates/AGENTS.md`
from the origin to the repository root as `AGENTS.md`.

There is no sync mechanism. Editing the origin changes nothing elsewhere, and
copies drift by default. A change to a shared artifact is finished only when it
has been mirrored into every adopting repository by hand.

## Issues

Issues follow `.github/ISSUE_STANDARD.md` — its types and title convention, its
core sections, and its writing principles — and are opened through the templates
in `.github/ISSUE_TEMPLATE/`. The standard's `config.yml` disables blank issues.

- One type per issue, matching the template's `title:` prefix.
- Every template applies `status/needs-review` itself, so an issue is never
  briefly unlabelled.
- Name only labels declared in `.github/labels.yml`. GitHub drops an unknown
  label silently, without an error.

## Pull requests

- Titles follow Conventional Commits. The allowed types are the filenames of
  `.github/PULL_REQUEST_TEMPLATE/`, minus the default form.
- Branches are `<github-username>/<type>/<description>`, all lowercase. The type
  is the title type, except `breaking-change`, which is a commit marker and not a
  branch type.
- The body follows the template for the title's type. Common headings are:
  `## Linked issue`, `## Summary`, `## Changes`, `## Verification`,
  `## Risk / compatibility` (optional), `## Migration` (optional).
- Link the issue with a closing keyword (`Closes #N`); the issue must carry
  `status/ready`.
- Apply exactly one `type/*` label. The title type and the label differ: several
  title types share a label.
- `## Verification` asks for evidence, not commands. Do not name commands CI
  already runs.
- Update documentation where public behavior or configuration changed.
- All CI checks pass.

## Labels

`.github/labels.yml` is the manifest. It is a record, not configuration: GitHub
does not read it, and editing it changes no remote.

- Six families: `type/`, `status/`, `scope/`, `meta/`, `github_actions`, and
  `release/`.
- `type/` is frozen at five members. A repository-specific concern uses an
  existing `type/*` label plus an `extensions` entry in its customization record;
  it does not add a `type/*` label.
- Never name a label the manifest does not declare.

## Commits

Conventional Commits. The type is not decoration: the pull-request title check
reads it, and the body check resolves the template from it.

## Review

Changes land through a pull request, not a direct push to the default branch.
`.github/CODEOWNERS` assigns reviewers.

- The **last** matching pattern wins; ownership is not additive.
- Keep a catch-all rule: an unowned path skips the code-owner gate.
- CODEOWNERS binds only through branch protection or rulesets. GitHub silently
  skips a line it cannot parse and an owner without write access.
- Configured is not enforced. Verify against the specific repository and branch
  through the GitHub API before calling a rule binding.

## Customization

A repository that changes nothing is an exact copy. To deviate, copy
`.github/standards.local.example.yml` to `.github/standards.local.yml` and declare
each change. A `reason` is required on every override, drop and structural entry;
a level-3 deviation also records the issue that approved it. The example file's
header is the reference for the sections.

- It declares; it does not configure. No workflow reads it, including the gate.

## Documentation

- A change to public behavior or configuration updates the documentation in the
  same change.
- A generated changelog is not edited by hand.
- What "documented" means for a repository — a comment, a docstring, a page, a
  changelog entry — is declared in its `AGENTS.md`.

## Agent instructions

- An agent that opens a pull request opens it as a draft.
- Every repository has an `AGENTS.md` at its root. Agents read the nearest one
  above the file they are editing; the deeper file wins for the paths it covers.
- `AGENTS.md` links these conventions and does not restate them. If a rule is not
  in this file, it is not an organisation rule.
- Repository-specific rules, and every departure from these conventions, are
  recorded in `AGENTS.md` with a reason.

## Security and workflows

- Workflow permissions are minimal: each job declares only what it needs.
- A workflow that executes code from a pull-request head never receives secrets,
  and a policy check does not execute the code under review.
- Every workflow job sets `timeout-minutes`.
- Checkout sets `persist-credentials: false` unless credentials are required.
