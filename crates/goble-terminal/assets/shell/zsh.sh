# Goble shell integration for zsh.
#
# The pane's shell sources this file at startup, from the `.zshrc` of the
# directory it was pointed at with ZDOTDIR. It reports shell state to the
# emulator over the DCS hex-JSON hook channel the Rust side decodes in
# `crates/goble-terminal/src/hooks.rs`; that schema is authoritative and the
# payloads below match it field for field.
#
# Every hook is written as the envelope
#
#     ESC P $ d <hex(JSON)> ESC \
#
# so arbitrary bytes in a command line or a path can never be mistaken for a
# control sequence by the VT parser.
#
# Installation: GOBLE_ORIG_ZDOTDIR names the directory the user's own .zshrc
# lives in (normally $HOME); it is sourced before the prompt is taken over.
# When the script is sourced directly that variable is unset and no user rc is
# read.
#
# Degradation: with `honor_ps1` set (a later toggle) the shell's own prompt is
# shown instead of hidden; the flag travels in the Precmd payload.

if [ -n "${__GOBLE_ACTIVE:-}" ]; then
    return 0 2>/dev/null || exit 0
fi

# ---------------------------------------------------------------------------
# Wire format
# ---------------------------------------------------------------------------

__GOBLE_DCS_START=$(printf '\033P$d')
__GOBLE_DCS_END=$(printf '\033\134')

# Hex-encode one payload. `od` keeps the encoding byte-exact, including bytes
# that would otherwise end the sequence (ESC, ST) or the JSON string.
__goble_hex() {
    printf '%s' "$1" | command od -An -v -tx1 | command tr -d ' \n'
}

# Escape one value for a JSON string literal. Done per character in `awk`
# rather than with `sed`: the BSD and GNU `sed`s disagree about `\t` and about
# a last line of input without a trailing newline, and both cases matter here
# (paths with spaces, command lines with quotes).
__goble_json_escape() {
    printf '%s' "$1" | command awk '
        function esc(s,   i, c, out) {
            out = ""
            for (i = 1; i <= length(s); i++) {
                c = substr(s, i, 1)
                if (c == "\\") out = out "\\\\"
                else if (c == "\"") out = out "\\\""
                else if (c == "\t") out = out "\\t"
                else if (c == "\r") out = out "\\r"
                else out = out c
            }
            return out
        }
        BEGIN { out = "" }
        { if (NR > 1) out = out "\\n"; out = out esc($0) }
        END { printf "%s", out }
    '
}

# Write one hook envelope to the terminal.
__goble_emit() {
    printf '%s%s%s' "$__GOBLE_DCS_START" "$(__goble_hex "$1")" "$__GOBLE_DCS_END"
}

__goble_bool() {
    if [ "$1" = "1" ]; then printf 'true'; else printf 'false'; fi
}

# ---------------------------------------------------------------------------
# Session state
# ---------------------------------------------------------------------------

__GOBLE_SESSION_ID="${GOBLE_SESSION_ID:-$$-${RANDOM}}"
__GOBLE_HOST=$(command -p hostname 2>/dev/null || command -p uname -n 2>/dev/null)
__goble_block_id=0
# The integration hides the shell's prompt, so the emulator is told not to
# expect the shell's prompt to be visible. A later phase negotiates the toggle.
__goble_honor_ps1=0
__GOBLE_ACTIVE=1

# ---------------------------------------------------------------------------
# Hook payloads
# ---------------------------------------------------------------------------

__goble_emit_initshell() {
    local cwd host
    cwd=$(__goble_json_escape "$PWD")
    host=$(__goble_json_escape "$__GOBLE_HOST")
    __goble_emit "{\"hook\":\"InitShell\",\"value\":{\"session_id\":\"$__GOBLE_SESSION_ID\",\"shell\":\"zsh\",\"host\":\"$host\",\"cwd\":\"$cwd\",\"honor_ps1\":$(__goble_bool "$__goble_honor_ps1")}}"
}

__goble_emit_bootstrapped() {
    __goble_emit "{\"hook\":\"Bootstrapped\",\"value\":{\"version\":\"1\"}}"
}

__goble_emit_preexec() {
    __goble_emit "{\"hook\":\"Preexec\",\"value\":{\"command\":\"$(__goble_json_escape "$1")\"}}"
}

__goble_emit_command_finished() {
    local code=$1
    case "$code" in
        '' | *[!0-9-]*) code=0 ;;
    esac
    __goble_block_id=$((__goble_block_id + 1))
    __goble_emit "{\"hook\":\"CommandFinished\",\"value\":{\"exit_code\":$code,\"next_block_id\":\"precmd-$__GOBLE_SESSION_ID-$__goble_block_id\"}}"
}

