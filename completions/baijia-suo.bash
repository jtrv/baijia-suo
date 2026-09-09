_baijia__suo() {
    local i cur prev opts cmd
    COMPREPLY=()
    if [[ "${BASH_VERSINFO[0]}" -ge 4 ]]; then
        cur="$2"
    else
        cur="${COMP_WORDS[COMP_CWORD]}"
    fi
    prev="$3"
    cmd=""
    opts=""

    for i in "${COMP_WORDS[@]:0:COMP_CWORD}"
    do
        case "${cmd},${i}" in
            ",$1")
                cmd="baijia__suo"
                ;;
            *)
                ;;
        esac
    done

    case "${cmd}" in
        baijia__suo)
            opts="-C -c -d -R -A -V --config --color --debug --daemonize --no-daemonize --ready-fd --animation --cycle --max-fps --low-battery-percent --debug-timing --list-animations --indicator-mode --indicator-opacity --indicator-color --auth-backend --username --auth-test --version"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 1 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --config)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                -C)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --color)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                -c)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --ready-fd)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                -R)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --animation)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                -A)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --cycle)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --max-fps)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --low-battery-percent)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --indicator-mode)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --indicator-opacity)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --indicator-color)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --auth-backend)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --username)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
    esac
}

if [[ "${BASH_VERSINFO[0]}" -eq 4 && "${BASH_VERSINFO[1]}" -ge 4 || "${BASH_VERSINFO[0]}" -gt 4 ]]; then
    complete -F _baijia__suo -o nosort -o bashdefault -o default baijia-suo
else
    complete -F _baijia__suo -o bashdefault -o default baijia-suo
fi
