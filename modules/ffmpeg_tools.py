"""Onde estão o ffmpeg e o ffprobe nesta máquina.

O app chamava `ffmpeg`/`ffprobe` pelo nome e exigia que estivessem no PATH.
Numa máquina limpa eles não existem: o app escrevia "ffmpeg is not installed."
e fechava, e quem tinha o ffmpeg instalado à parte (Chocolatey, Homebrew)
nunca via o problema. Agora a ordem é:

1. o ffmpeg do sistema, se houver (costuma ter mais codificadores de hardware);
2. o binário que o pacote `imageio-ffmpeg` traz para o próprio sistema
   (Windows, macOS Intel/Apple Silicon, Linux) — vem pelo pip junto com as
   outras dependências, então qualquer forma de instalação o recebe.

O `imageio-ffmpeg` não traz o ffprobe. Por isso quem precisa de informação
do vídeo (fps, tamanho, quadros) usa o ffprobe quando existe e cai para o
OpenCV quando não — ver modules.utilities.
"""

from __future__ import annotations

import os
import shutil
from functools import lru_cache
from typing import Optional

_EXE_SUFFIX = ".exe" if os.name == "nt" else ""


@lru_cache(maxsize=None)
def ffmpeg_path() -> Optional[str]:
    """Caminho do ffmpeg, ou None se não houver nenhum."""
    found = shutil.which("ffmpeg")
    if found:
        return found
    try:
        import imageio_ffmpeg

        bundled = imageio_ffmpeg.get_ffmpeg_exe()
    except Exception:  # noqa: BLE001 — pacote ausente ou binário faltando
        return None
    return bundled if bundled and os.path.isfile(bundled) else None


@lru_cache(maxsize=None)
def ffprobe_path() -> Optional[str]:
    """Caminho do ffprobe, ou None. Procura no PATH e ao lado do ffmpeg."""
    found = shutil.which("ffprobe")
    if found:
        return found
    ffmpeg = ffmpeg_path()
    if ffmpeg:
        sibling = os.path.join(os.path.dirname(ffmpeg), "ffprobe" + _EXE_SUFFIX)
        if os.path.isfile(sibling):
            return sibling
    return None


def ffmpeg_command() -> str:
    """Executável para montar uma linha de comando do ffmpeg.

    Cai para o nome "ffmpeg" quando nada foi encontrado, para a falha
    aparecer como "arquivo não encontrado" no ponto de uso — mas
    modules.core.pre_check já impede o app de chegar aqui sem ffmpeg.
    """
    return ffmpeg_path() or "ffmpeg"
