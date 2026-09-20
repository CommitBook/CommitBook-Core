# Auto-Sync AI Agent Dotfiles

Keep your AI agent configs versioned and synced across machines using CommitBook.

## The Problem

AI coding agents store configuration in dotfile folders scattered across your home directory:

| Folder | Agent |
|---|---|
| `~/.claude/` | Claude Code |
| `~/.cursor/` | Cursor |
| `~/.codex/` | Codex CLI |
| `~/.copilot/` | GitHub Copilot |
| `~/.agents/` | Agents.dev |
| `~/.factory/` | Factory |

You (or the agents themselves) edit these configs over time. Without version control, changes are invisible: there's no history, no backup, and no way to sync across machines.

## The Solution

1. Create a normal git repo for your dotfiles
2. Move config folders into it and symlink them back
3. Let CommitBook auto-commit and push on a schedule

Every non-ignored config change (whether you make it or an agent does) is versioned and synced automatically.

## Setup

### 1. Create a dotfiles repo

```bash
mkdir ~/dotfiles
cd ~/dotfiles
git init
```

### 2. Move folders and create symlinks

For each agent config folder you want to track:

```bash
# Move the folder into the repo
mv ~/.claude ~/dotfiles/.claude

# Symlink it back so the agent still finds it. The destination was removed by
# mv, so use ln -s rather than risking a nested link inside an existing folder.
ln -s ~/dotfiles/.claude ~/.claude
```

Repeat for other agents:

```bash
mv ~/.cursor ~/dotfiles/.cursor && ln -s ~/dotfiles/.cursor ~/.cursor
mv ~/.codex ~/dotfiles/.codex && ln -s ~/dotfiles/.codex ~/.codex
mv ~/.copilot ~/dotfiles/.copilot && ln -s ~/dotfiles/.copilot ~/.copilot
```

Verify the symlinks work:

```bash
ls -la ~/.claude
# Should show: .claude -> /Users/you/dotfiles/.claude
```

### 3. Exclude credentials and machine-specific files

Create a `.gitignore` in your dotfiles repo to keep secrets out of git:

```bash
cat > ~/dotfiles/.gitignore << 'EOF'
# Credentials and tokens
**/auth.toml
**/credentials.json
**/token
**/secrets.*
.codex/auth.json

# Machine-specific state
**/projects/
**/state.toml
**/*.lock
**/logs/

# OS files
.DS_Store
EOF
```

Codex stores account credentials in `~/.codex/auth.json`; never commit that
file. Gitignore does not untrack a file that was already added, so check and
remove it from the index before the first push if necessary:

```bash
cd ~/dotfiles
git ls-files --error-unmatch .codex/auth.json >/dev/null 2>&1 && \
  git rm --cached .codex/auth.json
```

### 4. Push to a remote

```bash
cd ~/dotfiles
git add .
git commit -m "Add AI agent dotfiles"
git remote add origin git@github.com:you/dotfiles.git
git push -u origin main
```

### 5. Start CommitBook

```bash
cd ~/dotfiles
commitbook init                # Initialize .CommitBook/ (once per repo)
commitbook sync                # First sync
commitbook schedule every-4h   # Sync every 4 hours
commitbook start               # Start the scheduler
```

That's it. From now on, every non-ignored config change lands in git automatically.

## What This Looks Like

```
~/dotfiles/                    # Git repo, managed by CommitBook
├── .CommitBook/               # CommitBook config
│   ├── .gitignore              # Ignores /local/
│   ├── config.toml
│   └── local/                  # Device state; ignored
├── .gitignore
├── .claude/                   # Claude Code config (real files)
│   ├── CLAUDE.md
│   └── settings.json
├── .cursor/                   # Cursor config (real files)
│   └── settings.json
└── .codex/                    # Codex config (real files)
    └── instructions.md

~/                             # Home directory
├── .claude -> ~/dotfiles/.claude    # Symlink
├── .cursor -> ~/dotfiles/.cursor    # Symlink
└── .codex -> ~/dotfiles/.codex      # Symlink
```

## Setting Up Another Machine

```bash
git clone git@github.com:you/dotfiles.git ~/dotfiles

# Back up existing destinations before creating links. Forcing a symlink onto
# an existing directory would otherwise create a nested link inside it.
backup_stamp=$(date +%Y%m%d-%H%M%S)
for name in .claude .cursor .codex; do
  if [ -e "$HOME/$name" ] || [ -L "$HOME/$name" ]; then
    mv "$HOME/$name" "$HOME/${name}.pre-dotfiles-$backup_stamp"
  fi
  ln -s "$HOME/dotfiles/$name" "$HOME/$name"
done

# Start syncing
cd ~/dotfiles
commitbook schedule every-4h
commitbook start
```

## Tips

- **Check what's tracked** before pushing: `cd ~/dotfiles && git status`, and make sure no credentials slipped through
- **Use `commitbook log`** to see sync activity: `cd ~/dotfiles && commitbook log`
- **Adjust the schedule** based on how often configs change: `every-4h` is a good default for dotfiles
