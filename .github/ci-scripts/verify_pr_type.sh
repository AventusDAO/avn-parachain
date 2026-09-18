#!/bin/bash

description="$1"
event_type="$2"

RELEASE=$([[ -n "$(echo "$description" | grep -oe '- \[x\] Release')" ]] && echo true || echo false)
INCREASE_VERSIONS=$([[ -n "$(echo "$description" | grep -oe '- \[x\] Increase versions')" ]] && echo true || echo false)

if [[ -n "$(echo -e "$description" | grep -woe '\- \[x\] \(Major\|Minor\|Patch\) release')" ]]; then
    SEM_VERSION_TYPE=$(echo -e "$description" | grep -woe '\- \[x\] \(Major\|Minor\|Patch\) release'| grep -woe '\(Major\|Minor\|Patch\)')
else
    SEM_VERSION_TYPE='noVersion'
fi

if [[ "$event_type" == "pull_request" ]]; then
    # VAR CHECKS
    if $RELEASE; then
        [[ "$(echo "$SEM_VERSION_TYPE" | wc -l | tr -d " ")" != "1" ]] && \
          echo "multiple release types...exiting with error" && exit 1

        if [[ "$SEM_VERSION_TYPE" == "noVersion" ]]; then
            echo "You need to ensure a release type was selected."
            echo "exiting..."
            exit 1
        fi
    else
        if $INCREASE_VERSIONS || [[ "$SEM_VERSION_TYPE" != "noVersion" ]] ; then
            echo "Can't update spec_version or create a release tag without turning this PR into a release type!"
            echo "exiting..."
            exit 1
        fi
    fi
elif [[ "$event_type" == "push" ]]; then
    {
        echo "RELEASE=$RELEASE"
        echo "INCREASE_VERSIONS=$INCREASE_VERSIONS"
        echo "SEM_VERSION_TYPE=$SEM_VERSION_TYPE"
    } >> "$GITHUB_OUTPUT"
fi

{
    echo "RELEASE=$RELEASE"
    echo "INCREASE_VERSIONS=$INCREASE_VERSIONS"
    echo "SEM_VERSION_TYPE=$SEM_VERSION_TYPE"
} >> "$GITHUB_ENV"

#printing vars
echo "##### VARIABLES ####"
echo "Release: $RELEASE"
echo "Increase versions: $INCREASE_VERSIONS"
echo "Semantic Version: $SEM_VERSION_TYPE"
echo ###################
