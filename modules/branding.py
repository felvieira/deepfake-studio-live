"""Identidade do app: ícone, nome e AppUserModelID.

Fica separado de modules.ui porque a tela de carregamento (modules.splash)
precisa disto antes dos imports pesados — importar modules.ui puxa OpenCV,
insightface e onnxruntime, que são justamente a demora que ela cobre.
"""

from __future__ import annotations

import os
import sys

import modules.metadata

_BRAND_DIR = os.path.join(
    os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "media", "brand"
)
_ICON_SIZES = (16, 32, 48, 64, 128, 256, 512)


def brand_file(name: str) -> str:
    """Caminho de um arquivo em media/brand/."""
    return os.path.join(_BRAND_DIR, name)


def load_app_icon():
    """Ícone do app, montado com todos os tamanhos de media/brand/.

    Vários tamanhos deixam o sistema escolher o nítido para cada lugar
    (barra de título, barra de tarefas, Dock, Alt+Tab) em vez de reduzir um
    só. Faltar arquivo não é erro: o app abre com o ícone padrão do Qt.
    """
    from PySide6.QtCore import QSize
    from PySide6.QtGui import QIcon

    icon = QIcon()
    for size in _ICON_SIZES:
        path = brand_file(f"icon-{size}.png")
        if os.path.isfile(path):
            icon.addFile(path, QSize(size, size))
    return icon


def set_windows_app_id() -> None:
    """Sem um AppUserModelID próprio, o Windows agrupa a janela sob o
    python.exe e mostra o ícone dele na barra de tarefas. Precisa rodar antes
    da primeira janela aparecer."""
    if sys.platform != "win32":
        return
    try:
        import ctypes

        ctypes.windll.shell32.SetCurrentProcessExplicitAppUserModelID(
            modules.metadata.app_id
        )
    except (AttributeError, OSError):
        pass


def get_application():
    """O QApplication do processo, criado com a identidade do app se ainda
    não existir. Quem chegar primeiro (tela de carregamento ou janela
    principal) cria; o outro reaproveita."""
    from PySide6.QtWidgets import QApplication

    app = QApplication.instance()
    if app is None:
        set_windows_app_id()
        app = QApplication(sys.argv)
    app.setApplicationName(modules.metadata.name)
    app.setWindowIcon(load_app_icon())
    return app
