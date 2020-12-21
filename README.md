# Fisherman

`fisherman` is a tool for continous delivery of Rust projects.

It can be run on a remote server, where it will listen for webhook events being
sent to it. If these involve pushes to the `master_branch` of a repository, it
will fetch and merge the changes, rebuild the binary and allow `supervisor` to
restart execution. This allows changes to be automated by pull requests or
pushes.

## Installation

`fisherman` can be setup easily by cloning the repository and building it in
release mode, before adding it to `supervisor`'s execution tasks.

```bash
git clone git@github.com:alexander-jackson/fisherman.git

cd fisherman/
cargo build --release
```

## Usage

Currently, `fisherman` expects messages to reach it on port `5000`, and for
repositories to be located under `/root/*`. It also assumes that repositories
were cloned using SSH, and will rely on this for authentication.
