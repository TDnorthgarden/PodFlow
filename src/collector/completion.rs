//! Shell completion module
//!
//! Provides shell completion functionality for nuts-observer CLI

/// Generate bash completion script
pub fn generate_bash_completion() -> String {
    r#"
# nuts-observer bash completion
_nuts_observer_completion() {
    local cur prev words cword
    COMPREPLY=()
    cur="${COMP_WORDS[COMP_CWORD]}"
    prev="${COMP_WORDS[COMP_CWORD-1]}"
    words="${COMP_WORDS[@]}"

    case "${prev}" in
        nuts-observer)
            case "${cur}" in
                "")
                    COMPREPLY=(trigger watch list-pods export completion config health)
                    ;;
                *)
                    # Complete subcommands and their options
                    local commands="trigger watch list-pods export completion config health"
                    local cmd
                    for cmd in $commands; do
                        if [[ "$cmd" == "${cur}"* ]]; then
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
}

complete -F _nuts_observer_completion nuts-observer
"#.to_string()
}

/// Generate zsh completion script
pub fn generate_zsh_completion() -> String {
    r#"
#compdef -n _nuts_observer nuts-observer

_nuts_observer() {
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
}
"#.to_string()
}

/// Generate fish completion script
pub fn generate_fish_completion() -> String {
    r#"
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
"#.to_string()
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
