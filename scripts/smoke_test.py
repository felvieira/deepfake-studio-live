"""Teste de fumaça de uma instalação real (usado pelo CI em Windows e macOS).

Roda com o Python da instalação, de dentro da pasta do app:

    <python da instalação> scripts/smoke_test.py <pasta-de-saída>

Confere o que precisa funcionar numa máquina limpa — ffmpeg, leitura de
vídeo, modelos carregando no acelerador do sistema e fazendo uma inferência
— e salva prints da interface em <pasta-de-saída>. Sai com código 1 na
primeira falha.
"""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile
import time

APP_DIR = os.getcwd()
sys.path.insert(0, APP_DIR)
OUT = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else "smoke-output")
os.makedirs(OUT, exist_ok=True)

# O mesmo preâmbulo de DLLs do run.py (no Windows, as bibliotecas CUDA).
_preamble = open(os.path.join(APP_DIR, "run.py"), encoding="utf-8").read().split("from modules import platform_info")[0]
exec(compile(_preamble.replace("if __name__ == '__main__'", "if False"), "run.py", "exec"))


def check(label: str, ok: bool, detail: str = "") -> None:
    print(f"[{'ok' if ok else 'FALHOU'}] {label}" + (f" — {detail}" if detail else ""), flush=True)
    if not ok:
        sys.exit(1)


from modules import platform_info  # noqa: E402

platform_info.print_banner()

# --- ffmpeg -----------------------------------------------------------------
from modules.ffmpeg_tools import ffmpeg_path, ffprobe_path  # noqa: E402
from modules import utilities  # noqa: E402

ffmpeg = ffmpeg_path()
check("ffmpeg encontrado", bool(ffmpeg), str(ffmpeg))
encoders = subprocess.run([ffmpeg, "-hide_banner", "-encoders"], capture_output=True, text=True).stdout
wanted = ["libx264"] + (["h264_videotoolbox"] if sys.platform == "darwin" else [])
for enc in wanted:
    check(f"codificador {enc}", f" {enc} " in encoders)
print("ffprobe:", ffprobe_path())

video = os.path.join(tempfile.gettempdir(), "dsl_smoke.mp4")
subprocess.run(
    [ffmpeg, "-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i",
     "testsrc=size=320x240:rate=25", "-t", "2", "-pix_fmt", "yuv420p", video],
    check=True,
)
check("fps do vídeo", abs(utilities.detect_fps(video) - 25) < 0.5)
check("tamanho do vídeo", utilities.get_video_dimensions(video) == (320, 240))
check("quadros do vídeo", 45 <= utilities.estimate_frame_count(video) <= 55)

# --- app: argumentos e checagens de abertura ---------------------------------
sys.argv = ["run.py"]
from modules import core  # noqa: E402
import modules.globals  # noqa: E402

core.parse_args()
print("providers escolhidos:", modules.globals.execution_providers, "| idioma:", modules.globals.lang)
check("pre_check do app", core.pre_check())

expected = {"darwin": "CoreMLExecutionProvider"}.get(sys.platform)
if expected:
    check(f"acelerador {expected} selecionado", expected in modules.globals.execution_providers)

# --- modelos: carga e inferência ----------------------------------------------
import numpy as np  # noqa: E402
from modules.face_analyser import get_face_analyser  # noqa: E402
from modules.processors.frame import face_swapper  # noqa: E402

t = time.time()
analyser = get_face_analyser()
swapper = face_swapper.get_face_swapper()
check("modelos carregados", analyser is not None and swapper is not None, f"{time.time() - t:.1f}s")

from modules import ui  # noqa: E402

used = sorted(set(ui._runtime_providers()))
print("providers em uso pelos modelos:", used)
if expected:
    check(f"modelos rodando em {expected}", expected in used)

session = swapper.session
inputs = session.get_inputs()
feed = {}
for spec in inputs:
    shape = [d if isinstance(d, int) and d > 0 else 1 for d in spec.shape]
    feed[spec.name] = np.random.rand(*shape).astype(np.float32)
t = time.time()
session.run(None, feed)
check("inferência do swapper", True, f"{(time.time() - t) * 1000:.0f} ms")

frame = (np.random.rand(480, 640, 3) * 255).astype(np.uint8)
faces = analyser.get(frame)
check("detector de rostos roda", isinstance(faces, list), f"{len(faces)} rostos no ruído")

# --- interface ---------------------------------------------------------------
os.environ["DSL_NO_SPLASH"] = "1"
window = ui.init(lambda: None, lambda *a, **k: None, modules.globals.lang)
main = window._main
main.show()


def pump(seconds: float) -> None:
    end = time.time() + seconds
    while time.time() < end:
        ui._APP.processEvents()
        time.sleep(0.02)


pump(3)
main.resize(1040, 900)
main._select_mode(0)
pump(0.5)
main.grab().save(os.path.join(OUT, "app-arquivo.png"))
main.advanced_toggle.setChecked(True)
main.resize(1040, 1200)
pump(0.5)
main.grab().save(os.path.join(OUT, "app-avancado.png"))
main.advanced_toggle.setChecked(False)
main._select_mode(1)
main.resize(1040, 900)
main._refresh_accelerator_badge()
pump(0.5)
main.grab().save(os.path.join(OUT, "app-live.png"))
print("selo:", ui._accelerator_info(main._gpu_name))
check("prints salvos", all(os.path.isfile(os.path.join(OUT, f)) for f in ("app-arquivo.png", "app-avancado.png", "app-live.png")))
print("SMOKE OK", flush=True)
os._exit(0)
