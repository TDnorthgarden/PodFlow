//! Shell completion module
//!
//! Provides shell completion functionality for nuts-observer CLI

/// Generate bash completion script
pub fn generate_bash_completion() -> String {
    format!(r#"
# nuts-observer bash completion
_nuts_observer_completion() {{
    local cur prev words cword
    COMPREPLY=()
    cur="${{COMP_WORDS[COMP_CWORD]}}"
    prev="${{COMP_WORDS[COMP_CWORD-1]}}"
    words="${{COMP_WORDS[@]}}"

    case "${{prev}}" in
        nuts-observer)
            case "${{cur}}" in
                "")
                    COMPREPLY=(trigger watch list-pods export completion config health)
                    ;;
                *)
                    # Complete subcommands and their options
                    local commands="trigger watch list-pods export completion config health"
                    local cmd
                    for cmd in $commands; do
                        if [[ "$cmd" == "${{cur}}"* ]]; then
                            COMPREPLY+=("$cmd")
                        fi
                    done
                    ;;
            esac
            ;;
        *)
            COMPREPLY=()
            ;;
    esac

    return 0
}}

complete -F _nuts_observer_completion nuts-observer
"#)
}

/// Generate zsh completion script
pub fn generate_zsh_completion() -> String {
    format!(r#"
#compdef -n _nuts_observer nuts-observer

_nuts_observer() {{
    local -a commands
    commands=(
        'trigger:Trigger manual diagnostics'
        'watch:Watch mode for real-time monitoring'
        'list-pods:List pods'
        'export:Export case library'
        'completion:Generate shell completion'
        'config:Manage configuration'
        'health:Check server health'
    )

    if (( CURRENT == 1 )); then
        _describe 'commands'
        _nuts_observer_commands
        return
    fi

    case "$words[1]" in
        trigger|watch|list-pods|export|completion|config|health)
            _arguments '--namespace --name --output'
            ;;
    esac
}}
"#)
}

/// Generate fish completion script
pub fn generate_fish_completion() -> String {
    format!(r#"
# nuts-observer fish completion

complete -c nuts-observer -n '__fish_nuts_observer_no_subcommand' -f

function __fish_nuts_observer_no_subcommand
    for cmd in trigger watch list-pods export completion config health
        echo $cmd
    end
end

complete -c nuts-observer -n '__fish_nuts_observer_using_command' -x -a '
command trigger
command watch
command list-pods
command export
command completion
command config
command health
' -p 'nuts-observer'

complete -c nuts-observer -n '__fish_nuts_observer_using_command' -l trigger -s 'trigger' -d 'Trigger manual diagnostics' -a '
{
    short   long     description
    u       pod-uid  "Target Pod UID"
    n       namespace "Target namespace"
    p       pod-name  "Pod name"
    g       cgroup-id "cgroup ID"
    e       evidence-types "Evidence types (comma separated)"
    m       metrics    "Metrics to collect"
    w       window-secs "Time window in seconds"
    c       count      "Number of iterations"
    d       detailed   "Show detailed output"
}'

complete -c nuts-observer -n '__fish_nuts_observer_using_command' -l watch -s 'watch' -d 'Watch mode for real-time monitoring' -a '
{
    short   long     description
    u       pod-uid  "Target Pod UID"
    n       namespace "Target namespace"
    p       pod-name  "Pod name"
    e       evidence-types "Evidence types"
    m       metrics    "Metrics to collect"
    w       window-secs "Time window"
    i       interval   "Monitoring interval"
    c       count      "Number of iterations"
    d       detailed   "Show detailed output"
}'

complete -c nuts-observer -n '__fish_nuts_observer_using_command' -l list-pods -s 'list-pods' -d 'List pods' -a '
{
    short   long     description
    n       namespace "Filter by namespace"
    m       name      "Filter by name"
    o       output    "Output format"
}'

complete -c nuts-observer -n '__fish_nuts_observer_using_command' -l export -s 'export' -d 'Export case library' -a '
{
    short   long     description
    f       file      "Output file path"
    n       namespace "Filter by namespace"
    m       name      "Filter by name"
    o       output    "Output format"
}'

complete -c nuts-observer -n '__fish_nuts_observer_using_command' -l completion -s 'completion' -d 'Generate shell completion' -a '
{
    short   long     description
    s       shell      "Shell type (bash, zsh, fish)"
}'
"#)
}

/// Generate completion script based on shell type
pub fn generate_completion_script(shell: &str) -> Result<String, crate::types::error::NutsError> {
    match shell {
        "bash" => Ok(generate_bash_completion()),
        "zsh" => Ok(generate_zsh_completion()),
        "fish" => Ok(generate_fish_completion()),
        _ => Err(crate::types::error::NutsError::InvalidInput(format!("Unsupported shell: {}", shell))),
    }
}
