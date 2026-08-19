#!/bin/sh
# Bootstrap the versioned cargo-dist installer for phig.
set -eu

usage() {
  cat <<'EOF'
Install phig from checksummed GitHub Release artifacts.

Usage: install.sh [--prefix DIR] [--yes] [--help]

Options:
  --prefix DIR  install phig into DIR/bin
  --yes         accepted for non-interactive scripts (installation never prompts)
  --help        show this help

Environment:
  PHIG_VERSION  release to install, for example 1.0.0 (default: latest stable)
EOF
}

prefix=
while [ "$#" -gt 0 ]; do
  case "$1" in
    --prefix)
      [ "$#" -ge 2 ] || { echo "install.sh: --prefix requires a directory" >&2; exit 2; }
      prefix=$2
      shift 2
      ;;
    --yes)
      shift
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      echo "install.sh: unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

command -v curl >/dev/null 2>&1 || {
  echo "install.sh: curl is required" >&2
  exit 1
}

valid_identifiers() {
  identifiers=$1
  prerelease=$2
  case "$identifiers" in ''|.*|*.|*..*) return 1 ;; esac
  saved_ifs=$IFS
  IFS=.
  # Deliberate field splitting walks each dot-separated identifier.
  # shellcheck disable=SC2086
  set -- $identifiers
  IFS=$saved_ifs
  for identifier do
    case "$identifier" in *[!0-9A-Za-z-]*) return 1 ;; esac
    if [ "$prerelease" = yes ]; then
      case "$identifier" in
        *[!0-9]*) ;;
        0) ;;
        0*) return 1 ;;
      esac
    fi
  done
}

version=${PHIG_VERSION-latest}
case "$version" in
  latest)
    installer_url=https://github.com/phall1/phig/releases/latest/download/phig-cli-installer.sh
    ;;
  v*|[0-9]*)
    version=${version#v}
    case "$version" in
      *[!0-9A-Za-z.+-]*) echo "install.sh: invalid PHIG_VERSION" >&2; exit 2 ;;
    esac
    core=${version%%[-+]*}
    old_ifs=$IFS
    IFS=.
    # Deliberate field splitting validates exactly three numeric components.
    # shellcheck disable=SC2086
    set -- $core
    IFS=$old_ifs
    if [ "$#" -ne 3 ]; then
      echo "install.sh: invalid PHIG_VERSION" >&2
      exit 2
    fi
    for number do
      case "$number" in
        ''|*[!0-9]*) echo "install.sh: invalid PHIG_VERSION" >&2; exit 2 ;;
        0) ;;
        0*) echo "install.sh: invalid PHIG_VERSION" >&2; exit 2 ;;
      esac
    done
    suffix=${version#"$core"}
    case "$suffix" in
      '') ;;
      -*)
        prerelease=${suffix#-}
        case "$prerelease" in
          *+*)
            build=${prerelease#*+}
            prerelease=${prerelease%%+*}
            case "$build" in *+*) echo "install.sh: invalid PHIG_VERSION" >&2; exit 2 ;; esac
            valid_identifiers "$build" no || { echo "install.sh: invalid PHIG_VERSION" >&2; exit 2; }
            ;;
        esac
        valid_identifiers "$prerelease" yes || { echo "install.sh: invalid PHIG_VERSION" >&2; exit 2; }
        ;;
      +*)
        build=${suffix#+}
        case "$build" in *+*) echo "install.sh: invalid PHIG_VERSION" >&2; exit 2 ;; esac
        valid_identifiers "$build" no || { echo "install.sh: invalid PHIG_VERSION" >&2; exit 2; }
        ;;
      *) echo "install.sh: invalid PHIG_VERSION" >&2; exit 2 ;;
    esac
    installer_url="https://github.com/phall1/phig/releases/download/v${version}/phig-cli-installer.sh"
    ;;
  *)
    echo "install.sh: invalid PHIG_VERSION (expected latest or a semantic version)" >&2
    exit 2
    ;;
esac

temporary=$(mktemp -d "${TMPDIR:-/tmp}/phig-install.XXXXXXXX") || {
  echo "install.sh: could not create a temporary directory" >&2
  exit 1
}
cleanup() {
  rm -rf "$temporary"
}
trap cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM
installer=$temporary/phig-cli-installer.sh
umask 077

curl --proto '=https' --tlsv1.2 --fail --silent --show-error --location \
  --connect-timeout 10 --max-time 120 --max-filesize 4194304 \
  --output "$installer" "$installer_url"

if [ -n "$prefix" ]; then
  install_dir=$prefix/bin
  mkdir -p "$install_dir"
  PHIG_CLI_UNMANAGED_INSTALL=$install_dir /bin/sh "$installer"
  printf '%s\n' "phig installed in $install_dir"
else
  /bin/sh "$installer"
fi

printf '%s\n' 'Verify with: phig version'
