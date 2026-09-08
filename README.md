# CommitBook

Automated git commits and sync for your markdown notebooks. Turn any git repository into a self-saving, self-syncing note-taking workspace.

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/CommitBook/CommitBook-Core/main/install.sh | sh
```

This downloads the latest release for your platform (macOS arm64/x86_64, Linux
x86_64/aarch64), verifies it against the published `SHA256SUMS`, and installs
into `~/.local/bin`.

### From source (Rust 1.91+)

```bash
git clone https://github.com/CommitBook/CommitBook-Core.git
cd CommitBook-Core
cargo install --path crates/commitbook-cli
```

### Manual download

Grab a tarball from [GitHub Releases](https://github.com/CommitBook/CommitBook-Core/releases),
verify it against `SHA256SUMS`, and extract it onto your `PATH`.

## Quick start

```bash
# Navigate to your notes repo
cd ~/my-notes

# Initialize CommitBook (once per repo)
commitbook init

# Run your first sync
commitbook sync

# Start scheduled syncs
commitbook schedule hourly
commitbook start
```

## Documentation

Full documentation, configuration reference, and troubleshooting live in
[`docs/README.md`](docs/README.md).

## License

MIT. See [LICENSE](LICENSE).
