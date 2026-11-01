#!/usr/bin/env bash
# The STEP diff action's work (action.yml): install specify-core, compare each STEP file the
# pull request changes with the base branch's, publish the pictures and write the comment.
set -euo pipefail

if [ -z "${PR:-}" ]; then
  echo "::error::The STEP diff action runs on pull_request events."
  exit 1
fi
if [ "$(uname -s)-$(uname -m)" != Linux-x86_64 ]; then
  echo "::error::The STEP diff action runs on ubuntu-latest (Linux x86_64)."
  exit 1
fi
if [ "$HEAD_REPO" != "$REPO" ]; then
  echo "::notice::A pull request from a fork has a read-only token, so the STEP diff action can't comment on it."
  exit 0
fi

# The release: the one asked for, the action's own tag, or the latest with a Linux build.
release() { echo "specify-core-$1-x86_64-unknown-linux-gnu"; }
if [ -z "$VERSION" ]; then
  case "$ACTION_REF" in
    v[0-9]*) VERSION=$ACTION_REF ;;
    *)
      for tag in $(gh release list -R pzfreo/specify-core -L 5 --exclude-drafts --json tagName --jq '.[].tagName'); do
        if gh release view "$tag" -R pzfreo/specify-core --json assets --jq '.assets[].name' \
          | grep -qx "$(release "$tag").tar.gz"; then
          VERSION=$tag
          break
        fi
      done
      ;;
  esac
fi
name=$(release "$VERSION")
gh release download "$VERSION" -R pzfreo/specify-core -p "$name.tar.gz" -p "$name.tar.gz.sha256" -D "$RUNNER_TEMP"
(cd "$RUNNER_TEMP" && sha256sum -c "$name.tar.gz.sha256" && tar -xzf "$name.tar.gz")
PATH="$RUNNER_TEMP/$name:$PATH"

# What the pull request changes: the checkout is its merge commit, against the base it merges
# into. Each part is a kind (added, removed, changed), its old path and its new path. Only an
# exact rename is one: STEP files are alike enough as text for git to pair unrelated parts.
BASE=$(git rev-parse HEAD^1)
is_step() { case "${1,,}" in *.step | *.stp) return 0 ;; esac; return 1; }
kinds=() olds=() news=() answered=()
declare -A listed
git -c core.quotePath=false diff -z --name-status -M100% "$BASE" HEAD > "$RUNNER_TEMP/changes"
while IFS= read -r -d '' status; do
  IFS= read -r -d '' old
  new=$old
  if [[ $status == R* ]]; then IFS= read -r -d '' new; fi
  if is_step "$new" || is_step "$old"; then
    case "$status" in
      A) kinds+=(added) ;;
      D) kinds+=(removed) ;;
      *) kinds+=(changed) ;;
    esac
    olds+=("$old") news+=("$new")
    listed[$new]=1
  elif [[ $new == *.answers.json ]]; then
    answered+=("${new%.answers.json}")
  fi
done < "$RUNNER_TEMP/changes"
# A part whose answers changed but not its STEP file is compared too.
for stem in ${answered[@]+"${answered[@]}"}; do
  for f in "$stem".{step,stp,STEP,STP}; do
    if [ -f "$f" ] && [ -z "${listed[$f]:-}" ] && git cat-file -e "$BASE:$f" 2> /dev/null; then
      kinds+=(changed) olds+=("$f") news+=("$f")
      listed[$f]=1
    fi
  done
done

uri() { jq -rn --arg s "$1" '$s | @uri'; }
links=()
outs="$RUNNER_TEMP/step-diff"
mkdir -p "$outs"
for i in "${!kinds[@]}"; do
  out="$outs/$i"
  mkdir -p "$out"
  [ "${kinds[$i]}" = changed ] || continue
  old=${olds[$i]} new=${news[$i]}
  title=$new
  [ "$old" = "$new" ] || title="$new (was $old)"
  git show "$BASE:$old" > "$out/old.step"
  # The answers as they were on the base branch are the ones to carry.
  answers=()
  if git cat-file -e "$BASE:${old%.*}.answers.json" 2> /dev/null; then
    git show "$BASE:${old%.*}.answers.json" > "$out/answers.old.json"
    answers=(--answers "$out/answers.old.json")
  fi
  # A public repository's revisions can be compared live in the browser, at draftwright.io: the
  # picture links there, and an assembly's changed parts link there each with &part=.
  viewer=()
  if [ "$PRIVATE" != true ]; then
    link="https://draftwright.io/diff?repo=$REPO&base=$BASE&head=$PR_HEAD&path=$(uri "$new")"
    [ "$old" = "$new" ] || link+="&was=$(uri "$old")"
    links[i]=$link
    viewer=(--viewer "$link")
  fi
  if ! specify-core diff "$out/old.step" "./$new" ${answers[@]+"${answers[@]}"} ${viewer[@]+"${viewer[@]}"} --title "$title" -o "$out" 2> "$out/error"; then
    cat "$out/error" >&2
    kinds[$i]=failed
  fi
