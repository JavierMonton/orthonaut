#!/usr/bin/env bash
set -euo pipefail

BASE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DICT_DIR="${BASE_DIR}/backend/dictionaries"

mkdir -p "${DICT_DIR}"

DIC_URL="https://raw.githubusercontent.com/elastic/hunspell/master/dicts/es_ES/es_ES.dic"
AFF_URL="https://raw.githubusercontent.com/elastic/hunspell/master/dicts/es_ES/es_ES.aff"

echo "Downloading es_ES.dic..."
curl -fsSL "${DIC_URL}" -o "${DICT_DIR}/es_ES.dic"
echo "Downloading es_ES.aff..."
curl -fsSL "${AFF_URL}" -o "${DICT_DIR}/es_ES.aff"
echo "Dictionary files downloaded to ${DICT_DIR}"
