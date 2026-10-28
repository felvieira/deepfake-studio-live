"""Tela de carregamento.

Entre o clique no atalho e a janela principal aparecer passam de 5 a 30
segundos (carregar OpenCV, insightface e onnxruntime; mais na primeira vez,
com o disco frio e o antivírus lendo tudo). Sem nada na tela nesse tempo,
parece que o atalho não funcionou e a pessoa clica de novo. Esta tela aparece
em cerca de um segundo, antes dos imports pesados, e some quando a janela
principal abre — ou quando o app desiste de abrir (modules.core).
"""

from __future__ import annotations

import os

_splash = None


def show() -> None:
    """Mostra a tela de carregamento. Nunca derruba o app: sem tela (modo
    linha de comando, CI sem display) ou com qualquer erro, só não aparece."""
    global _splash
    if _splash is not None or os.environ.get("DSL_NO_SPLASH") == "1":
        return
    try:
        from PySide6.QtCore import QRectF, Qt
        from PySide6.QtGui import QColor, QFont, QPainter, QPixmap
        from PySide6.QtWidgets import QSplashScreen

        import modules.metadata
        from modules.branding import brand_file, get_application
        from modules.gettext import LanguageManager, system_language

        app = get_application()
        width, height = 440, 220
        ratio = app.primaryScreen().devicePixelRatio() if app.primaryScreen() else 1.0
        pixmap = QPixmap(int(width * ratio), int(height * ratio))
        pixmap.setDevicePixelRatio(ratio)
        pixmap.fill(QColor("#1e1e1e"))

        painter = QPainter(pixmap)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing)
        painter.setRenderHint(QPainter.RenderHint.SmoothPixmapTransform)
        painter.setPen(QColor("#333333"))
        painter.drawRect(QRectF(0.5, 0.5, width - 1, height - 1))
        icon = QPixmap(brand_file("icon-128.png"))
        if not icon.isNull():
            painter.drawPixmap(QRectF(36, 64, 92, 92), icon, QRectF(icon.rect()))
        painter.setPen(QColor("#e6e6e6"))
        title_font = QFont()
        title_font.setPointSizeF(17)
        title_font.setBold(True)
        painter.setFont(title_font)
        painter.drawText(QRectF(148, 74, width - 170, 40), Qt.AlignmentFlag.AlignVCenter, modules.metadata.name)
        painter.setPen(QColor("#9aa3b2"))
        body_font = QFont()
        body_font.setPointSizeF(10.5)
        painter.setFont(body_font)
        loading = LanguageManager(system_language())._("Loading…")
        painter.drawText(QRectF(148, 112, width - 170, 30), Qt.AlignmentFlag.AlignVCenter, loading)
        painter.end()

        _splash = QSplashScreen(pixmap, Qt.WindowType.WindowStaysOnTopHint)
        _splash.show()
        app.processEvents()
    except Exception as exc:  # noqa: BLE001 — a tela é cortesia, não requisito
        print(f"[splash] {exc}", flush=True)
        _splash = None


def close(main_window=None) -> None:
    """Fecha a tela de carregamento (esperando a janela principal pintar,
    quando ela é passada)."""
    global _splash
    if _splash is None:
        return
    try:
        if main_window is not None:
            _splash.finish(main_window)
        else:
            _splash.close()
    except Exception:  # noqa: BLE001
        pass
    _splash = None
