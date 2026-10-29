#!/usr/bin/env bash
# Abre o app instalado por ./install.sh. Os modelos de IA baixam na primeira vez.
set -euo pipefail
cd "$(dirname "$0")"
exec venv/bin/python run.py "$@"
