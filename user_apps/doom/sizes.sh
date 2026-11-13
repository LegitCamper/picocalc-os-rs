#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"
python3 build.py
python3 check.py
