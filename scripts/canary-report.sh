#!/usr/bin/env bash
# canary-report.sh <results-dir> <state-dir>
# The weekly canary's last step (spec §19). <results-dir> holds each part's
# `<part>.result` and `<part>.versions.json`, and `update.from.json` when the Update
# started from a record. Writes <state-dir>/versions.json, which the next run's Update
# starts from, then opens one issue for each failed part, or comments on the open one.
set -euo pipefail

results=$1
state=$2
: "${GITHUB_REPOSITORY:?}" "${GITHUB_SHA:?}" "${GITHUB_REF_NAME:?}" "${RUN_URL:?}"

failed=()
for part in fresh-install update memory end-to-end; do
  result=$(cat "$results/$part.result" 2>/dev/null || echo missing)
  echo "$part: $result"
  if [[ $result != success && $result != skipped ]]; then
    failed+=("$part")
  fi
done

mkdir -p "$state"
if ((${#failed[@]} == 0)); then
  cp "$results/update.versions.json" "$state/versions.json"
  exit 0
fi
if [[ -f $results/update.from.json ]]; then
  cp "$results/update.from.json" "$state/versions.json"
fi

versions() {
  if [[ -s $1 ]]; then
    jq -r 'to_entries[] | select(.value != null) | "- \(.key) \(.value.version)"' "$1"
  else
    echo "- $2"
  fi
}

latest_releases() {
  local repo tag
  for repo in elie222/rakazo vectorize-io/hindsight lemonade-sdk/lemonade; do
    tag=$(gh api "repos/$repo/releases/latest" --jq .tag_name) || tag=unknown
    echo "- $repo $tag"
  done
}

body() {
  local part=$1
  echo "The weekly canary's **$part** part failed."
  echo
  echo "- Run: $RUN_URL"
  echo "- Commit: $GITHUB_SHA on \`$GITHUB_REF_NAME\`"
  if [[ ${FORCED:-} == "$part" ]]; then
    echo "- Forced by the \`fail\` input of a manual run."
  fi
  if [[ $part == update ]]; then
    echo
    echo "Updating from:"
    versions "$results/update.from.json" "no record: the program's floors"
  fi
  echo
  echo "Versions the part's Instance recorded:"
  versions "$results/$part.versions.json" "none: it stopped before writing \`settings.json\`"
  echo
  echo "Latest upstream releases:"
  latest_releases
}

# Only people with triage access can label an issue, so a stranger's issue with the same
# title is never taken for the canary's.
gh label create canary --force --color D93F0B --description "Opened by the weekly canary"

for part in "${failed[@]}"; do
  title="Canary: $part failed"
  open=$(gh issue list --label canary --state open --limit 100 --json number,title |
    jq -r --arg title "$title" 'map(select(.title == $title)) | .[0].number // empty')
  if [[ -n $open ]]; then
    body "$part" | gh issue comment "$open" --body-file -
  else
    body "$part" | gh issue create --label canary --title "$title" --body-file -
  fi
done
