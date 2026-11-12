__dt_sequence=0
__dt_at_prompt=0
__dt_command_id=''

__devtool_debug() {
    [[ $BASHPID == $$ && $BASH_COMMAND != __devtool_prompt ]] || return 0
    local command=$BASH_COMMAND line
    if [[ $__dt_at_prompt == 1 ]]; then
        __dt_at_prompt=0
        ((__dt_sequence+=1))
        __dt_command_id="$$:$__dt_sequence"
        line=$(HISTTIMEFORMAT= builtin history 1)
        [[ $line =~ ^[[:space:]]*[0-9]+[[:space:]]+(.*)$ ]] && line=${BASH_REMATCH[1]}
        builtin printf 'start\0%s\0%s\0%s\0' "$__dt_command_id" "$EPOCHREALTIME" "${line:-$command}" >> "$DEMO_EVENTS_FILE"
    fi
    if [[ -n $__dt_command_id ]]; then
        builtin printf 'step\0%s\0%s\0%s\0' "$__dt_command_id" "$EPOCHREALTIME" "$command" >> "$DEMO_EVENTS_FILE"
    fi
    return 0
}

__devtool_prompt() {
    local result=$?
    if [[ -n $__dt_command_id ]]; then
        builtin printf 'end\0%s\0%s\0%s\0' "$__dt_command_id" "$EPOCHREALTIME" "$result" >> "$DEMO_EVENTS_FILE"
        __dt_command_id=''
    fi
    builtin printf '%s' "$PWD" > "$DEMO_CWD_FILE"
    __dt_at_prompt=1
}

HISTCONTROL=
HISTIGNORE=
PROMPT_COMMAND=__devtool_prompt
trap '__devtool_debug' DEBUG
