## diffr

Reviewing changes involves reading diffs.
Sometimes, a line-oriented presentation of changes is not precise
enough, especially when changes involve long lines or very similar
consecutive lines.

This program takes diffs and adds additional formatting on top
of the unified diff format using text attributes.
It works hunk by hunk, recomputing the diff on a word-by-word basis.

The current implementation uses 
[Myers' longest common subsequence](http://www.xmailserver.org/diff2.pdf) 
algorithm.

[![crates.io](https://img.shields.io/crates/v/diffr.svg)](https://crates.io/crates/diffr)
[![crates.io](https://img.shields.io/crates/d/diffr.svg)](https://crates.io/crates/diffr)
[![Build Status](https://dev.azure.com/nathanmoreau/diffr/_apis/build/status/mookid.diffr?branchName=master)](https://dev.azure.com/nathanmoreau/diffr/_build/latest?definitionId=4&branchName=master)

![Demo](screenshots/example_simple_mac.png)
![Demo](screenshots/example_nonconsecutive.png)
![Demo](screenshots/example_cross_lines_common_tokens.png)

### Installation

#### Arch Linux

Install from the [AUR](https://aur.archlinux.org/packages/diffr/):
```sh
git clone https://aur.archlinux.org/diffr.git
cd diffr
makepkg -si
```

#### Homebrew
```sh
brew install diffr
```

#### From source

You will need the [Rust compiler installed](https://www.rust-lang.org/tools/install).

To install the latest published version:
```sh
cargo install diffr
```

Alternatively, you can build the development version:
```sh
git clone https://github.com/mookid/diffr.git
cd diffr
cargo install --path .
```

### How to use it?

diffr tries to be a well behaved Unix program:
It reads its input from stdin and writes to stdout.

#### One-off usage

```sh
git show HEAD | diffr
```

#### Integration with git

Add the following section to your `.gitconfig` file:

```gitconfig
[core]
    pager = diffr | less -R
[interactive]
    diffFilter = diffr
```

Alternatively, you can update your configuration from the command line:

```sh
git config --global core.pager 'diffr | less -R'
git config --global interactive.difffilter diffr
```

#### Color customization

Use the `--colors` flag.

You can customize the display of diffing and common segments of added
and removed lines.

For instance, here is an example to tweak the red used for uniquely removed text:

```sh
diffr --colors refine-removed:background:200,0,0:foreground:white:bold
```

The configuration used in the first screenshot is

```sh
diffr --colors refine-added:none:background:0x33,0x99,0x33:bold --colors added:none:background:0x33,0x55,0x33 --colors refine-removed:none:background:0x99,0x33,0x33:bold --colors removed:none:background:0x55,0x33,0x33
```

#### Display line numbers

The ` --line-numbers` otion results in each line being prefixed with its line number.

### Related projects

This is an improvement on the
[diff-highlight](https://github.com/git/git/tree/master/contrib/diff-highlight)
script distributed with git.

git itself provides both `--word-diff` and `--color-words` options to
several commands.
