import json
from pathlib import Path

_LOCALES_DIR = Path(__file__).parent.parent / "locales"


def available_languages() -> list[str]:
    """Códigos com tradução disponível (os arquivos em locales/), mais "en"."""
    return ["en"] + sorted(p.stem for p in _LOCALES_DIR.glob("*.json"))


def system_language() -> str:
    """Idioma da interface a partir das preferências do sistema.

    Percorre os idiomas preferidos do usuário e o formato regional (Windows
    e macOS expõem os dois pelo Qt) e devolve a primeira tradução que
    existir — "pt-BR" vira "pt-br", "de-DE" vira "de". Inglês é o padrão do
    app, então só é escolhido quando nada mais combina.

    Antes o idioma só mudava com `--lang`, que o instalador do Windows
    passava lendo o idioma do sistema por conta própria; decidir aqui vale
    para qualquer forma de abrir o app, em qualquer sistema.
    """
    available = set(available_languages())
    try:
        from PySide6.QtCore import QLocale

        system = QLocale.system()
        # Idiomas de exibição e, depois, o formato regional. Quem usa o
        # sistema em inglês com região Brasil (comum) recebe português —
        # era o que o instalador já fazia lendo a região do Windows.
        preferred = list(system.uiLanguages()) + [system.name()]
    except Exception:  # noqa: BLE001 — sem Qt, fica o padrão
        preferred = []
    for tag in preferred:
        tag = tag.lower().replace("_", "-")
        base = tag.split("-")[0]
        if base == "en":
            continue
        if tag in available:
            return tag
        regional = sorted(code for code in available if code.split("-")[0] == base)
        if regional:
            # "pt-pt" não tem arquivo próprio: o mais próximo é "pt-br".
            return regional[0]
    return "en"

class LanguageManager:
    def __init__(self, default_language="en"):
        self.current_language = default_language
        self.translations = {}
        self.load_language(default_language)

    def load_language(self, language_code) -> bool:
        """load language file"""
        if language_code == "en":
            return True
        try:
            file_path = Path(__file__).parent.parent / f"locales/{language_code}.json"
            with open(file_path, "r", encoding="utf-8") as file:
                self.translations = json.load(file)
            self.current_language = language_code
            return True
        except FileNotFoundError:
            print(f"Language file not found: {language_code}")
            return False

    def _(self, key, default=None) -> str:
        """get translate text"""
        return self.translations.get(key, default if default else key)