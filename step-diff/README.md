# STEP diff action

A pull request that changes a STEP file gets a comment saying what changed in the part, in
words a person can check against the model, with a picture of the new part coloured by what
changed. If the part has been specified with [Specify](../README.md), the comment also carries
its manufacturing intent (material, fits, threads, datums, tolerances) to the new revision.

![The comment on a pull request that thickens a flange and shrinks its bolt holes](comment.png)

That comment is on [pzfreo/step-diff-demo#2](https://github.com/pzfreo/step-diff-demo/pull/2).
The [demo repository](https://github.com/pzfreo/step-diff-demo) is a working example to copy.

## Adding it to a repository

1. Add `.github/workflows/step-diff.yml`:

   ```yaml
   name: STEP diff
   on:
     pull_request:
       paths: ['**.step', '**.stp', '**.STEP', '**.STP', '**.answers.json']
   permissions:
     contents: write       # the pictures, on the step-diff-images branch
     pull-requests: write  # the comment
   jobs:
     diff:
       runs-on: ubuntu-latest
       steps:
         - uses: pzfreo/specify-core/step-diff@main
   ```

2. Commit it to the default branch.

The next pull request that changes a STEP file gets the comment. Every later push to that pull
request rewrites the same comment, so there's only ever one. Nothing else is needed: no token, no
secret, no build. The action checks the repository out, downloads the specify-core release and
checks its checksum.

## Reading the comment

- **The summary line** counts what changed by family ("2 holes changed, 8 rounds and surfaces
  with them") and says whether the answers carry. ✅ means every answer carried and the new part
  asks nothing new. ⚠️ means a decision is needed: an answer couldn't be carried, or the part
  asks something new. ❌ means Specify's review finds problems in the new part that it didn't
  find in the old one.
- **The picture** is the new part. Faces that changed are orange, new faces are green, faces
  that can't be told apart from a matching one are purple, and the old part's faces that are
  gone or moved are drawn over it in translucent red.
- **Turn the part round in 3D:** in a public repository, the picture and the line under it link to
  [draftwright.io/diff](https://draftwright.io/diff). That page fetches the two revisions from
  GitHub, compares them in your browser, and shows the part coloured the same way, to turn round
  and zoom. A private repository's comment has the picture only, because the page can't read its
  files.
- **The parts table**, for an assembly, lists the parts that differ: changed, added, removed,
  placed a different number of times, or moved, with a count of the rest. Each changed part
  then gets its own section, read as a part's comment is, with a link that opens that part on
  the /diff page.
- **The measures** are the volume, surface area, bounding box and face count, before and
  after.
- **What changed** names each feature as a drawing would ("2× Ø10 × 11 through holes") and
  places it so it can be found ("on the −Z side, at X 10.9, Y 28.4 from the −X, −Y corner"),
  then says what changed: "depth 11 → 13; diameter 10 → 9", "moved Z +2", "removed".
- **Answers carried to a changed feature** are applied but listed to check. A clearance hole
  that got smaller may no longer clear its bolt.
- **Answers not carried** keep their value and say why: the feature was removed, or the new
  part has several features it could be.
- **New questions** are what the new part asks that the old one didn't, for a new hole, say.
- **PMI** lists the dimensions, tolerances and datums in the STEP files themselves that were
  added, removed or changed.
- **The answers carried to the new revision** are the answers file for the new part, ready to
  commit.

How `diff` matches the two parts and decides what carries is in [docs/diff.md](../docs/diff.md).

## Answers

The answers are the decisions made about the part: its material, what each hole is for, which
faces are datums, the tolerances. The action looks for them in two places:

- **A file beside the part.** For `bracket.step`, the file is `bracket.answers.json`, a JSON object
  mapping question ids to values, as `specify-core write` takes. The base branch's file is the
  one carried. The comment shows the carried answers, renamed where the part's faces
  renumbered, for you to commit as the new revision's file.
- **The STEP file itself.** A STEP file written by Specify, including one exported from the
  Specify editor at [draftwright.io](https://draftwright.io), stores its answers as PMI. When
  there's no answers file, those are carried.

A part with no answers still gets the comment: what changed, the picture and the measures.

## What it compares

- **Changed files:** each STEP file the pull request changes, as it would be once merged, is
  compared with the base branch's version.
- **Answers changed without the STEP file:** the part is compared too, so the comment shows
  the answers again.
- **New and removed files** are listed as new or removed parts.
- **Renames:** a renamed file is compared with its old path when the rename leaves it
  unchanged. Otherwise it's reported as one part removed and another added, because STEP
  files look too alike as text to pair by content.
- **Unreadable files:** a file Specify can't read gets its error in the comment, and the
  other files are still compared.
- **Assemblies:** a file of several parts, or of a part placed more than once, is compared part
  by part. Parts are paired by their names in the STEP file, so a renamed part shows as one
  removed and another added. The comment has the parts table (changed, added, removed, quantity
  changed and moved, plus a count of the unchanged), a section for each changed part, and a
  picture of the whole assembly: changed, re-counted and moved parts orange, added green,
  removed red. In a public
  repository each changed part links to /diff with `&part=` to open it there. An assembly's
  answers file maps part names to their answers: `{"GRM-04 drive plate": {"part.material":
  "6061-T6"}}`. A file that was a single part keeps its old answers file: it is taken as that
  part's.

## Options

```yaml
      - uses: pzfreo/specify-core/step-diff@main
        with:
          version: v0.1.6         # the specify-core release to run
          token: ${{ secrets.MY_TOKEN }}
```

- **`version`:** with the default, `@main` runs the latest specify-core release that has a
  Linux build, and a release tag such as `@v0.1.0` runs that release. Pin a tag, or set
  `version`, to stop the comment changing when a new release comes out.
- **`token`:** pushes the pictures and writes the comment. The default is the job's
  `github.token`, which needs the `permissions` above.

## Where things go

- **Pictures:** these are pushed to an orphan branch, `step-diff-images`, under
  `pr-<number>/<commit>/`, so the comment can show them. The branch shares no history with
  your code and can be deleted at any time; old comments then lose their pictures.
- **The comment:** it is found by a hidden `<!-- step-diff -->` marker at its start and
  rewritten on each push.

## From another web page

A CAD tool's own page can open the same 3D comparison with two files it already has: it opens
https://editor.draftwright.io/diff and posts it the files, which are compared in the browser and
never uploaded. [HANDOFF.md](HANDOFF.md) gives the message and the few lines that send it.

## Limits

- **Runner:** it runs on `ubuntu-latest` (Linux x86_64).
- **Pull requests from forks** get a read-only token, so there the action can't push pictures or
  comment. It passes with a notice saying so.
- **Comment size:** a comment longer than GitHub's 65,536-character limit is cut short; the
  whole report is in the job's log.

## Running the same comparison yourself

The action runs `specify-core diff`. The same report comes from the command line:

```bash
specify-core diff old.step new.step --answers old.answers.json -o changes
```

`changes/` gets `report.md` (the comment), `model.png` (the picture), `report.json` and the
carried `answers.json`. An assembly gives the same four files, its answers by part name.
Installing the command line is described in [the README](../README.md#install).
