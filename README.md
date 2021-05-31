# Fisherman

`fisherman` is a tool for continuous delivery of Rust projects.

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

By default, `fisherman` expects messages to reach it on port `5000`, although
this can be changed in the configuration file. The location of repositories is
defined by the `repo_root` field in the configuration file. Repositories are
also assumed to use SSH, and the private key at `ssh_private_key` will be used
for authentication.

### Configuration

Configuration for `fisherman` is defined by the `fisherman.yml` file and has
the following structure:

```yaml
default:
    ssh_private_key: "path to SSH key for authentication"
    repo_root: "top level directory where repositories are stored"
    cargo_path: "path to binary for cargo"
    secret: "globally used default secret"
    port: "port to listen on, defaults to 5000"

specific:
    alexander-jackson/fisherman:
        secret: "specific secret value"

    FreddieBrown/dodona:
        follow: "develop"
        code_root: "/backend"
        binaries: ["api-server", "dcl"]
```

### Discord Integration

`fisherman` supports sending messages to a Discord channel when a repository
successfully updates. This requires a Discord token and a channel identifier to
send the messages to, and will inform you of the commit hash that it was
updated to and the associated commit message.

This can be set as follows in the configuration file:
```yaml
default:
    discord:
        token: "<token>"
        channel_id: <channel_id>
```
