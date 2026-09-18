#!/bin/bash

major="$1"
minor="$2"
patch="$3"
event_type="$4"

if [[ "$SEM_VERSION_TYPE" == "Major" ]]; then
    NEW_PACKAGE_TAG="$major"
elif [[ "$SEM_VERSION_TYPE" == "Minor" ]]; then
    NEW_PACKAGE_TAG="$minor"
elif [[ "$SEM_VERSION_TYPE" == "Patch" ]]; then
    NEW_PACKAGE_TAG="$patch"
fi

echo "new tag to be used is: $NEW_PACKAGE_TAG"
echo "NEW_PACKAGE_TAG=$NEW_PACKAGE_TAG" >> "$GITHUB_OUTPUT"

[[ "$event_type" == "push" ]] && exit 0

git config --global user.name aventus-ci-agent
git config --global user.email ci-agent-bot@aventus.io

# The checkout runs with `persist-credentials: false`, so the token is supplied
# explicitly here and only for the push. It never lands in `.git/config` for the
# build and test jobs.
if [[ -n "${GITHUB_TOKEN:-}" ]]; then
    git remote set-url --push origin \
        "https://x-access-token:${GITHUB_TOKEN}@github.com/${GITHUB_REPOSITORY}.git"
fi

if $INCREASE_VERSIONS; then
    git checkout main
    CURRENT_SPEC_VERSION=$(grep -Eow "spec_version: [0-9]+" runtime/avn/src/lib.rs | grep -Eow "[0-9]+")
    let NEW_SPEC_VERSION=$CURRENT_SPEC_VERSION+1

    git checkout "${GITHUB_HEAD_REF}"

    # avn runtime
    sed -i "s@$REGEX_SPEC_VERSION@\1\2$NEW_SPEC_VERSION@" runtime/avn/src/lib.rs
    sed -i "s@$REGEX_IMPL_VERSION@\1\20@" runtime/avn/src/lib.rs
    # test runtime: for now, always follow the avn releases versions
    sed -i "s@$REGEX_SPEC_VERSION@\1\2$NEW_SPEC_VERSION@" runtime/test/src/lib.rs
    sed -i "s@$REGEX_IMPL_VERSION@\1\20@" runtime/test/src/lib.rs

    COMMIT_MESSAGE="cargo package, spec and impl versions increased"
else
    COMMIT_MESSAGE="cargo package, spec and impl versions increased"
fi

git checkout "${GITHUB_HEAD_REF}"

cargo set-version --workspace $NEW_PACKAGE_TAG

git add .
git commit -m "$COMMIT_MESSAGE" || exit 0
git push

git push
