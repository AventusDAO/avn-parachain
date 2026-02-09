# Upgrading to newer version of Polkadot SDK

## Inspect the changes to the template

Use the GitHub compare view to inspect changes between two releases of the parachain template. For example, to compare the `polkadot-stable2412-11` and `polkadot-stable2503-11` tags:
- https://github.com/paritytech/polkadot-sdk/compare/polkadot-stable2412-11...polkadot-stable2503-11

You can choose to port the changes manually or generate a patch file to apply.

## Generating a patch file

If you want to create a diff that you can apply, run the following after you check out the [polkadot-sdk](https://github.com/paritytech/polkadot-sdk/) codebase. The parachain template lives under `templates/parachain` in this repo:
```sh
# Shows all the changes between the two releases
git diff polkadot-stable2412-11 polkadot-stable2503-11
# Shows the names of the files changed between the two releases
git diff polkadot-stable2412-11 polkadot-stable2503-11 --name-only
# Show the changes of some paths between two releases
git diff polkadot-stable2412-11 polkadot-stable2503-11 -- path_1 path_2
# For example to show only the diff under templates/parachain and templates/parachain/node/Cargo.toml run:
git diff polkadot-stable2412-11 polkadot-stable2503-11 -- templates/parachain templates/parachain/node/Cargo.toml
# Stores all changes in a patch file
git diff polkadot-stable2412-11 polkadot-stable2503-11 -- templates/parachain > polkadot-stable2412-11_to_polkadot-stable2503-11_upgrade.diff
```
Generate a diff that you prefer, including the files you want to apply. To apply it go to the avn-node-parachain-repo and run the following:
```sh
git apply <path_to_diff>/polkadot-stable2412-11_to_polkadot-stable2503-11_upgrade.diff --reject --ignore-whitespace
```
This will apply the changes that have no conflicts and create .rej files for the ones that could not be applied automatically.
Then inspect the .rej files and rectify case by case.

Once completed commit the changes and ensure the project builds.

## Using psvm to select the upgrade target

You can use [psvm](https://crates.io/crates/psvm) to list the available Polkadot SDK versions and select the one you need for the upgrade. This automatically manages the polkadot-sdk dependencies and versions for the workspace.

```sh
# Ensure psvm is up to date
cargo install psvm

# List available versions
psvm --list

# Select the target version for the upgrade
psvm -v polkadot-stable2503-11

```
