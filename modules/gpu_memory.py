"""Liberar a GPU depois que o trabalho acaba.

Os modelos ficam em variáveis globais de cada módulo (swapper, detector,
enhancers). Sem ninguém zerá-las, cada sessão do onnxruntime continua viva —
e com ela a memória de vídeo e o contexto CUDA — até o programa fechar, mesmo
com o live parado e nada sendo processado. Aqui elas são soltas e o coletor
de lixo roda, o que destrói as sessões e devolve a VRAM ao sistema.

Só mexe em módulos que já foram importados: descarregar nunca deve importar
(e portanto carregar) nada.
"""

from __future__ import annotations

import gc
import sys
from typing import Any

# módulo -> {atributo: valor "vazio"}. dict/list vazios são esvaziados no
# lugar (outros módulos podem ter uma referência ao mesmo objeto).
_HOLDERS: dict[str, dict[str, Any]] = {
    "modules.processors.frame.face_swapper": {
        "FACE_SWAPPER": None,
        "_cuda_graph_session": {},
        "FACE_DETECTION_CACHE": {},
        "PREVIOUS_FRAME_RESULT": None,
    },
    "modules.face_analyser": {"FACE_ANALYSER": None},
    "modules.processors.frame.face_enhancer": {"FACE_ENHANCER": None},
    "modules.processors.frame.face_enhancer_gpen256": {"ENHANCER": None},
    "modules.processors.frame.face_enhancer_gpen512": {"ENHANCER": None},
    "modules.predicter": {"model": None},
}


def models_loaded() -> bool:
    """Há algum modelo carregado agora?"""
    for module_name, attrs in _HOLDERS.items():
        module = sys.modules.get(module_name)
        if module is None:
            continue
        for attr, empty in attrs.items():
            if empty is None and getattr(module, attr, None) is not None:
                return True
    return False


def unload_models() -> bool:
    """Solta todos os modelos e devolve a memória da GPU.

    Devolve True se havia algo carregado. Não deve ser chamada enquanto o
    live ou um processamento estiver usando os modelos.
    """
    had_models = models_loaded()
    for module_name, attrs in _HOLDERS.items():
        module = sys.modules.get(module_name)
        if module is None:
            continue
        for attr, empty in attrs.items():
            current = getattr(module, attr, None)
            if isinstance(empty, dict) and isinstance(current, dict):
                current.clear()
            elif current is not None or empty is not None:
                try:
                    setattr(module, attr, empty)
                except AttributeError:
                    pass

    gc.collect()
    # O torch (quando instalado) guarda um cache próprio de memória da GPU.
    torch = sys.modules.get("torch")
    if torch is not None:
        try:
            if torch.cuda.is_available():
                torch.cuda.empty_cache()
        except Exception:  # noqa: BLE001 — liberar é melhor-esforço
            pass
    return had_models
