# Goble's ZDOTDIR redirect for zsh: `.zshenv` is read for every startup, from
# `$ZDOTDIR` and nowhere else, so when the pane points ZDOTDIR at the
# integration this shim has to restore the user's own environment file.
#
# It is written next to `.zshrc` and runs before it; GOBLE_ORIG_ZDOTDIR is the
# directory the user's zsh configuration actually lives in.

if [ -n "${GOBLE_ORIG_ZDOTDIR:-}" ] && [ -f "${GOBLE_ORIG_ZDOTDIR}/.zshenv" ]; then
    GOBLE_KEEP_ZDOTDIR=$ZDOTDIR
    ZDOTDIR=$GOBLE_ORIG_ZDOTDIR
    . "$GOBLE_ORIG_ZDOTDIR/.zshenv"
    ZDOTDIR=$GOBLE_KEEP_ZDOTDIR
    unset GOBLE_KEEP_ZDOTDIR
fi
