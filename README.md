# cosmic-applet-github-status

A COSMIC panel applet that shows the number of GitHub PRs waiting for your review.

[![GitHub](https://img.shields.io/badge/GitHub-Laeborg%2Fcosmic--applet--github--status-blue)](https://github.com/Laeborg/cosmic-applet-github-status)

## Features

- Displays a count of open PRs where you are a requested reviewer and have not yet approved
- Updates every 60 seconds
- Click the applet to open a popup with the current count
- Click "Open GitHub" to go directly to your GitHub review queue

## Requirements

- [COSMIC desktop environment](https://github.com/pop-os/cosmic-epoch)
- [GitHub CLI (`gh`)](https://cli.github.com/) — authenticated via `gh auth login`

## Installation

### 1. Authenticate with GitHub CLI

```sh
gh auth login
```

### 2. Build

```sh
cargo build --release
```

### 3. Install files

```sh
sudo just install
```

To uninstall:

```sh
sudo just uninstall
```

### 4. Add to panel

Right-click the COSMIC panel → **Edit panel** → click **+** → select **GitHub Status**.

## Development

- `cargo build` — debug build
- `cargo build --release` — release build
- `cargo run --release` — run standalone (outside the panel, for testing)
- `cargo clippy` — lint

## Links

- [libcosmic API documentation](https://pop-os.github.io/libcosmic/cosmic/)
- [libcosmic book](https://pop-os.github.io/libcosmic-book/)
- [GitHub CLI](https://cli.github.com/)
