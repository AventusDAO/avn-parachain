# AvN Node

![image](./res/aventus.jpg)

Aventus Network belongs to the new generation of composable blockchain networks built for scalability and interoperability 🚀. It is capable of high transaction throughput and low and predictable transaction costs. The network currently operates as a parachain to Polkadot.

## Open Sourcing

The purpose of this repo is to gradually open-source the Aventus parachain code and also give the community the opportunity to contribute to the Aventus codebase either by providing enhancements to the already available code or contributing new pallets.

We are very keen on community engagement and contributions.

## Contributions Guidelines

We welcome contributions but before you devote quite a bit of time to contributing, you should make sure you're certain your contribution has not already been addressed.

Read our contribution guide [HERE](./CONTRIBUTING.adoc).

Note: This repository is managed frequently so you do not need to email/contact us to notify us of your submission.

## Parachains introduction and tutorials
This project is originally a fork of the
[Substrate Node Template](https://github.com/substrate-developer-hub/substrate-node-template)
modified to include dependencies required for registering this node as a **parathread** or
**parachain** to a **relay chain**.

👉 Learn more about parachains [here](https://wiki.polkadot.network/docs/learn-parachains), and
parathreads [here](https://wiki.polkadot.network/docs/learn-parathreads).

## Building the project
*Based on [Polkadot Build Guide](https://github.com/paritytech/polkadot#building)*

First [Install Rust](https://www.rust-lang.org/tools/install):

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
# set default cargo location
source $HOME/.cargo/env
```

If you already have Rust installed, make sure you're using the latest version by running:

```bash
rustup update
```

Once done, finish installing the support software and configure your default toolchain:

```bash

# Additional OS dependencies
sudo apt install build-essential
sudo apt install --assume-yes git clang curl libssl-dev protobuf-compiler
```

Verify the configuration of your development environment by running the following command:
```bash
rustup show
rustup +nightly show
```
The command displays output similar to the following:
```bash
# rustup show

active toolchain
----------------

stable-x86_64-unknown-linux-gnu (default)
rustc 1.62.1 (e092d0b6b 2022-07-16)

# rustup +nightly show

active toolchain
----------------

nightly-x86_64-unknown-linux-gnu (overridden by +toolchain on the command line)
rustc 1.65.0-nightly (34a6cae28 2022-08-09)
```
See [here](https://docs.substrate.io/install/linux/) for a more detailed guide on installing Rust and the required dependecies.

Build the client by cloning this repository and running the following commands from the root
directory of the repo:

```bash
git checkout <latest tagged release>
cargo build --release
```

### Enabling avn optional features

[The Cargo Book - Features](https://doc.rust-lang.org/cargo/reference/features.html) section explains how features and feature flags can be used in cargo to enable additional features when building a project.

```bash
# single feature
cargo build --features feature_name
# multiple features
cargo build --features feature_name_1,feature_name_2
```

#### Activating the Test Runtime

The avn test runtime is an independent runtime that integrates the newest features of AvN, currently undergoing development and testing. By default, the test runtime is not built, but you can enable it using one of the following feature flags:

- `avn-test-runtime`: Enables the compilation and chainspecs that utilize the test runtime
- `test-native-runtime`: Switches the native runtime of the node to use the test runtime. This feature implies the `avn-test-runtime` feature.

## Building and testing the pallets

To build and test a pallet, navigate to the directory of the pallet in your project by running the following command:
```sh
cd pallets/<pallet_name>/
```
Once you're in the pallet directory, use cargo to build and test the pallet by running the following commands:
```sh
# Build the pallet
cargo build

# Run the unit and integration tests
cargo test

# Test the benchmark tests
cargo test --features runtime-benchmarks
```
AvN binaries are built on Ubuntu 20.04 (focal), which is a Long Term Support (LTS) version. Consequently, these binaries may rely on certain native libraries such as OpenSSL. While Debian bullseye-based operating systems should be compatible, we highly recommend using Ubuntu 20.04 for running a node binary.

## Storage migration tests
If you are working on functionality that requires storage migration, you must test the migration logic using the chain state of your staging and/or production chain to ensure you don't have any unexpected errors due to the state being different. To perform this test you should:
1. Compile the code using the try-runtime feature by running: \
`cargo b -r --features try-runtime` \
Using the new binary that was built from the script above, you can now do a dry run of the migration

2. `./<avn-binary> try-runtime --runtime <path to new wasm that was built on step 1> on-runtime-upgrade --checks live --uri <websocket url of staging/production chain with port number>`

You can enable more logging by prefixing the command with `RUST_LOG=info,runtime=debug ` \
If you pass in the `--checks` options, this will execute the `pre_upgrade` and `post_upgrade` functions of your migration code.

 - `pre_upgrade`: Function that runs before the storage migration has executed. This function can return data.
 - `post_upgrade`: Function that runs after the storage migration has executed. This functiion will take the output of pre_upgrade and can use it to validate the migration. \

When possible, it is recommended to implement these functions to verify the outcome of the migration.

Example implementation:

```rsut
#[cfg(feature = "try-runtime")]
fn pre_upgrade() -> Result<Vec<u8>, TryRuntimeError> {
    Ok(vec![])
}

#[cfg(feature = "try-runtime")]
fn post_upgrade(input: Vec<u8>) -> Result<(), TryRuntimeError> {
    Ok(())
}
```

## Building the docker image
When building an image, make sure the binary is located under the target/release folder.
If you are using the official releases or have built binaries locally using Ubuntu 20.04:
```sh
docker build . --tag avn-node-parachain:latest
```

If you have built binaries locally using Ubuntu 22.04:
```sh
docker build -f Dockerfile.22_04 . --tag avn-node-parachain:latest
```

## Logs

When running a node, various log messages are displayed in the output. Each log has a sensitivity level, such as `error`, `warning`, `info`, `debug`, or `trace` and is associated with a specific target.

The following CLI options can be used to configure logging:
```
  -l, --log <LOG_PATTERN>...
          Sets a custom logging filter. Syntax is `<target>=<level>`, e.g. -lsync=debug
  --detailed-log-output
      Enable detailed log output
```

Enabling the `detailed-log-output` flag provides more comprehensive log information, including the log target, log level, and the name of the emitting thread. If no target is specified, the name of the module will be used.

Here are some examples of log statements and their corresponding outputs:

```Rust
log::info!(target: "aventus", "💾 Sample log");
```
Output:

```console
2023-05-15 08:00:00 💾 Sample log
```
Output with detailed log output enabled:
```console
2023-05-15 08:00:00  INFO main aventus: [Parachain] 💾 Sample log
```
During node execution, you have the flexibility to modify the log level for all or specific targets using the following command line parameters:
```
# Setting the log level to all targets
-ldebug
--log debug

# Setting the log level to specific targets.
-ltxpool=debug -lsub-libp2p=debug
```
There are numerous log targets available, and you can discover more by utilizing the detailed-log-output parameter or by referring to the code. However, for convenience, here are some commonly used log targets:

- txpool
- avn-service
- sub-libp2p
- sub-authority-discovery
- parachain::collator-protocol
- parachain::validator-discovery
- gossip
- peerset
- cumulus-collator
- db
- executor
- wasm-runtime
- sync
- offchain-worker::http
- state-db
- state

*Please note that this list is not exhaustive*.

### Log output manipulation using environment variables

Alternatively, you can use the `RUST_LOG` environment variable to specify the desired log level per module. Substrate utilizes the [log crate](https://github.com/rust-lang/log) and [env_logger crate](https://docs.rs/env_logger/latest/env_logger/) for its internal logging implementation. These crates provide a flexible and configurable way to manage log output through the use of the `RUST_LOG` environment variable.

- [Substrate - Debug](https://docs.substrate.io/test/debug/)
- [Rust - Enabling logs per module](https://rust-lang-nursery.github.io/rust-cookbook/development_tools/debugging/config_log.html)

## Chainspec Generation

To generate the necessary chainspec files for your parachain, simply replace `<chain_name>` with your specific configuration name and execute these commands. This will produce the essential files for your parachain's configuration and genesis state.
```sh
# Generate the plain text version
./avn-parachain-collator build-spec --chain <chain_name> --disable-default-bootnode > avn_chain_plain.json

# Generate the raw version
./avn-parachain-collator build-spec --chain avn_chain_plain.json --disable-default-bootnode --raw > avn_chain_raw.json
```
## Export Genesis State and Wasm

To export the genesis state and wasm files, use the following commands:

```sh
# Export the genesis wasm (example <chain_name>: avn_chain_plain.json)
./avn-parachain-collator export-genesis-wasm --chain <chain_name> --raw > genesis.wasm

# Export the genesis state (example <chain_name>: avn_chain_plain.json)
./avn-parachain-collator export-genesis-state --chain <chain_name> --raw > genesis.state
```
