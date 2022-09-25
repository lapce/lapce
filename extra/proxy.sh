#!/bin/sh
set -eux

# This script is written to be as POSIX as possible
# so it works fine for all Unix-like operating systems

test_cmd() {
  command -v "$1" >/dev/null
}

# Using variables prefixed with _
# to avoid clashing, POSIX doesn't
# have 'local' keyword

_TMP='/tmp'
# proxy version
_VER="${1}"
# proxy directory
# eval to resolve '~' into proper user dir
eval _DIR="'${2}'"

if [ -e "${_DIR}/lapce" ]; then
  chmod +x "${_DIR}/lapce"

  _ver=$("${_DIR}/lapce" --version | cut -d'v' -f2)

  printf '[DEBUG]: %s = %s' "${_ver}" "${_VER}"
  if [ "${_ver}" = "${_VER}" ]; then
    printf 'Proxy outdated. Replacing proxy\n'
    rm "${_DIR}/lapce"
  else
    printf 'Proxy already exists\n'
    exit 0
  fi
fi

for _cmd in tar gzip uname; do
  if ! test_cmd "${_cmd}"; then
    printf 'Missing required command: %s\n' "${_cmd}"
    exit 1
  fi
done

# Currently only linux/darwin are supported
_OS="$(uname -s)"
if [ "${_OS}" = "Linux" ]; then
  _OS=linux
elif [ "${_OS}" = "Darwin" ]; then
  _OS=darwin
fi

# Currently only amd64/arm64 are supported
_ARCH="$(uname -m)"
if [ "${_ARCH}" = "x86_64" ]; then
  _ARCH=x86_64
elif [ "${_ARCH}" = "arm64" ]; then
  _ARCH=aarch64
fi

printf 'Switching to "%s"\n' "${_TMP}"
cd "${_TMP}"

_URL="https://github.com/lapce/lapce/releases/download/${_VER}/lapce-proxy-${_OS}-${_ARCH}.gz"

if test_cmd 'curl'; then
  # How old curl has these options? we'll find out
  printf 'Downloading using curl\n'
  curl --proto '=https' --tlsv1.2 -LZfS -O "${_URL}"
  # curl --proto '=https' --tlsv1.2 -LZfS -o "${_TMP}/lapce-proxy-${_OS}-${_ARCH}.gz" "${_URL}"
elif test_cmd 'wget'; then
  printf 'Downloading using wget\n'
  wget "${_URL}"
else
  printf 'curl/wget not found, failed to download proxy\n'
  exit 1
fi

printf 'Creating "%s"\n' "${_DIR}"
mkdir -p "${_DIR}"

printf 'Decompressing gzip\n'
gzip -d "${_TMP}/lapce-proxy-${_OS}-${_ARCH}.gz"

printf 'Moving proxy to our dir\n'
mv -v "${_TMP}/lapce-proxy-${_OS}-${_ARCH}" "${_DIR}/lapce"

printf 'Making it executable\n'
chmod +x "${_DIR}/lapce"

printf 'lapce-proxy installed\n'

exit 0
