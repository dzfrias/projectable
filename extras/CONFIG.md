# Configuring projectable

Almost every behavior of projectable can be custommized through a simple `toml`
file.

To get started, run `prj --make-config` to create a new config file. Then, run
`prj --config` to get the location of your config file. Go to that directory,
and edit the TOML file.

## Commands

To create a new command, bound to a key, use the `commands` key of the
configuration. This will be a key-value pair value.

Here's an example:

```toml
commands = { "ctrl-e" = "echo Hello, World!" }
```

You can use [the command syntax](../README.md#command-syntax) for more dynamic
commands!

### Special Commands

In projectable, you may also define commands that change on a per-file basis.
By default, this is bound to `v`, but can be [changed](#keys).

For example, here's a possible configuration for a `Cargo.toml` file:

```toml
special_commands = { "Cargo.toml" = ["cargo add {...}", "cargo remove {...}", "cargo build"] }
```

When you press `v` while selecting a `Cargo.toml` file, projectable will prompt
you to run one of these commands!

The key part of the configuration accepts globs, so you could generalize this
to run with `Cargo.*` if you'd like this prompt to appear in lock file as well.

## Keys

Many of the default keybinds can be changed in projectable.

For example, if you'd like to change the default up/down selection keys, you
could do so with:

```toml
up = "up"
down = "down"
```

Changing the new file key could look like this:

```toml
[filetree]
new_file = "alt-n"
```

alt and ctrl are the only currently supported modifiers.

For the rest of the possible keybinds, see
[the entire configuration](#all-configuration-options).

## Colors

Like everything else, projectable's colorscheme can be completely user-defined!
"Colors" is a bit of a misnomer; projectable also allows you to control the
text style too, giving you the options of bold or italic text.

Here's an example of a possible color change:

```toml
selected = { color = "rgb(0, 0, 0)", bg = "#FFFFFF", mods = ["italic"] }

[filetree]
border_color = { color = "yellow" }
```

RGB and hex are both supported, along with a list of modifiers. Currently, only
italic and bold are available.

To see all possible color options, see
[the entire configuration reference](#all-configuration-options).

## All Configuration Options

TODO
