# 💫 projectable

![screenshot](./extras/screenshot.png)

`projectable` is a highly configurable project manager. You can do everything
your project needs from a comfortable and smooth interface: run commands, open
your editor, integrate with tmux, see git changes, and more.

Instead of exploring the depths of your most nested directory, open a file simply
from the `projectable` file listing!

Here are just a few builtin things projectable can do:

- 🔍 Preview files
- 💥 Run commands, foreground or background
- 👀 Fuzzy search files
- 📁 Create files or directories
- ❌ Delete files or directories
- 🙈 Ignore files based on glob patterns
- 🔳 Toggle hidden files
- 🎯 Mark files to quick and easy access
- 🙉 Respect gitignore
- 🔔 Live update to new files/changes
- 🌲 View your project as a hierarchy
- 🔦 Automatically recognize project root, with customizability
- 💼 Run special commands that change on a per-file basis
- 👓 View git changes
- ✏️ Easily write custom commands
- 📖 Fully configurable with a dead-simple `toml` file

## 🚀 Getting Started

To get started, you can use one of the following installation methods:

<details>
  <summary>cargo</summary>

```bash
$ cargo install projectable
```

</details>

<details>
  <summary>Build from source</summary>

Requires [Rust](https://github.com/rust-lang/rust) to be installed on your
computer.

```bash
$ git clone https://github.com/dzfrias/projectable
$ cd projectable
$ cargo build --release
$ ./target/release/prj
```

</details>

After you've installed, run `prj` to start it up! The default keybinds are
vim-like (j for up, k for down), but you can change them in
[CONFIG.md](./extras/CONFIG.md).

## ⌨️ Keybinds

Here a list of the available actions and their default bindings. For
customization, see [CONFIG.md](./extras/CONFIG.md).
