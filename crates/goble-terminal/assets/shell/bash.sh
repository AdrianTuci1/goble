# Goble shell integration for bash.
#
# The pane's shell sources this file at startup. It reports shell state to the
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
# Installation: the pane runs `bash --rcfile <this file>`, so bash does not
# read ~/.bashrc on its own. This script reads it first, before it takes over
# the prompt; set GOBLE_SKIP_USER_RC=1 to leave the user's rc out (tests do).
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
# expect the shell's PS1 to be visible. A later phase negotiates the toggle.
__goble_honor_ps1=0
__GOBLE_ACTIVE=1

# ---------------------------------------------------------------------------
# Hook payloads
# ---------------------------------------------------------------------------

__goble_emit_initshell() {
    local cwd host
    cwd=$(__goble_json_escape "$PWD")
    host=$(__goble_json_escape "$__GOBLE_HOST")
    __goble_emit "{\"hook\":\"InitShell\",\"value\":{\"session_id\":\"$__GOBLE_SESSION_ID\",\"shell\":\"bash\",\"host\":\"$host\",\"cwd\":\"$cwd\",\"honor_ps1\":$(__goble_bool "$__goble_honor_ps1")}}"
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

# The `clear` builtin is replaced while the integration is active; the hook is
# what lets the emulator keep the transcript instead of wiping it.
clear() {
    __goble_emit_clear
    command clear "$@"
}

# ---------------------------------------------------------------------------
# Command lifecycle (forked bash-preexec pattern)
# ---------------------------------------------------------------------------

# Set by __goble_prompt_command once the prompt is about to be drawn, cleared by
# the first command that follows it. Only that first command is the user's.
__goble_preexec_interactive_mode=""

__goble_debug_trap() {
    # The prompt command's own invocation reaches the trap with its name in
    # BASH_COMMAND. Clear the flag so its internal commands are not reported as
    # a command of their own; an empty Enter is the case that needs this.
    if [ "${BASH_COMMAND:-}" = "__goble_prompt_command" ]; then
        __goble_preexec_interactive_mode=""
        return 0
    fi
    if [ "$__goble_preexec_interactive_mode" != "on" ]; then
        return 0
    fi
    # Only a command typed at an interactive terminal is a user command.
    if [ ! -t 1 ]; then
        return 0
    fi
    if [ -n "${COMP_LINE:-}" ]; then
        return 0
    fi
    if [ -n "${READLINE_LINE+x}" ]; then
        return 0
    fi
    if [ "${BASH_SUBSHELL:-0}" -eq 0 ]; then
        __goble_preexec_interactive_mode=""
    fi
    local command
    command=$(export LC_ALL=C; HISTTIMEFORMAT='' builtin history 1 2>/dev/null | command sed '1 s/^ *[0-9][0-9]*[* ] //')
    if [ -z "$command" ]; then
        return 0
    fi
    __goble_emit_preexec "$command"
}

__goble_prompt_command() {
    # $? is the previous command's status and must be read before anything
    # else runs.
    local __goble_exit=$?
    __goble_emit_command_finished "$__goble_exit"
    __goble_emit_precmd
    # Last statement: the trap fires for every command above, so the flag is
    # armed only once the next prompt is about to be drawn.
    __goble_preexec_interactive_mode="on"
}

# Leave the shell exactly as it was found and stop reporting.
__goble_disable() {
    if [ -z "${__GOBLE_ACTIVE:-}" ]; then
        return 0
    fi
    __GOBLE_ACTIVE=""
    __goble_preexec_interactive_mode=""
    trap - DEBUG
    if [ -n "$__goble_saved_debug_trap" ]; then
        eval "$__goble_saved_debug_trap"
    fi
    PROMPT_COMMAND=$__goble_saved_prompt_command
    HISTCONTROL=$__goble_saved_histcontrol
    PS1=$__goble_saved_ps1
    unset -f clear 2>/dev/null
}

__goble_teardown() {
    __goble_disable
    if [ -n "${__goble_previous_exit_trap:-}" ]; then
        eval "$__goble_previous_exit_trap"
    fi
}

__goble_install() {
    __goble_saved_ps1=$PS1
    __goble_saved_prompt_command=$PROMPT_COMMAND
    __goble_saved_histcontrol=${HISTCONTROL:-}
    __goble_saved_debug_trap=$(trap -p DEBUG 2>/dev/null)

    # A command the history ignores never appears in `history 1`, so the trap
    # would report the previous command instead. This is bash-preexec's fix.
    case "${HISTCONTROL:-}" in
        *ignorespace*) HISTCONTROL=${HISTCONTROL//ignorespace/} ;;
    esac
    case "${HISTCONTROL:-}" in
        *ignoreboth*) HISTCONTROL=${HISTCONTROL//ignoreboth/ignoredups} ;;
    esac

    # Hide the shell's prompt; __goble_disable restores it.
    if [ "$__goble_honor_ps1" != "1" ]; then
        PS1=""
    fi

    # The user's own PROMPT_COMMAND is saved, not composed: bash-preexec owns
    # PROMPT_COMMAND while the integration is active.
    PROMPT_COMMAND="__goble_prompt_command"
    trap '__goble_debug_trap' DEBUG
}

# ---------------------------------------------------------------------------
# Bootstrap
# ---------------------------------------------------------------------------

# The handshake starts the moment the integration runs.
__goble_emit_initshell

# `--rcfile` replaced ~/.bashrc, so read it now, before taking over the prompt.
if [ -z "${GOBLE_SKIP_USER_RC:-}" ] && [ -n "${HOME:-}" ] && [ -f "$HOME/.bashrc" ]; then
    . "$HOME/.bashrc"
fi

__goble_install
__goble_emit_bootstrapped
__goble_emit_precmd
__goble_preexec_interactive_mode="on"
__goble_previous_exit_trap=$(trap -p EXIT 2>/dev/null)
trap '__goble_teardown' EXIT
