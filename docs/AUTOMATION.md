# Automation

How this repository builds, tests, updates itself and publishes — and the
one-time setup GitHub needs before any of it works.

If you are reading this to get things running, start at
[First-time setup](#first-time-setup). It is five tasks and takes about ten
minutes.

## The shape of it

```
 nightly (04:00 UTC)                you                         result
 ────────────────────               ───                         ──────
 update-deps.yml
   cargo update
   test + build
   bump patch version
   open pull request  ──────────►  email arrives
                                   wait a day or two
                                   click Approve      ──────►  auto-merge
                                                               once CI is green
                                                                    │
 release.yml ◄──────────────────────────────────────────────────────┘
   sees a new version in Cargo.toml
   test + build
   tag v0.2.1
   publish release, becomes `latest`

 nightly.yml (05:00 UTC, and on every push to main)
   build main
   move the `nightly` tag
   replace the nightly prerelease
```

Your only interaction is the Approve click.

## Workflows

| File | Runs on | Does |
|---|---|---|
| `ci.yml` | every push and pull request | rustfmt, clippy, tests, wasm build, `cargo audit` |
| `update-deps.yml` | 04:00 UTC daily, or manually | updates dependencies, opens/updates one pull request |
| `nightly.yml` | 05:00 UTC daily, pushes to `main`, or manually | rebuilds `main`, moves the `nightly` release |
| `release.yml` | pushes to `main`, `v*.*.*` tags, or manually | publishes a release when `Cargo.toml` names an untagged version |

### Tags and releases

- **`vX.Y.Z`** — one per release, permanent. `/releases/latest/download/…`
  resolves to the newest of these.
- **`nightly`** — a single moving tag, force-updated each night. Published as a
  prerelease so it never becomes `latest`.

There is deliberately no tag named `latest`. GitHub already tracks the newest
release, and a real tag by that name would have to be force-pushed on every
release for no gain.

### Versioning

Releases are patch bumps: `update-deps.yml` increments the patch number as part
of the pull request it opens, so merging one lands a new version on `main` and
`release.yml` publishes it.

Minor and major bumps are yours to make. Edit `version` in `Cargo.toml`, merge
that to `main`, and the same machinery publishes it — the workflow only asks
whether the version in `Cargo.toml` has been tagged yet, not how it got there.

### How dependencies are chosen

`cargo update` only moves within the range each `Cargo.toml` entry allows. For
`0.x` crates that is patch releases, so `zellij-tile = "0.44.3"` accepts 0.44.4
but never 0.45.0.

That is the policy on purpose. **A `zellij-tile` newer than the Zellij you run
breaks hints silently.** Zellij's plugin boundary decodes a binding's actions
with `.filter_map(|a| a.try_into().ok())`, so an action the plugin does not
recognise is dropped and the binding arrives truncated. The plugin builds,
tests pass, and hints render with wrong labels. There is no error anywhere.

`.github/scripts/check_deps.py` asks crates.io what exists and reports anything
held back. When a Zellij crate has a new minor, the pull request is labelled
`needs-zellij-upgrade` and **auto-merge is not enabled** — upgrade Zellij
first, then bump the crate by hand.

## First-time setup

### 1. Create the automation token

A pull request opened with the built-in `GITHUB_TOKEN` does not start any
workflows. GitHub does this to prevent loops, but it means required checks
would sit pending forever and auto-merge would never fire. A personal access
token avoids that.

1. Go to **Settings → Developer settings → Personal access tokens →
   Fine-grained tokens** (on your account, not the repository).
2. **Generate new token**.
3. Name it `zjstatus-hints automation`. Set an expiry you will notice —
   90 days is reasonable; you will get an email before it lapses.
4. Under **Repository access**, choose **Only select repositories** and pick
   `zjstatus-hints`.
5. Under **Permissions → Repository permissions**, set:
   - **Contents**: Read and write
   - **Pull requests**: Read and write
   - **Workflows**: Read and write
6. Generate it and copy the value — it is shown once.
7. In the repository: **Settings → Secrets and variables → Actions →
   New repository secret**. Name it exactly `AUTOMATION_TOKEN`, paste the
   value, save.

`update-deps.yml` checks for this first and fails with an explanation if it is
missing, rather than opening a pull request that can never merge.

### 2. Let Actions open pull requests

**Settings → Actions → General → Workflow permissions**:

- Select **Read and write permissions**.
- Tick **Allow GitHub Actions to create and approve pull requests**.
- Save.

### 3. Turn on auto-merge

**Settings → General → Pull Requests**: tick **Allow auto-merge**.

Without this, `gh pr merge --auto` fails and the workflow logs a warning
pointing back here.

### 4. Protect `main`

Auto-merge needs something to wait for. With no rule, "merge when checks pass"
has no checks to pass and GitHub refuses to arm it.

**Settings → Rules → Rulesets → New branch ruleset**:

- Name: `main`
- Enforcement status: **Active**
- Target branches: **Include default branch**
- Tick **Require a pull request before merging**
  - Required approvals: **1**
- Tick **Require status checks to pass**
  - Add: `rustfmt`, `clippy`, `test`, `build (wasm)`
- Leave **Require branches to be up to date** off, or a busy day means
  rebasing before every merge.

> The status check names must match the `name:` of each job in `ci.yml`. They
> only appear in the picker after a workflow has run once, so push this branch
> first and come back.

Since you are the only maintainer, also decide whether to tick **Do not allow
bypassing the above settings**. Leaving it unticked lets you push directly to
`main` when you need to; ticking it means even you go through a pull request.

### 5. Get the email

**Your avatar → Settings → Notifications**:

- Under **Subscriptions → Watching**, make sure email is enabled.
- On the repository page, **Watch → All Activity** (or **Custom → Pull
  requests** for less noise).

Test it with **Actions → Update dependencies → Run workflow**. If dependencies
are already current the job stops early without opening anything, which is also
a useful signal that the plumbing works.

## Approving

The email arrives when the nightly finds updates. What to look at:

- The pull request body lists what moved and what was held back.
- If it is labelled `needs-zellij-upgrade`, read the warning before approving.
  Auto-merge is off for that one deliberately.
- CI runs on the pull request; tests and a release build also passed *before*
  it was opened, so a red pull request means CI found something the update job
  did not.

Click **Approve**. Auto-merge takes it from there: merge, delete the branch,
publish the release, rebuild the nightly.

To stop one: **Close** it. The branch is reused, so the next run reopens with
whatever is current — nothing is lost by closing one you dislike.

## Installing what is published

```sh
make latest    # newest tagged release
make nightly   # tonight's build of main
```

Both fetch straight into the Zellij plugin path. Start a new session to load
it — Zellij caches plugins per session, and detaching does not reload.

Pointing your Zellij config at a nightly URL does not work well: Zellij caches
remote plugins by URL, so it keeps serving whatever it downloaded first. Fetch
locally instead.

## Maintenance

**Pin the actions.** Three are pinned to commit SHAs; three carry a
`# TODO: pin to a SHA` comment because they were added without network access
to verify one. A version tag can be moved by whoever controls the action, so
pinning is worth doing:

```sh
gh api repos/Swatinem/rust-cache/commits/v2 --jq .sha
gh api repos/rustsec/audit-check/commits/v2 --jq .sha
gh api repos/peter-evans/create-pull-request/commits/v7 --jq .sha
```

Then replace `@v2` with `@<sha> # v2`. Dependabot updates the pinned ones
monthly.

**Rotate the token** when the expiry email arrives — regenerate and update the
`AUTOMATION_TOKEN` secret. `update-deps.yml` fails loudly if it lapses.

**Watch for `needs-zellij-upgrade`.** Those accumulate rather than merge, and
they are the ones that matter.

## When something breaks

| Symptom | Cause |
|---|---|
| Update workflow fails immediately | `AUTOMATION_TOKEN` missing or expired |
| Pull request opens but no checks run | Opened with `GITHUB_TOKEN`; the token is not being picked up |
| Approved, but never merges | Auto-merge not allowed, or no branch protection rule |
| Release does not publish after merge | Version in `Cargo.toml` already tagged — check the run's `Resolve version` step |
| `cargo test` fails to link | OpenSSL headers missing; the workflows install `libssl-dev`, locally use your package manager |
| Nightly is stale | Check the `nightly.yml` schedule ran; scheduled workflows are paused after 60 days of repository inactivity |

That last one is worth knowing: **GitHub disables scheduled workflows in
repositories with no activity for 60 days**, and emails you when it does. Any
push re-enables them.
