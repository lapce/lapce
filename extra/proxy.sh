#!/bin/sh
set -eux

# This script is written to be as POSIX as possible
# so it works fine for all Unix-like operating systems

stderr="/dev/stderr"
if ! test -w $stderr ; then
  stderr="/dev/null"
fi

stdout="/dev/stdout"
if ! test -w $stdout ; then
  stdout="/dev/null"
fi

test_cmd() {
  command -v "$1" >/dev/null
}

println() {
  printf "%s\n" $@ > $stdout
}

eprintln() {
  printf "%s\n" $@ > $stderr
}

# proxy version
lapce_new_ver="${1}"
# proxy directory
# eval to resolve '~' into proper user dir
eval lapce_dir="'${2}'"

case "${lapce_new_ver}" in
  v*)
    lapce_new_version=$(echo "${lapce_new_ver}" | cut -d'v' -f2)
    lapce_new_ver_tag="${lapce_new_ver}"
  ;;
  nightly*)
    lapce_new_version="${lapce_new_ver}"
    lapce_new_ver_tag=$(echo "${lapce_new_ver}" | cut -d '-' -f1)
  ;;
  *)
    eprintln 'Unknown version\n'
    exit 1
  ;;
esac

lapce_new_ver_base_major=$(echo "${lapce_new_version}" | cut -d'.' -f1)
lapce_new_ver_base_minor=$(echo "${lapce_new_version}" | cut -d'.' -f2)
lapce_new_ver_base_patch=$(echo "${lapce_new_version}" | cut -d'.' -f3)

test_lapce_ver() {
  proxy_path="${1}"
  rm_proxy="${2:-0}"

  if test_cmd "lapce-proxy"; then
    lapce_ver_installed=$(lapce-proxy --version | cut -d' ' -f2)
    lapce_ver_base=$(echo "${lapce_ver_installed}" | cut -d'+' -f1)
    lapce_ver_base_major=$(echo "${lapce_ver_base}" | cut -d'.' -f1)
    lapce_ver_base_minor=$(echo "${lapce_ver_base}" | cut -d'.' -f2)
    lapce_ver_base_patch=$(echo "${lapce_ver_base}" | cut -d'.' -f3)

    eprintln '[DEBUG]: Current proxy version: %s\n' "${lapce_installed_ver}"
    eprintln '[DEBUG]: New proxy version: %s\n' "${lapce_new_version}"

    # Installed proxy is older than required
    if $(( $lapce_ver_base_major > $lapce_new_ver_base_major )); then
      if [ $rm_proxy = 1 ]; then
        rm $proxy_path
      fi
      return
    fi

    if $(( $lapce_ver_base_minor > $lapce_new_ver_base_minor )); then
      if [ $rm_proxy = 1 ]; then
        rm $proxy_path
      fi
      return
    fi

    if $(( $lapce_ver_base_patch > $lapce_new_ver_base_patch )); then
      if [ $rm_proxy = 1 ]; then
        rm $proxy_path
      fi
      return
    fi

    eprintln 'Proxy already exists\n'
    printf "${proxy_path}"
    exit 0
  fi
}

test_lapce_ver "lapce-proxy"

test_lapce_ver "/usr/libexec/lapce-proxy"

test_lapce_ver "${lapce_dir}/lapce"

if [ -e "${lapce_dir}/lapce" ]; then
  lapce_installed_ver=$("${lapce_dir}/lapce" --version | cut -d' ' -f2)

  eprintln '[DEBUG]: Current proxy version: %s\n' "${lapce_installed_ver}"
  eprintln '[DEBUG]: New proxy version: %s\n' "${lapce_new_version}"
  if [ "${lapce_installed_ver}" = "${lapce_new_version}" ]; then
    eprintln 'Proxy already exists\n'
    exit 0
  else
    eprintln 'Proxy outdated. Replacing proxy\n'
    rm "${lapce_dir}/lapce"
  fi
fi

for _cmd in tar gzip uname; do
  if ! test_cmd "${_cmd}"; then
    eprintln 'Missing required command: %s\n' "${_cmd}"
    exit 1
  fi
done

# Currently only linux/darwin are supported
# This is used only for our distribution of proxy
case $(uname -s) in
  Linux) os_name=linux ;;
  Darwin) os_name=darwin ;;
  *)
    eprintln '[ERROR] unsupported os\n'
    exit 1
  ;;
esac

# Currently only amd64/arm64 are supported
case $(uname -m) in
  x86_64|amd64|x64) arch_name=x86_64 ;;
  arm64|aarch64) arch_name=aarch64 ;;
  # riscv64) arch_name=riscv64 ;;
  *)
    eprintln '[ERROR] unsupported arch\n'
    exit 1
  ;;
esac

lapce_download_url="https://github.com/lapce/lapce/releases/download/${lapce_new_ver_tag}/lapce-proxy-${os_name}-${arch_name}.gz"

eprintln 'Creating "%s"\n' "${lapce_dir}"
mkdir -p "${lapce_dir}"
cd "${lapce_dir}"

if test_cmd 'curl'; then
  eprintln 'Downloading using curl\n'
  curl --proto '=https' --tlsv1.2 -LfS -O "${lapce_download_url}"
elif test_cmd 'wget'; then
  eprintln 'Downloading using wget\n'
  wget "${lapce_download_url}"
else
  eprintln 'curl/wget not found, failed to download proxy\n'
  exit 1
fi

eprintln 'Decompressing gzip\n'
gzip -df "${lapce_dir}/lapce-proxy-${os_name}-${arch_name}.gz"

eprintln 'Renaming proxy \n'
mv -v "${lapce_dir}/lapce-proxy-${os_name}-${arch_name}" "${lapce_dir}/lapce"

eprintln 'Making it executable\n'
chmod +x "${lapce_dir}/lapce"

eprintln 'lapce-proxy installed\n'

# Return proxy path to lapce
eprintln "${lapce_dir}/lapce"

exit 0
