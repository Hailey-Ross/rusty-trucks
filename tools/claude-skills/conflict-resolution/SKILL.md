---
name: conflict-resolution
description: Resolve git merge conflicts between branches and PR branches without losing anything - reuse resolutions already made (an integration branch, an earlier merge), stack conflicting PRs so they merge in order, and prove afterwards that no line from any branch was dropped and the tests still pass. Use whenever a merge, sync or PR combination conflicts, or before claiming branches "merge cleanly together".
---

# Conflict resolution

Goal: branches that are meant to land together merge without conflicts, and no branch's change is lost. Don't
delete any branch until the merged result is verified.

Helper scripts are in `tools/` next to this file (installed under `.claude/skills/conflict-resolution/tools/`).

## 1. Look before merging
- Pairwise conflict map without touching a worktree:
  `git merge-tree --write-tree --name-only A B` (conflicted files are listed after the tree id).
- Find an existing resolution: an integration branch that already merges the same branches. A file is safe to take
  wholesale from it only if no unrelated commit touched it there:
  `git rev-list --no-merges <integration> --not <all source branches> -- <file> | wc -l` must be 0.
- Each worktree owns one branch (`git worktree list`); merge in that branch's worktree, never `git stash`.

## 2. Resolve
In order of preference:
1. **Take the file from a reference that merges exactly the same commits** (`git checkout <ref> -- <file>`).
   Check: after committing, `git diff <ref> HEAD` is empty or only differs by what the reference has extra.
   Trap: an integration branch can carry a LATER branch's lines; a later merge brings them in properly, but the
   intermediate branch must not have them.
2. **Hunk-level reuse:** `python .claude/skills/conflict-resolution/tools/resolve_like.py <ref> <files>` accepts only
   "ours+theirs" / "theirs+ours" with exact context found in the reference; the rest stays as a conflict for you.
   Never loosen it: a fuzzy version once kept one side only and dropped a doc section, an SDK doc block and two
   `validate()` arms.
3. **By hand:** keep both sides' intent. Lists/match arms/`mod` lines: keep both. A line both branches edited (for
   example a macro field list or a system chain tuple): merge the fields of both into one line.
Before committing: `git grep -n -E '^(<<<<<<<|>>>>>>>) '` must be empty; watch for dropped braces.

## 3. Prove nothing was lost (mandatory)
- `python .claude/skills/conflict-resolution/tools/check_lines_kept.py <base> <result> <branch heads...>` with
  `<base>` = the common parent branch of the sources (not `main` when they sit on another PR). Use the OLD heads of
  branches you just moved (record them before merging). Read every hit: only same-line combinations and deliberate
  rewrites are allowed.
- Run it from inside a worktree (outside a repo it sees no files; it exits 2 when it has no files).
- **Semantic conflicts** (merge is clean, build is not: one branch adds a parameter to a function, another adds a
  call with the old arity). When a reference already merges the same branches, list its merge-only fixes: every
  file whose reference history has only source-branch commits must be identical to ours:
  `for f in $(git diff --name-only HEAD <ref>); do [ $(git rev-list --no-merges <ref> --not <old heads> -- $f | wc -l) = 0 ] && echo $f; done`
  then take those files from the reference.
- Every old head is an ancestor: `git merge-base --is-ancestor <old-head> <result>`.
- Build and test the result (skill `build-and-run`; know the pre-existing upstream failures), and when audio
  changed, the e2e bench must stay IDENTICAL (skill `optimisation`).

## 4. PRs that conflict with each other
- Two open PRs touching the same lines cannot both merge cleanly in any order. Either stack them (the later PR
  merges the earlier one, its description says "merges after #N") or combine them into one PR. Maintainers usually
  prefer fewer, themed PRs; ask when unclear.
- After moving a PR branch: update its description so it matches the branch.
- Close PRs or delete branches only after section 3 passes on the pushed result; `gh pr close` without
  `--delete-branch`; delete branches only with the owner's agreement.

## 5. Commits
Merge commits are commits: sign them like any other commit if the project signs, and only make them when asked.
Amend a merge only while it is unpushed; after a push, fix forward.
