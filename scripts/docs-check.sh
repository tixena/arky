#!/usr/bin/env bash
#
# Gate for the arky book. Builds it and checks the structure of its pages.
# Run from anywhere: bash scripts/docs-check.sh

set -uo pipefail

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
docs_dir="$repo_root/docs"
src_dir="$docs_dir/src"
summary="$src_dir/SUMMARY.md"

failures=0

fail() {
    printf 'FAIL: %s\n' "$1" >&2
    failures=$((failures + 1))
}

finish_with_failures() {
    printf 'docs-check: %d problem(s)\n' "$failures" >&2
    exit 1
}

link_targets() {
    grep -oE '\]\([^)]+\)' <<< "$1" | sed -e 's/^](//' -e 's/)$//'
}

if [[ ! -f "$summary" ]]; then
    fail "docs/src/SUMMARY.md is missing"
    finish_with_failures
fi

# Every link target in SUMMARY.md names a chapter file that has to exist. This
# runs before the build because mdBook creates a missing chapter file instead of
# reporting it.
chapter_count=0
while IFS= read -r target; do
    target=${target%%#*}
    [[ -z "$target" ]] && continue
    chapter_count=$((chapter_count + 1))
    if [[ ! -f "$src_dir/$target" ]]; then
        fail "SUMMARY.md lists src/$target, which does not exist"
    fi
done < <(link_targets "$(cat "$summary")")

if [[ $chapter_count -eq 0 ]]; then
    fail "SUMMARY.md lists no chapters"
fi

[[ $failures -ne 0 ]] && finish_with_failures

if ! mdbook build "$docs_dir"; then
    fail "mdbook build docs returned an error"
    finish_with_failures
fi

# Page structure. SUMMARY.md is the table of contents rather than a page: its
# part titles are level-one headings by mdBook's own convention, and it makes no
# claims that carry a status.
page_count=0
while IFS= read -r page; do
    rel=${page#"$repo_root"/}
    page_dir=$(dirname "$page")
    page_count=$((page_count + 1))

    if [[ "$(head -n 1 "$page")" == "---" ]]; then
        fail "$rel starts with a frontmatter fence; mdBook renders it as body text"
    fi

    headings=0
    in_fence=0
    fence_marker=""
    while IFS= read -r line || [[ -n "$line" ]]; do
        trimmed=${line#"${line%%[![:space:]]*}"}
        if [[ $in_fence -eq 1 ]]; then
            [[ "$trimmed" == "$fence_marker"* ]] && in_fence=0
            continue
        fi
        case "$trimmed" in
            '```'*) in_fence=1; fence_marker='```'; continue ;;
            '~~~'*) in_fence=1; fence_marker='~~~'; continue ;;
        esac

        [[ "$line" == "# "* ]] && headings=$((headings + 1))

        while IFS= read -r target; do
            case "$target" in
                http://*|https://*|mailto:*|/*|'#'*|'') continue ;;
            esac
            path=${target%%#*}
            [[ -z "$path" ]] && continue
            [[ "$path" != *.md ]] && continue
            if [[ ! -f "$page_dir/$path" ]]; then
                fail "$rel links to $target, which does not exist"
            fi
        done < <(link_targets "$line")
    done < "$page"

    [[ $headings -ne 1 ]] && fail "$rel has $headings level-one heading(s); exactly one is required"
done < <(find "$src_dir" -type f -name '*.md' ! -name 'SUMMARY.md' | sort)

[[ $failures -ne 0 ]] && finish_with_failures

# Professional-voice guard: the book never mentions people, internal projects,
# design documents, dates, or review apparatus. It documents the tool.
banned='Eduardo|Burgos|aa_tool|vigil|marginalia|2026-|design corpus|root design|planning document|vision blueprint|feature ledger|grammar document|requirements document|remargin_vision|src/tixena|\*\*Status:\*\*|\*\*Decided|\*\*Proposed|Suggestion \(documentation author\)|the corpus'
if grep -rnE "$banned" "$src_dir" --include='*.md'; then
    fail "banned token found (see lines above): the book must read as tool documentation"
fi

[[ $failures -ne 0 ]] && finish_with_failures

printf 'PASS: %d pages checked, %d chapters listed, book built.\n' "$page_count" "$chapter_count"