__goble_emit_precmd() {
    local pwd git_branch
    pwd=$(__goble_json_escape "$PWD")
    git_branch=""
    if command -v git >/dev/null 2>&1; then
        git_branch=$(GIT_OPTIONAL_LOCKS=0 command git symbolic-ref --short HEAD 2>/dev/null)
        if [ -z "$git_branch" ]; then
            git_branch=$(GIT_OPTIONAL_LOCKS=0 command git rev-parse --short HEAD 2>/dev/null)
        fi
    fi
    __goble_emit "{\"hook\":\"Precmd\",\"value\":{\"pwd\":\"$pwd\",\"git_branch\":\"$(__goble_json_escape "$git_branch")\",\"rprompt\":\"\",\"session_id\":\"$__GOBLE_SESSION_ID\",\"virtual_env\":\"$(__goble_json_escape "${VIRTUAL_ENV:-}")\",\"conda_env\":\"$(__goble_json_escape "${CONDA_DEFAULT_ENV:-}")\",\"node_version\":\"\",\"honor_ps1\":$(__goble_bool "$__goble_honor_ps1")}}"
}

__goble_emit_input_buffer() {
    local buffer=$1 cursor=$2
    case "$cursor" in
        '' | *[!0-9]*) cursor=0 ;;
    esac
    __goble_emit "{\"hook\":\"InputBuffer\",\"value\":{\"buffer\":\"$(__goble_json_escape "$buffer")\",\"cursor\":$cursor}}"
}

__goble_emit_clear() {
    __goble_emit '{"hook":"Clear"}'
}

# The `clear` command is replaced while the integration is active; the hook is
# what lets the emulator keep the transcript instead of wiping it.
clear() {
    __goble_emit_clear
    command clear "$@"
}

# ---------------------------------------------------------------------------
# Command lifecycle (zsh precmd/preexec hooks)
# ---------------------------------------------------------------------------

__goble_preexec() {
    __goble_emit_preexec "$1"
}

__goble_precmd() {
    # $? is the previous command's status; zsh sets it before precmd runs.
    local __goble_exit=$?
    __goble_emit_command_finished "$__goble_exit"
    __goble_emit_precmd
}

# Leave the shell exactly as it was found and stop reporting.
__goble_disable() {
    if [ -z "${__GOBLE_ACTIVE:-}" ]; then
        return 0
    fi
    __GOBLE_ACTIVE=""
    if type add-zsh-hook >/dev/null 2>&1; then
        add-zsh-hook -d preexec __goble_preexec 2>/dev/null
        add-zsh-hook -d precmd __goble_precmd 2>/dev/null
        add-zsh-hook -d zshexit __goble_disable 2>/dev/null
    else
        preexec_functions=(${preexec_functions:#__goble_preexec})
        precmd_functions=(${precmd_functions:#__goble_precmd})
    fi
    PROMPT=$__goble_saved_prompt
    RPROMPT=$__goble_saved_rprompt
    unset -f clear 2>/dev/null
}

__goble_install() {
    __goble_saved_prompt=$PROMPT
    __goble_saved_rprompt=$RPROMPT

    # Hide the shell's prompt; __goble_disable restores it.
    if [ "$__goble_honor_ps1" != "1" ]; then
        PROMPT=""
        RPROMPT=""
    fi

    if ! type add-zsh-hook >/dev/null 2>&1; then
        autoload -Uz add-zsh-hook 2>/dev/null
    fi
    if type add-zsh-hook >/dev/null 2>&1; then
        add-zsh-hook preexec __goble_preexec
        add-zsh-hook precmd __goble_precmd
        add-zsh-hook zshexit __goble_disable
    else
        preexec_functions+=(__goble_preexec)
        precmd_functions+=(__goble_precmd)
    fi
}

# ---------------------------------------------------------------------------
# Bootstrap
# ---------------------------------------------------------------------------

# The handshake starts the moment the integration runs.
__goble_emit_initshell

# ZDOTDIR points at this file's directory, so read the user's own .zshrc here.
if [ -z "${GOBLE_SKIP_USER_RC:-}" ] && [ -n "${GOBLE_ORIG_ZDOTDIR:-}" ] && [ -f "$GOBLE_ORIG_ZDOTDIR/.zshrc" ]; then
    . "$GOBLE_ORIG_ZDOTDIR/.zshrc"
fi

__goble_install
__goble_emit_bootstrapped
__goble_emit_precmd