done

# The pictures, on the step-diff-images branch, for the comment to show.
dir="pr-$PR/${PR_HEAD:0:7}"
pictures=no
if [ ${#kinds[@]} -gt 0 ]; then
  git worktree add -q --detach "$RUNNER_TEMP/images"
  cd "$RUNNER_TEMP/images"
  if git ls-remote --exit-code origin refs/heads/step-diff-images > /dev/null; then
    git fetch -q origin step-diff-images && git checkout -q -B step-diff-images FETCH_HEAD
  else
    git checkout -q --orphan step-diff-images && git rm -rq --cached . && git clean -fdq
  fi
  mkdir -p "$dir"
  for i in "${!kinds[@]}"; do
    if [ -f "$outs/$i/model.png" ]; then cp "$outs/$i/model.png" "$dir/$i.png"; fi
  done
  git add "$dir" 2> /dev/null || true
  if git -c user.name=github-actions -c user.email=github-actions@users.noreply.github.com \
    commit -qm "Pictures for #$PR at ${PR_HEAD:0:7}"; then
    # Another pull request's pictures may land first: each writes its own directory.
    for try in 1 2 3 4; do
      if git push -q origin HEAD:refs/heads/step-diff-images; then
        pictures=yes
        break
      fi
      git fetch -q origin step-diff-images && git rebase -q FETCH_HEAD
    done
  else
    pictures=yes
  fi
  cd - > /dev/null
fi

# One comment, found by its marker and rewritten on each push.
marker='<!-- step-diff -->'
body="$RUNNER_TEMP/comment.md"
{
  echo "$marker"
  for i in "${!kinds[@]}"; do
    out="$outs/$i" old=${olds[$i]} new=${news[$i]}
    case "${kinds[$i]}" in
      added) printf '### %s: Specify\n\nA new file: nothing to compare it with.\n\n' "$new" ;;
      removed) printf '### %s: Specify\n\nThe file was removed.\n\n' "$old" ;;
      failed)
        printf '### %s: Specify\n\nSpecify could not compare it:\n\n```\n' "$new"
        tail -n 20 "$out/error"
        printf '```\n\n'
        ;;
      changed)
        report=$(< "$out/report.md")
        viewer=${links[i]:-}
        assembly=no
        if jq -e 'has("parts")' "$out/report.json" > /dev/null; then assembly=yes; fi
        if [ "$pictures" = yes ]; then
          image="https://github.com/$REPO/blob/step-diff-images/$dir/$i.png?raw=true"
          if [ -n "$viewer" ]; then
            report=${report/"!["/"[!["}
            report=${report//"](model.png)"/"]($image)]($viewer)"}
          else
            report=${report//"](model.png)"/"]($image)"}
          fi
        fi
        printf '%s\n' "$report"
        what="the part"
        if [ $assembly = yes ]; then what="the assembly"; fi
        sidecar="${new%.*}.answers.json"
        stale=no
        if [ -f "$sidecar" ] && ! jq -e -s '.[0] == .[1]' "$out/answers.json" "$sidecar" > /dev/null; then
          stale=yes
        fi
        if [ "$stale" = yes ]; then
          printf '\n`%s` is stale: its requirements differ from this comparison.' "$sidecar"
          if [ -n "$viewer" ]; then printf ' [Review in 3D](%s).' "$viewer"; fi
          printf '\n'
        elif [ -n "$viewer" ]; then
          printf '\n[Turn %s round in 3D](%s), compared live in the browser.\n' "$what" "$viewer"
        fi
        echo
        ;;
    esac
  done
  if [ ${#kinds[@]} -eq 0 ]; then printf 'This pull request no longer changes a STEP file.\n\n'; fi
  echo "<sub>Compared by [specify-core](https://github.com/pzfreo/specify-core) $VERSION at ${PR_HEAD:0:7}.</sub>"
} > "$body"
# GitHub refuses a comment over 65,536 characters.
if [ "$(wc -m < "$body")" -gt 65000 ]; then
  cat "$body"
  { head -c 60000 "$body"; printf '\n\n…the report is cut short here; the whole of it is in the job log.\n'; } > "$body.short"
  mv "$body.short" "$body"
fi
id=$(gh api "repos/$REPO/issues/$PR/comments" --paginate \
  --jq ".[] | select(.body | startswith(\"$marker\")) | .id" | sed -n 1p)
if [ -n "$id" ]; then
  gh api -X PATCH "repos/$REPO/issues/comments/$id" -F body=@"$body" > /dev/null
elif [ ${#kinds[@]} -gt 0 ]; then
  gh api -X POST "repos/$REPO/issues/$PR/comments" -F body=@"$body" > /dev/null
fi
