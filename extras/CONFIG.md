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
to run with `Cargo.*` if you'd like this prompt to appear in lock file too.

## Keys

Many of the default keybinds can be changed in projectable.

TODO

## Colors

Like everything else, projectable's colorscheme can be completely user-defined!

TODO

## All Configuration Options

TODO
