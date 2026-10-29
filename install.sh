#!/usr/bin/env bash
# Instalação pelo código-fonte no macOS (e Linux).
#
#   ./install.sh        cria o ambiente em venv/ e instala as dependências
#   ./run.sh            abre o app
#
# No Mac, quem só quer usar o app deve preferir o instalador (.dmg) do
# Release; este script é para quem vai mexer no código.
set -euo pipefail
cd "$(dirname "$0")"

# Python 3.11–3.13 (o instalador usa 3.12).
PYTHON=""
for candidate in python3.12 python3.11 python3.13 python3; do
    if command -v "$candidate" >/dev/null 2>&1; then
        version=$("$candidate" -c 'import sys; print(f"{sys.version_info[0]}.{sys.version_info[1]}")')
        case "$version" in 3.11|3.12|3.13) PYTHON="$candidate"; break ;; esac
    fi
done
if [ -z "$PYTHON" ]; then
    echo "Python 3.11–3.13 não encontrado. No Mac: brew install python@3.12" >&2
    exit 1
fi

# O insightface compila uma extensão em C: no Mac precisa das ferramentas de
# linha de comando do Xcode.
if [ "$(uname)" = "Darwin" ] && ! xcode-select -p >/dev/null 2>&1; then
    echo "Instalando as Ferramentas de Linha de Comando do Xcode — conclua a janela que abriu e rode ./install.sh de novo."
    xcode-select --install || true
    exit 1
fi

echo "[1/2] Criando o ambiente com $PYTHON..."
[ -x venv/bin/python ] || "$PYTHON" -m venv venv

echo "[2/2] Instalando as dependências (alguns minutos)..."
venv/bin/python -m pip install --upgrade pip setuptools wheel
# numpy e Cython antes: o insightface os importa ao compilar.
venv/bin/python -m pip install "$(grep -E '^numpy' requirements.txt | head -1)" Cython
venv/bin/python -m pip install -r requirements.txt

echo
echo "Pronto. Abra o app com: ./run.sh"
if [ "$(uname)" = "Darwin" ]; then
    echo "Câmera virtual no Mac: instale o OBS Studio e ative a câmera virtual uma vez nele."
fi
