"""PySide6 UI for Deep-Live-Cam.

Public API kept stable for the rest of the codebase:
    init(start, destroy, lang) -> _Window
        Returned object has .mainloop() that core.py calls.
    update_status(text)
        Thread-safe; routed through Qt signal when called off-UI.
    check_and_ignore_nsfw(target, destroy=None) -> bool
"""

from __future__ import annotations

import os
import platform
import queue
import sys
import tempfile
import threading
import time
import webbrowser
from typing import Callable, List, Optional, Tuple

import cv2
import numpy as np
import requests
try:
    import pyvirtualcam
except ImportError:
    pyvirtualcam = None

from PIL import Image, ImageOps
from PySide6.QtCore import (
    QObject,
    QThread,
    QTimer,
    Qt,
    Signal,
)
from PySide6.QtGui import QImage, QPixmap
from PySide6.QtWidgets import (
    QApplication,
    QCheckBox,
    QComboBox,
    QDialog,
    QFileDialog,
    QGridLayout,
    QGroupBox,
    QHBoxLayout,
    QLabel,
    QMainWindow,
    QPushButton,
    QScrollArea,
    QSizePolicy,
    QSlider,
    QVBoxLayout,
    QStackedWidget,
    QToolButton,
    QWidget,
)

import modules.globals
import modules.metadata
from modules.capturer import get_video_frame, get_video_frame_total
from modules.face_analyser import (
    add_blank_map,
    detect_many_faces_fast,
    detect_one_face_fast,
    ensure_landmarks,
    get_one_face,
    get_unique_faces_from_target_image,
    get_unique_faces_from_target_video,
    has_valid_map,
    simplify_maps,
)
from modules.gettext import LanguageManager
from modules.gpu_processing import gpu_cvt_color, gpu_flip, gpu_resize
from modules.processors.frame.core import get_frame_processors_modules
from modules.utilities import (
    has_image_extension,
    is_image,
    is_video,
)
from modules import imread_unicode
from modules.video_capture import VideoCapturer

if platform.system() == "Windows":
    from pygrabber.dshow_graph import FilterGraph

import json


# ─── constants ────────────────────────────────────────────────────────────

ROOT_HEIGHT = 820
ROOT_WIDTH = 640

PREVIEW_MAX_HEIGHT = 700
PREVIEW_MAX_WIDTH = 1200
PREVIEW_DEFAULT_WIDTH = 640
PREVIEW_DEFAULT_HEIGHT = 360

POPUP_WIDTH = 750
POPUP_HEIGHT = 810
POPUP_SCROLL_WIDTH = 720
POPUP_SCROLL_HEIGHT = 700

POPUP_LIVE_WIDTH = 900
POPUP_LIVE_HEIGHT = 820
POPUP_LIVE_SCROLL_WIDTH = 870
POPUP_LIVE_SCROLL_HEIGHT = 700

MAPPER_PREVIEW_SIZE = 100
SOURCE_TARGET_PREVIEW_SIZE = 200


# ─── modern dark stylesheet ───────────────────────────────────────────────

QSS = """
QMainWindow, QDialog { background-color: #1e1e1e; color: #e6e6e6; }
QWidget { color: #e6e6e6; font-family: "Segoe UI", "SF Pro Display", "Helvetica Neue", Arial, sans-serif; font-size: 11pt; }

QGroupBox {
    background-color: #262626;
    border: 1px solid #333333;
    border-radius: 10px;
    margin-top: 14px;
    padding-top: 18px;
    font-weight: 600;
}
QGroupBox::title {
    subcontrol-origin: margin;
    subcontrol-position: top left;
    padding: 0 8px;
    color: #9ec5ff;
}

QPushButton {
    background-color: #2d6cdf;
    color: white;
    border: none;
    border-radius: 8px;
    padding: 8px 16px;
    font-weight: 600;
}
QPushButton:hover  { background-color: #3a7af0; }
QPushButton:pressed{ background-color: #1d57c2; }
QPushButton:disabled { background-color: #444; color: #888; }
QPushButton#primary {
    background-color: #2d6cdf;
    padding: 10px 24px;
    font-size: 12pt;
}
QPushButton#modeTab {
    background-color: #292929;
    border: 1px solid #444;
    padding: 12px 22px;
}
QPushButton#modeTab:checked { background-color: #2d6cdf; border-color: #6ea8ff; }
QPushButton#secondary {
    background-color: #3a3a3a;
}
QPushButton#secondary:hover { background-color: #4a4a4a; }
QPushButton#danger { background-color: #c2412d; }
QPushButton#danger:hover  { background-color: #d8523c; }
QToolButton#advancedToggle {
    background-color: #262626;
    border: 1px solid #333;
    border-radius: 8px;
    padding: 10px 12px;
    text-align: left;
    font-weight: 600;
}
QToolButton#advancedToggle:hover { border-color: #2d6cdf; }
QLabel#appTitle { font-size: 22pt; font-weight: 700; }
QLabel#appSubtitle { color: #a0a0a0; font-size: 10pt; }
QLabel#sectionTitle { font-size: 14pt; font-weight: 700; }
QLabel#sectionDescription { color: #a8a8a8; }
QLabel#accelerator {
    color: #9ee493;
    background-color: #17351e;
    border-radius: 8px;
    padding: 8px 14px;
    font-weight: 600;
}
QLabel#flowHint {
    color: #9ec5ff;
    background-color: #242b36;
    border-radius: 6px;
    padding: 8px 12px;
}
QLabel#outputDevice { color: #9ec5ff; font-weight: 600; }

QComboBox {
    background-color: #2a2a2a;
    border: 1px solid #404040;
    border-radius: 6px;
    padding: 6px 10px;
    min-height: 24px;
}
QComboBox:hover { border-color: #2d6cdf; }
QComboBox QAbstractItemView {
    background-color: #2a2a2a;
    selection-background-color: #2d6cdf;
    border: 1px solid #404040;
}

QCheckBox {
    spacing: 8px;
    padding: 4px 0;
}
QCheckBox::indicator {
    width: 36px; height: 18px;
    border-radius: 9px;
    background-color: #3a3a3a;
}
QCheckBox::indicator:checked {
    background-color: #2d6cdf;
}

QSlider::groove:horizontal {
    height: 6px;
    background: #3a3a3a;
    border-radius: 3px;
}
QSlider::handle:horizontal {
    background: #ffffff;
    width: 16px; height: 16px;
    margin: -5px 0;
    border-radius: 8px;
    border: 1px solid #cccccc;
}
QSlider::sub-page:horizontal {
    background: #2d6cdf;
    border-radius: 3px;
}

QLabel#imageDrop {
    background-color: #2a2a2a;
    border: 2px dashed #444;
    border-radius: 8px;
}
QLabel#statusLabel {
    color: #b9b9b9;
    font-size: 10pt;
    font-style: italic;
}
QLabel#linkLabel {
    color: #6ea8ff;
    text-decoration: underline;
}

QScrollArea { border: none; background: transparent; }

QFrame#card {
    background-color: #262626;
    border-radius: 10px;
}
"""


# ─── module-level state ───────────────────────────────────────────────────

_APP: Optional[QApplication] = None
_MAIN: Optional["MainWindow"] = None
_PREVIEW: Optional["PreviewWindow"] = None
_WEBCAM_PREVIEW: Optional["WebcamPreviewWindow"] = None
_MAPPER: Optional["MapperDialog"] = None
_LIVE_MAPPER: Optional["LiveMapperDialog"] = None
_LANG: Optional[LanguageManager] = None
_BRIDGE: Optional["_UIBridge"] = None


def _(text: str) -> str:
    """Translate via LanguageManager; falls back to identity."""
    if _LANG is None:
        return text
    return _LANG._(text)


# Preserve original cwd state for file dialogs.
_RECENT_SOURCE_DIR: Optional[str] = None
_RECENT_TARGET_DIR: Optional[str] = None
_RECENT_OUTPUT_DIR: Optional[str] = None

# QFileDialog filter strings, built from the canonical extension sets in
# globals so every dialog stays in sync (no hand-copied lists to drift).
_IMAGE_FILE_FILTER = "Images (" + " ".join(
    f"*{ext}" for ext in modules.globals.IMAGE_EXTENSIONS
) + ")"
_MEDIA_FILE_FILTER = "Media (" + " ".join(
    f"*{ext}" for ext in (*modules.globals.IMAGE_EXTENSIONS, *modules.globals.VIDEO_EXTENSIONS)
) + ")"
_VIDEO_FILE_FILTER = "Videos (" + " ".join(
    f"*{ext}" for ext in modules.globals.VIDEO_EXTENSIONS
) + ")"


# ─── image utilities ─────────────────────────────────────────────────────


def fit_image_to_size(image, width: int, height: int):
    """BGR ndarray → BGR ndarray scaled to fit within (width, height)."""
    if width is None and height is None or width <= 0 or height <= 0:
        return image
    h, w = image.shape[:2]
    ratio_w = width / w
    ratio_h = height / h
    ratio = min(ratio_w, ratio_h)
    new_size = (max(1, int(w * ratio)), max(1, int(h * ratio)))
    return gpu_resize(image, dsize=new_size)


def _bgr_to_qpixmap(bgr: np.ndarray) -> QPixmap:
    """Zero-copy BGR ndarray → QPixmap."""
    h, w = bgr.shape[:2]
    rgb = cv2.cvtColor(bgr, cv2.COLOR_BGR2RGB)
    qimg = QImage(rgb.data, w, h, w * 3, QImage.Format.Format_RGB888).copy()
    return QPixmap.fromImage(qimg)


def _pil_to_qpixmap(image: Image.Image) -> QPixmap:
    """PIL.Image → QPixmap."""
    image = image.convert("RGBA")
    data = image.tobytes("raw", "RGBA")
    qimg = QImage(data, image.width, image.height, QImage.Format.Format_RGBA8888)
    return QPixmap.fromImage(qimg.copy())


def render_image_preview(image_path: str, size: Tuple[int, int]) -> QPixmap:
    image = Image.open(image_path)
    if size:
        image = ImageOps.fit(image, size, Image.LANCZOS)
    return _pil_to_qpixmap(image)


def render_video_preview(
    video_path: str, size: Tuple[int, int], frame_number: int = 0
) -> Optional[QPixmap]:
    capture = cv2.VideoCapture(video_path)
    try:
        if frame_number:
            capture.set(cv2.CAP_PROP_POS_FRAMES, frame_number)
        has_frame, frame = capture.read()
        if not has_frame:
            return None
        image = Image.fromarray(gpu_cvt_color(frame, cv2.COLOR_BGR2RGB))
        if size:
            image = ImageOps.fit(image, size, Image.LANCZOS)
        return _pil_to_qpixmap(image)
    finally:
        capture.release()


# ─── persistence ─────────────────────────────────────────────────────────


def save_switch_states():
    state = {
        "keep_fps": modules.globals.keep_fps,
        "keep_audio": modules.globals.keep_audio,
        "keep_frames": modules.globals.keep_frames,
        "many_faces": modules.globals.many_faces,
        "map_faces": modules.globals.map_faces,
        "poisson_blend": modules.globals.poisson_blend,
        "color_correction": modules.globals.color_correction,
        "virtual_camera_enabled": modules.globals.virtual_camera_enabled,
        "nsfw_filter": modules.globals.nsfw_filter,
        "live_mirror": modules.globals.live_mirror,
        "live_resizable": modules.globals.live_resizable,
        "fp_ui": modules.globals.fp_ui,
        "show_fps": modules.globals.show_fps,
        "mouth_mask": modules.globals.mouth_mask,
        "show_mouth_mask_box": modules.globals.show_mouth_mask_box,
        "mouth_mask_size": modules.globals.mouth_mask_size,
    }
    try:
        with open("switch_states.json", "w") as f:
            json.dump(state, f)
    except OSError:
        pass


def load_switch_states():
    try:
        with open("switch_states.json", "r") as f:
            state = json.load(f)
        modules.globals.keep_fps = state.get("keep_fps", True)
        modules.globals.keep_audio = state.get("keep_audio", True)
        modules.globals.keep_frames = state.get("keep_frames", False)
        modules.globals.many_faces = state.get("many_faces", False)
        modules.globals.map_faces = state.get("map_faces", False)
        modules.globals.poisson_blend = state.get("poisson_blend", False)
        modules.globals.color_correction = state.get("color_correction", False)
        modules.globals.virtual_camera_enabled = state.get("virtual_camera_enabled", False)
        modules.globals.nsfw_filter = state.get("nsfw_filter", False)
        modules.globals.live_mirror = state.get("live_mirror", False)
        modules.globals.live_resizable = state.get("live_resizable", False)
        modules.globals.fp_ui = state.get("fp_ui", {"face_enhancer": False})
        modules.globals.show_fps = state.get("show_fps", False)
        # Mouth mask always starts disabled (slider at 0) on launch,
        # regardless of the persisted value — enable it explicitly each session.
        modules.globals.mouth_mask_size = 0.0
        modules.globals.mouth_mask = False
        modules.globals.show_mouth_mask_box = False
    except FileNotFoundError:
        pass
    except (OSError, json.JSONDecodeError):
        pass


# ─── thread-safe status bridge ───────────────────────────────────────────


class _UIBridge(QObject):
    """Single QObject that owns cross-thread signals."""

    statusChanged = Signal(str)


def _emit_status(text: str) -> None:
    if _BRIDGE is None:
        print(text)
        return
    _BRIDGE.statusChanged.emit(text)


# ─── public API ──────────────────────────────────────────────────────────


def update_status(text: str) -> None:
    """Thread-safe status update — uses signal if called off-UI thread."""
    _emit_status(_(text))
    if _APP is not None and QThread.currentThread() is _APP.thread():
        # On UI thread — flush events so the user sees the update during
        # long synchronous start() runs.
        _APP.processEvents()


def check_and_ignore_nsfw(target, destroy: Optional[Callable] = None) -> bool:
    from numpy import ndarray
    from modules.predicter import predict_frame, predict_image, predict_video

    check_nsfw = None
    if isinstance(target, str):
        check_nsfw = predict_image if has_image_extension(target) else predict_video
    elif isinstance(target, ndarray):
        check_nsfw = predict_frame

    if check_nsfw and check_nsfw(target):
        if destroy:
            destroy(to_quit=False)
        update_status("Processing ignored!")
        return True
    return False


# ─── camera enumeration (unchanged from tk version) ──────────────────────


def get_available_cameras() -> Tuple[List[int], List[str]]:
    if platform.system() == "Windows":
        try:
            graph = FilterGraph()
            devices = graph.get_input_devices()
            if devices:
                return list(range(len(devices))), devices
            return [], ["No cameras found"]
        except Exception as exc:
            print(f"Error detecting cameras: {exc}")
            return [], ["No cameras found"]

    if platform.system() == "Darwin":
        return [0, 1], ["Camera 0", "Camera 1"]

    # Linux probe
    indices: List[int] = []
    names: List[str] = []
    for i in range(10):
        cap = cv2.VideoCapture(f"/dev/video{i}")
        if cap.isOpened():
            indices.append(i)
            names.append(f"Camera {i}")
            cap.release()
    return (indices, names) if names else ([], ["No cameras found"])


# ─── main window ─────────────────────────────────────────────────────────


def _make_image_drop(text: str, size: Tuple[int, int]) -> QLabel:
    label = QLabel(text)
    label.setObjectName("imageDrop")
    label.setAlignment(Qt.AlignmentFlag.AlignCenter)
    label.setFixedSize(size[0], size[1])
    label.setText(text)
    return label


class _Switch(QWidget):
    """Compact toggle switch with label + optional tooltip."""

    toggled = Signal(bool)

    def __init__(self, text: str, initial: bool, tooltip: str = ""):
        super().__init__()
        layout = QHBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)
        self._checkbox = QCheckBox(text)
        self._checkbox.setChecked(initial)
        self._checkbox.toggled.connect(self.toggled.emit)
        if tooltip:
            self._checkbox.setToolTip(tooltip)
        layout.addWidget(self._checkbox)
        layout.addStretch(1)

    def isChecked(self) -> bool:
        return self._checkbox.isChecked()

    def setChecked(self, value: bool) -> None:
        self._checkbox.setChecked(value)


class MainWindow(QMainWindow):
    def __init__(self, start_cb: Callable, destroy_cb: Callable):
        super().__init__()
        load_switch_states()
        self._start_cb = start_cb
        self._destroy_cb = destroy_cb
        self._source_labels: list[QLabel] = []

        self.setWindowTitle(
            f"{modules.metadata.name} {modules.metadata.version} {modules.metadata.edition}"
        )
        self.setMinimumSize(900, 760)
        self.resize(1040, 900)

        root = QWidget()
        self.setCentralWidget(root)
        layout = QVBoxLayout(root)
        layout.setContentsMargins(20, 18, 20, 14)
        layout.setSpacing(12)

        layout.addLayout(self._build_header())
        layout.addLayout(self._build_mode_selector())

        self._mode_stack = QStackedWidget()
        self._mode_stack.addWidget(self._build_file_page())
        self._mode_stack.addWidget(self._build_live_page())
        layout.addWidget(self._mode_stack, 1)

        advanced_toggle, advanced_panel = self._build_advanced_settings()
        layout.addWidget(advanced_toggle)
        layout.addWidget(advanced_panel)

        self._status_label = QLabel("")
        self._status_label.setObjectName("statusLabel")
        self._status_label.setAlignment(Qt.AlignmentFlag.AlignLeft)
        layout.addWidget(self._status_label)

        self._select_mode(0)
        self._update_action_states()
        self._set_context_status()

    def _build_header(self) -> QHBoxLayout:
        row = QHBoxLayout()
        title_box = QVBoxLayout()
        title = QLabel("Deep Live Cam")
        title.setObjectName("appTitle")
        subtitle = QLabel(_("AI face replacement"))
        subtitle.setObjectName("appSubtitle")
        title_box.addWidget(title)
        title_box.addWidget(subtitle)
        row.addLayout(title_box)
        row.addStretch(1)

        providers = modules.globals.execution_providers
        accelerator = _("CUDA ready") if "CUDAExecutionProvider" in providers else _("CPU mode")
        self._accelerator_label = QLabel(f"●  {accelerator}")
        self._accelerator_label.setObjectName("accelerator")
        row.addWidget(self._accelerator_label)
        return row

    def _build_mode_selector(self) -> QHBoxLayout:
        row = QHBoxLayout()
        row.addStretch(1)
        self.mode_file = QPushButton(_("File processing"))
        self.mode_live = QPushButton(_("Live camera"))
        for index, button in enumerate((self.mode_file, self.mode_live)):
            button.setObjectName("modeTab")
            button.setCheckable(True)
            button.setMinimumWidth(220)
            button.clicked.connect(lambda _checked, i=index: self._select_mode(i))
            row.addWidget(button)
        row.addStretch(1)
        return row

    def _select_mode(self, index: int) -> None:
        self.mode_file.setChecked(index == 0)
        self.mode_live.setChecked(index == 1)
        self._mode_stack.setCurrentIndex(index)
        self._update_action_states()
        self._set_context_status()

    def _build_face_card(
        self,
        title: str,
        description: str,
        placeholder: str,
        button_text: str,
        label_attr: str,
        source: bool,
    ) -> QGroupBox:
        card = QGroupBox()
        layout = QVBoxLayout(card)
        heading = QLabel(_(title))
        heading.setObjectName("sectionTitle")
        description_label = QLabel(_(description))
        description_label.setObjectName("sectionDescription")
        layout.addWidget(heading)
        layout.addWidget(description_label)

        image_label = _make_image_drop(_(placeholder), (300, 250))
        image_label._placeholder_text = _(placeholder)
        setattr(self, label_attr, image_label)
        if source:
            self._source_labels.append(image_label)
        layout.addWidget(image_label, alignment=Qt.AlignmentFlag.AlignCenter)

        choose = QPushButton(_(button_text))
        choose.clicked.connect(self._on_select_source if source else self._on_select_target)
        layout.addWidget(choose)
        if source and not hasattr(self, "btn_select_source"):
            self.btn_select_source = choose
        if not source:
            self.btn_select_target = choose
        return card

    def _build_file_page(self) -> QWidget:
        page = QWidget()
        grid = QGridLayout(page)
        grid.setHorizontalSpacing(14)
        grid.setVerticalSpacing(12)

        source_card = self._build_face_card(
            "Face to use",
            "This face will be applied to the target.",
            "Drop a face image here",
            "Choose image",
            "source_label_file",
            True,
        )
        target_card = self._build_face_card(
            "File to process",
            "Choose an image or video.",
            "Drop an image or video here",
            "Choose file",
            "target_label",
            False,
        )
        self.source_label = self.source_label_file

        grid.addWidget(source_card, 0, 0)
        grid.addWidget(target_card, 0, 2)

        self.btn_swap = QPushButton(_("Swap images"))
        self.btn_swap.setObjectName("secondary")
        self.btn_swap.setToolTip(_("Exchange the source and target images"))
        self.btn_swap.clicked.connect(self._on_swap_paths)
        grid.addWidget(self.btn_swap, 0, 1, alignment=Qt.AlignmentFlag.AlignCenter)

        actions = QHBoxLayout()
        actions.addStretch(1)
        self.btn_preview = QPushButton(_("Preview"))
        self.btn_preview.setObjectName("secondary")
        self.btn_preview.setToolTip(_("Preview the processed image or video"))
        self.btn_preview.clicked.connect(self._on_toggle_preview)
        self.btn_start = QPushButton(_("Process file"))
        self.btn_start.setObjectName("primary")
        self.btn_start.setToolTip(_("Process the selected image or video"))
        self.btn_start.clicked.connect(self._on_start)
        actions.addWidget(self.btn_preview)
        actions.addWidget(self.btn_start)
        grid.addLayout(actions, 1, 0, 1, 3)
        return page

    def _build_live_page(self) -> QWidget:
        page = QWidget()
        grid = QGridLayout(page)
        grid.setHorizontalSpacing(14)
        grid.setVerticalSpacing(12)

        source_card = self._build_face_card(
            "Face to use",
            "This face will be applied to the camera.",
            "Drop a face image here",
            "Replace image",
            "source_label_live",
            True,
        )
        grid.addWidget(source_card, 0, 0)

        setup = QGroupBox(_("Camera setup"))
        setup_layout = QVBoxLayout(setup)
        input_title = QLabel(_("Input camera"))
        input_title.setObjectName("sectionTitle")
        input_description = QLabel(_("Camera used as the video source"))
        input_description.setObjectName("sectionDescription")
        setup_layout.addWidget(input_title)
        setup_layout.addWidget(input_description)

        self._camera_indices, self._camera_names = get_available_cameras()
        self.cb_camera = QComboBox()
        if not self._camera_names or self._camera_names[0] == "No cameras found":
            self.cb_camera.addItem(_("No cameras found"))
            self.cb_camera.setEnabled(False)
        else:
            self.cb_camera.addItems(self._camera_names)
        self.cb_camera.setToolTip(_("Physical webcam used as the live input"))
        setup_layout.addWidget(self.cb_camera)

        self.camera_preview_label = _make_image_drop(_("Camera preview"), (520, 180))
        setup_layout.addWidget(self.camera_preview_label)

        output = QGroupBox(_("Virtual camera output"))
        output_layout = QVBoxLayout(output)
        self.sw_virtual_camera = _Switch(
            _("Send video to a virtual camera"),
            modules.globals.virtual_camera_enabled,
            _("Send processed live video to Zoom, Discord, or Teams"),
        )
        self.sw_virtual_camera.toggled.connect(self._on_virtual_camera_toggled)
        output_layout.addWidget(self.sw_virtual_camera)
        output_row = QHBoxLayout()
        output_row.addWidget(QLabel(_("Output device")))
        self.output_device_label = QLabel("Unity Video Capture")
        self.output_device_label.setObjectName("outputDevice")
        output_row.addWidget(self.output_device_label, 1)
        output_layout.addLayout(output_row)
        output_hint = QLabel(_("Select this camera in Discord, Zoom, or Teams."))
        output_hint.setObjectName("sectionDescription")
        output_layout.addWidget(output_hint)
        setup_layout.addWidget(output)
        grid.addWidget(setup, 0, 1)

        flow = QLabel(_("Input webcam  →  Face processing  →  Unity Video Capture"))
        flow.setObjectName("flowHint")
        grid.addWidget(flow, 1, 0, 1, 2)

        actions = QHBoxLayout()
        actions.addStretch(1)
        self.btn_live = QPushButton(_("Start live"))
        self.btn_live.setObjectName("primary")
        self.btn_live.setToolTip(_("Start real-time face swap from the selected webcam"))
        self.btn_live.clicked.connect(self._on_live)
        actions.addWidget(self.btn_live)
        grid.addLayout(actions, 2, 0, 1, 2)
        return page

    def _build_advanced_settings(self) -> tuple[QToolButton, QWidget]:
        toggle = QToolButton()
        toggle.setObjectName("advancedToggle")
        toggle.setText(_("Advanced settings  ·  Output, face selection, and quality"))
        toggle.setCheckable(True)
        toggle.setChecked(False)
        toggle.setArrowType(Qt.ArrowType.RightArrow)
        panel = QWidget()
        panel.setVisible(False)
        content = QGridLayout(panel)
        content.setContentsMargins(0, 0, 0, 0)
        content.setHorizontalSpacing(12)
        content.setVerticalSpacing(10)

        def make(field, label, tip):
            switch = _Switch(_(label), getattr(modules.globals, field), _(tip))
            switch.toggled.connect(
                lambda value, name=field: (
                    setattr(modules.globals, name, value),
                    save_switch_states(),
                )
            )
            return switch

        video_box = QGroupBox(_("Video output"))
        video_grid = QGridLayout(video_box)
        self.sw_keep_fps = make("keep_fps", "Preserve frame rate", "Keep the original video frame rate")
        self.sw_keep_audio = make("keep_audio", "Preserve original audio", "Copy the source video's audio")
        self.sw_keep_frames = make("keep_frames", "Keep temporary frames", "Keep extracted frames after processing")
        video_grid.addWidget(self.sw_keep_fps, 0, 0)
        video_grid.addWidget(self.sw_keep_audio, 0, 1)
        video_grid.addWidget(self.sw_keep_frames, 1, 0)

        face_box = QGroupBox(_("Face selection"))
        face_grid = QGridLayout(face_box)
        face_grid.addWidget(QLabel(_("Faces to process")), 0, 0)
        self.cb_face_mode = QComboBox()
        self.cb_face_mode.addItem(_("One face"), "one")
        self.cb_face_mode.addItem(_("All detected faces"), "all")
        self.cb_face_mode.addItem(_("Map faces individually"), "map")
        if modules.globals.map_faces:
            self.cb_face_mode.setCurrentIndex(2)
        elif modules.globals.many_faces:
            self.cb_face_mode.setCurrentIndex(1)
        self.cb_face_mode.currentIndexChanged.connect(self._on_face_mode_changed)
        face_grid.addWidget(self.cb_face_mode, 0, 1)

        quality_box = QGroupBox(_("Face quality"))
        quality_grid = QGridLayout(quality_box)
        quality_grid.addWidget(QLabel(_("Face enhancer")), 0, 0)
        self.cb_enhancer = QComboBox()
        self.cb_enhancer.addItems(["None", "GFPGAN", "GPEN-512", "GPEN-256"])
        initial = "None"
        if modules.globals.fp_ui.get("face_enhancer", False):
            initial = "GFPGAN"
        elif modules.globals.fp_ui.get("face_enhancer_gpen512", False):
            initial = "GPEN-512"
        elif modules.globals.fp_ui.get("face_enhancer_gpen256", False):
            initial = "GPEN-256"
        self.cb_enhancer.setCurrentText(initial)
        self.cb_enhancer.currentTextChanged.connect(self._on_enhancer_change)
        quality_grid.addWidget(self.cb_enhancer, 0, 1)
        self.sw_poisson = make("poisson_blend", "Seamless face blending", "Blend face edges smoothly")
        quality_grid.addWidget(self.sw_poisson, 1, 0, 1, 2)
        self.s_transparency = self._add_value_slider(
            quality_grid, 2, "Replacement strength", 0, 100,
            modules.globals.opacity * 100, 1, self._on_transparency_change, "%"
        )
        self.s_sharpness = self._add_value_slider(
            quality_grid, 3, "Sharpness", 0, 50,
            modules.globals.sharpness * 10, 10, self._on_sharpness_change, ""
        )

        details_box = QGroupBox(_("Face details"))
        details_grid = QGridLayout(details_box)
        self.s_mouth = self._add_value_slider(
            details_grid, 0, "Preserve original mouth", 0, 100,
            modules.globals.mouth_mask_size, 1, self._on_mouth_mask_change, "%"
        )

        live_box = QGroupBox(_("Live camera"))
        live_grid = QGridLayout(live_box)
        self.sw_show_fps = make("show_fps", "Show FPS", "Display the live processing frame rate")
        self.sw_color_fix = make("color_correction", "Correct blue tint", "Correct blue or green webcam color")
        self.sw_live_mirror = make("live_mirror", "Mirror live preview", "Flip the live preview horizontally")
        live_grid.addWidget(self.sw_show_fps, 0, 0)
        live_grid.addWidget(self.sw_color_fix, 0, 1)
        live_grid.addWidget(self.sw_live_mirror, 1, 0)

        content.addWidget(video_box, 0, 0)
        content.addWidget(face_box, 0, 1)
        content.addWidget(quality_box, 1, 0)
        content.addWidget(details_box, 1, 1)
        content.addWidget(live_box, 2, 0)
        reset = QPushButton(_("Reset to defaults"))
        reset.setObjectName("secondary")
        reset.clicked.connect(self._reset_advanced_settings)
        content.addWidget(reset, 2, 1, alignment=Qt.AlignmentFlag.AlignRight)

        def update_panel(checked: bool) -> None:
            panel.setVisible(checked)
            toggle.setArrowType(Qt.ArrowType.DownArrow if checked else Qt.ArrowType.RightArrow)

        toggle.toggled.connect(update_panel)
        self.advanced_toggle = toggle
        self.advanced_panel = panel
        return toggle, panel

    def _add_value_slider(
        self, layout: QGridLayout, row: int, label: str, minimum: int,
        maximum: int, initial: float, scale: float, callback: Callable[[float], None],
        suffix: str,
    ) -> QSlider:
        layout.addWidget(QLabel(_(label)), row, 0)
        slider = QSlider(Qt.Orientation.Horizontal)
        slider.setRange(minimum, maximum)
        slider.setValue(int(initial))
        value_label = QLabel()
        value_label.setMinimumWidth(48)
        value_label.setAlignment(Qt.AlignmentFlag.AlignRight)
        slider_row = QHBoxLayout()
        slider_row.addWidget(slider, 1)
        slider_row.addWidget(value_label)
        holder = QWidget()
        holder.setLayout(slider_row)
        layout.addWidget(holder, row, 1)

        def update(value: int) -> None:
            rendered = value / scale
            value_label.setText(f"{rendered:.0f}{suffix}" if suffix == "%" else f"{rendered:.1f}")
            callback(rendered)

        value_label.setText(
            f"{slider.value() / scale:.0f}{suffix}"
            if suffix == "%"
            else f"{slider.value() / scale:.1f}"
        )
        slider.valueChanged.connect(update)
        return slider

    def set_status(self, text: str) -> None:
        self._status_label.setText(text)

    def _set_context_status(self) -> None:
        if self._mode_stack.currentIndex() == 0:
            if not modules.globals.source_path:
                self.set_status(_("Choose a face to begin."))
            elif not modules.globals.target_path:
                self.set_status(_("Now choose an image or video."))
            else:
                self.set_status(_("Ready to process."))
        elif not modules.globals.source_path:
            self.set_status(_("Choose a face before starting live mode."))
        elif not self._camera_names or self._camera_names[0] == "No cameras found":
            self.set_status(_("Camera could not be opened. Choose another input."))
        elif _WEBCAM_PREVIEW is not None and _WEBCAM_PREVIEW.isVisible():
            self.set_status(_("● Live  ·  Face processing is running."))
        else:
            self.set_status(_("Ready to start live processing."))

    def _update_action_states(self) -> None:
        has_source = bool(modules.globals.source_path and is_image(modules.globals.source_path))
        has_target = bool(
            modules.globals.target_path
            and (is_image(modules.globals.target_path) or is_video(modules.globals.target_path))
        )
        self.btn_start.setEnabled(has_source and has_target)
        self.btn_preview.setEnabled(has_source and has_target)
        self.btn_swap.setEnabled(has_source and bool(modules.globals.target_path) and is_image(modules.globals.target_path))
        camera_ok = bool(self._camera_names and self._camera_names[0] != "No cameras found")
        self.btn_live.setEnabled(camera_ok and has_source)

    def _refresh_source_labels(self) -> None:
        path = modules.globals.source_path
        for label in self._source_labels:
            if path and is_image(path):
                label.setPixmap(render_image_preview(path, (300, 250)))
                label.setText("")
            else:
                label.clear()
                label.setText(label._placeholder_text)

    def _on_select_source(self) -> None:
        global _RECENT_SOURCE_DIR
        if _PREVIEW is not None:
            _PREVIEW.hide()
        path, _filter = QFileDialog.getOpenFileName(
            self, _("Choose a face image"), _RECENT_SOURCE_DIR or "", _IMAGE_FILE_FILTER
        )
        if path and is_image(path):
            modules.globals.source_path = path
            _RECENT_SOURCE_DIR = os.path.dirname(path)
            self._refresh_source_labels()
        elif path:
            modules.globals.source_path = None
            self._refresh_source_labels()
        self._update_action_states()
        self._set_context_status()

    def _on_select_target(self) -> None:
        global _RECENT_TARGET_DIR
        if _PREVIEW is not None:
            _PREVIEW.hide()
        path, _filter = QFileDialog.getOpenFileName(
            self, _("Choose an image or video"), _RECENT_TARGET_DIR or "", _MEDIA_FILE_FILTER
        )
        if not path:
            return
        if is_image(path):
            modules.globals.target_path = path
            _RECENT_TARGET_DIR = os.path.dirname(path)
            self.target_label.setPixmap(render_image_preview(path, (300, 250)))
            self.target_label.setText("")
        elif is_video(path):
            modules.globals.target_path = path
            _RECENT_TARGET_DIR = os.path.dirname(path)
            preview = render_video_preview(path, (300, 250))
            if preview:
                self.target_label.setPixmap(preview)
                self.target_label.setText("")
        else:
            modules.globals.target_path = None
            self.target_label.clear()
            self.target_label.setText(self.target_label._placeholder_text)
        self._update_action_states()
        self._set_context_status()

    def _on_random_face(self) -> None:
        if _PREVIEW is not None:
            _PREVIEW.hide()
        try:
            response = requests.get(
                "https://thispersondoesnotexist.com/",
                headers={"User-Agent": "Mozilla/5.0"},
                timeout=10,
            )
            response.raise_for_status()
            temp_path = os.path.join(tempfile.gettempdir(), "deep_live_cam_random_face.jpg")
            with open(temp_path, "wb") as f:
                f.write(response.content)
            modules.globals.source_path = temp_path
            self._refresh_source_labels()
            self._update_action_states()
            self._set_context_status()
        except Exception as exc:
            print(f"Failed to fetch random face: {exc}")

    def _on_swap_paths(self) -> None:
        global _RECENT_SOURCE_DIR, _RECENT_TARGET_DIR
        source = modules.globals.source_path
        target = modules.globals.target_path
        if not (source and target and is_image(source) and is_image(target)):
            return
        modules.globals.source_path, modules.globals.target_path = target, source
        _RECENT_SOURCE_DIR = os.path.dirname(target)
        _RECENT_TARGET_DIR = os.path.dirname(source)
        if _PREVIEW is not None:
            _PREVIEW.hide()
        self._refresh_source_labels()
        self.target_label.setPixmap(render_image_preview(modules.globals.target_path, (300, 250)))
        self.target_label.setText("")
        self._update_action_states()
        self._set_context_status()

    def _on_face_mode_changed(self, index: int) -> None:
        mode = self.cb_face_mode.itemData(index)
        modules.globals.many_faces = mode == "all"
        modules.globals.map_faces = mode == "map"
        if mode != "map":
            close_mapper_window()
        save_switch_states()

    def _on_enhancer_change(self, choice: str) -> None:
        key_map = {
            "None": None,
            "GFPGAN": "face_enhancer",
            "GPEN-256": "face_enhancer_gpen256",
            "GPEN-512": "face_enhancer_gpen512",
        }
        for key in ("face_enhancer", "face_enhancer_gpen256", "face_enhancer_gpen512"):
            _update_tumbler(key, False)
        selected = key_map.get(choice)
        if selected:
            _update_tumbler(selected, True)
        save_switch_states()

    def _on_transparency_change(self, value: float) -> None:
        modules.globals.opacity = value / 100

    def _on_sharpness_change(self, value: float) -> None:
        modules.globals.sharpness = value / 10

    def _on_mouth_mask_change(self, value: float) -> None:
        modules.globals.mouth_mask_size = value
        modules.globals.mouth_mask = value > 0
        if value <= 0:
            modules.globals.show_mouth_mask_box = False

    def _reset_advanced_settings(self) -> None:
        self.sw_keep_fps.setChecked(True)
        self.sw_keep_audio.setChecked(True)
        self.sw_keep_frames.setChecked(False)
        self.sw_poisson.setChecked(False)
        self.sw_color_fix.setChecked(False)
        self.sw_show_fps.setChecked(False)
        self.sw_live_mirror.setChecked(False)
        self.cb_face_mode.setCurrentIndex(0)
        self.cb_enhancer.setCurrentText("None")
        self.s_transparency.setValue(100)
        self.s_sharpness.setValue(0)
        self.s_mouth.setValue(0)
        save_switch_states()
        self.set_status(_("Advanced settings reset."))

    def _on_start(self) -> None:
        if not modules.globals.source_path:
            self.set_status(_("Choose a face to begin."))
            return
        if not modules.globals.target_path:
            self.set_status(_("Now choose an image or video."))
            return
        if modules.globals.map_faces:
            modules.globals.source_target_map = []
            if is_image(modules.globals.target_path):
                update_status(_("Getting unique faces"))
                get_unique_faces_from_target_image()
            elif is_video(modules.globals.target_path):
                update_status(_("Getting unique faces"))
                get_unique_faces_from_target_video()
            if modules.globals.source_target_map:
                _open_mapper_dialog(self._start_cb, modules.globals.source_target_map)
            else:
                update_status(_("No faces found in target"))
        else:
            self._select_output_and_start()

    def _select_output_and_start(self) -> None:
        global _RECENT_OUTPUT_DIR
        if is_image(modules.globals.target_path):
            path, _filter = QFileDialog.getSaveFileName(
                self, _("Save image output"), os.path.join(_RECENT_OUTPUT_DIR or "", "output.png"), _IMAGE_FILE_FILTER
            )
        elif is_video(modules.globals.target_path):
            path, _filter = QFileDialog.getSaveFileName(
                self, _("Save video output"), os.path.join(_RECENT_OUTPUT_DIR or "", "output.mp4"), _VIDEO_FILE_FILTER
            )
        else:
            return
        if path:
            modules.globals.output_path = path
            _RECENT_OUTPUT_DIR = os.path.dirname(path)
            self.set_status(_("Processing file…"))
            self._start_cb()

    def _on_toggle_preview(self) -> None:
        if _PREVIEW is None:
            return
        if _PREVIEW.isVisible():
            _PREVIEW.hide()
        elif modules.globals.source_path and modules.globals.target_path:
            _PREVIEW.init_for_target()
            _PREVIEW.refresh_frame(0)
            _PREVIEW.show()

    def _on_virtual_camera_toggled(self, enabled: bool) -> None:
        modules.globals.virtual_camera_enabled = enabled
        save_switch_states()
        if _WEBCAM_PREVIEW is not None:
            _WEBCAM_PREVIEW.set_virtual_camera_enabled(enabled)
        self.set_status(_("Virtual camera output enabled.") if enabled else _("Virtual camera output disabled."))

    def _on_live(self) -> None:
        idx = self.cb_camera.currentIndex()
        if not modules.globals.source_path:
            self.set_status(_("Choose a face before starting live mode."))
            return
        if idx < 0 or idx >= len(self._camera_indices):
            self.set_status(_("Camera could not be opened. Choose another input."))
            return
        if _LIVE_MAPPER is not None and _LIVE_MAPPER.isVisible():
            _LIVE_MAPPER.raise_()
            return
        if not modules.globals.map_faces:
            from modules.face_analyser import get_face_analyser
            from modules.processors.frame.face_swapper import get_face_swapper
            get_face_analyser()
            get_face_swapper()
            _open_webcam_preview(self._camera_indices[idx])
        else:
            modules.globals.source_target_map = []
            _open_live_mapper_dialog(self._camera_indices[idx], modules.globals.source_target_map)
        self._set_context_status()

    def closeEvent(self, event):
        self._destroy_cb()
        event.accept()


def _update_tumbler(var: str, value: bool) -> None:
    modules.globals.fp_ui[var] = value
    save_switch_states()
    # If we're currently in a live preview, refresh frame processors so
    # toggling enhancers takes effect immediately.
    if _WEBCAM_PREVIEW is not None and _WEBCAM_PREVIEW.isVisible():
        get_frame_processors_modules(modules.globals.frame_processors)


# ─── preview window (still-image / video scrub) ──────────────────────────


class PreviewWindow(QWidget):
    def __init__(self):
        super().__init__()
        self.setWindowTitle(_("Preview"))
        self.resize(PREVIEW_DEFAULT_WIDTH, PREVIEW_DEFAULT_HEIGHT)
        layout = QVBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)

        self._image_label = QLabel()
        self._image_label.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self._image_label.setSizePolicy(QSizePolicy.Policy.Expanding, QSizePolicy.Policy.Expanding)
        layout.addWidget(self._image_label, 1)

        self._slider = QSlider(Qt.Orientation.Horizontal)
        self._slider.setRange(0, 0)
        self._slider.valueChanged.connect(self.refresh_frame)
        layout.addWidget(self._slider)

    def init_for_target(self) -> None:
        if is_image(modules.globals.target_path):
            self._slider.hide()
        elif is_video(modules.globals.target_path):
            total = get_video_frame_total(modules.globals.target_path)
            self._slider.setRange(0, max(0, total - 1))
            self._slider.setValue(0)
            self._slider.show()

    def refresh_frame(self, frame_number: int = 0) -> None:
        if not (modules.globals.source_path and modules.globals.target_path):
            return
        update_status("Processing...")
        temp_frame = get_video_frame(modules.globals.target_path, frame_number)
        if modules.globals.nsfw_filter and check_and_ignore_nsfw(temp_frame):
            return
        from modules.processors.frame.core import get_frame_processors_modules as _gfpm
        for fp in _gfpm(modules.globals.frame_processors):
            temp_frame = fp.process_frame(
                get_one_face(imread_unicode(modules.globals.source_path)), temp_frame
            )
        # Fit to current widget size while preserving aspect ratio.
        h, w = temp_frame.shape[:2]
        bound_w = min(PREVIEW_MAX_WIDTH, max(self.width(), PREVIEW_DEFAULT_WIDTH))
        bound_h = min(PREVIEW_MAX_HEIGHT, max(self.height(), PREVIEW_DEFAULT_HEIGHT))
        ratio = min(bound_w / w, bound_h / h)
        new_size = (max(1, int(w * ratio)), max(1, int(h * ratio)))
        temp_frame = cv2.resize(temp_frame, new_size, interpolation=cv2.INTER_LANCZOS4)
        self._image_label.setPixmap(_bgr_to_qpixmap(temp_frame))
        update_status("Processing succeed!")


# ─── webcam preview window ───────────────────────────────────────────────


class _CaptureWorker(QThread):
    """Reads frames from the camera into a bounded queue. Drops on overflow."""

    def __init__(self, cap, capture_queue: queue.Queue, stop_event: threading.Event):
        super().__init__()
        self._cap = cap
        self._queue = capture_queue
        self._stop = stop_event

    def run(self) -> None:
        while not self._stop.is_set():
            ret, frame = self._cap.read()
            if not ret:
                self._stop.set()
                break
            try:
                self._queue.put_nowait(frame)
            except queue.Full:
                try:
                    self._queue.get_nowait()
                except queue.Empty:
                    pass
                try:
                    self._queue.put_nowait(frame)
                except queue.Full:
                    pass


class _ProcessingWorker(QThread):
    """Pulls raw frames, runs detect/swap/enhance, pushes processed frames."""

    def __init__(self, capture_queue, processed_queue, stop_event, camera_fps: float):
        super().__init__()
        self._cq = capture_queue
        self._pq = processed_queue
        self._stop = stop_event
        self._fps = camera_fps

    def run(self) -> None:
        frame_processors = get_frame_processors_modules(modules.globals.frame_processors)
        source_image = None
        last_source_path = None
        prev_time = time.time()
        fps_update_interval = 0.5
        frame_count = 0
        fps = 0.0
        det_count = 0
        cached_target_face = None
        cached_many_faces = None
        det_interval = max(1, round(self._fps * 0.08))

        while not self._stop.is_set():
            try:
                frame = self._cq.get(timeout=0.05)
            except queue.Empty:
                continue

            temp_frame = frame
            if modules.globals.live_mirror:
                temp_frame = gpu_flip(temp_frame, 1)

            if not modules.globals.map_faces:
                if (
                    modules.globals.source_path
                    and modules.globals.source_path != last_source_path
                ):
                    last_source_path = modules.globals.source_path
                    source_image = get_one_face(imread_unicode(modules.globals.source_path))

                det_count += 1
                if det_count % det_interval == 0:
                    if modules.globals.many_faces:
                        cached_target_face = None
                        cached_many_faces = detect_many_faces_fast(temp_frame)
                    else:
                        cached_target_face = detect_one_face_fast(temp_frame)
                        cached_many_faces = None

                cached_faces = None
                if cached_many_faces:
                    cached_faces = cached_many_faces
                elif cached_target_face is not None:
                    cached_faces = [cached_target_face]

                # Fast detection skips the 2d106 landmark model, but the mouth
                # mask needs it. Attach landmarks on demand (computed once per
                # detection cycle — the helper no-ops if already present).
                if modules.globals.mouth_mask and cached_faces:
                    ensure_landmarks(temp_frame, cached_faces)

                for fp in frame_processors:
                    if fp.NAME == "DLC.FACE-ENHANCER":
                        if modules.globals.fp_ui["face_enhancer"]:
                            temp_frame = fp.process_frame(
                                None, temp_frame, detected_faces=cached_faces
                            )
                    elif fp.NAME == "DLC.FACE-ENHANCER-GPEN256":
                        if modules.globals.fp_ui.get("face_enhancer_gpen256", False):
                            temp_frame = fp.process_frame(
                                None, temp_frame, detected_faces=cached_faces
                            )
                    elif fp.NAME == "DLC.FACE-ENHANCER-GPEN512":
                        if modules.globals.fp_ui.get("face_enhancer_gpen512", False):
                            temp_frame = fp.process_frame(
                                None, temp_frame, detected_faces=cached_faces
                            )
                    elif fp.NAME == "DLC.FACE-SWAPPER":
                        swapped_bboxes = []
                        if modules.globals.many_faces and cached_many_faces:
                            result = temp_frame.copy()
                            for t_face in cached_many_faces:
                                result = fp.swap_face(source_image, t_face, result)
                                if hasattr(t_face, "bbox") and t_face.bbox is not None:
                                    swapped_bboxes.append(t_face.bbox.astype(int))
                            temp_frame = result
                        elif cached_target_face is not None:
                            temp_frame = fp.swap_face(
                                source_image, cached_target_face, temp_frame
                            )
                            if (
                                hasattr(cached_target_face, "bbox")
                                and cached_target_face.bbox is not None
                            ):
                                swapped_bboxes.append(cached_target_face.bbox.astype(int))
                        temp_frame = fp.apply_post_processing(temp_frame, swapped_bboxes)
                    else:
                        temp_frame = fp.process_frame(source_image, temp_frame)
            else:
                modules.globals.target_path = None
                for fp in frame_processors:
                    if fp.NAME == "DLC.FACE-ENHANCER":
                        if modules.globals.fp_ui["face_enhancer"]:
                            temp_frame = fp.process_frame_v2(temp_frame)
                    elif fp.NAME in ("DLC.FACE-ENHANCER-GPEN256", "DLC.FACE-ENHANCER-GPEN512"):
                        fp_key = fp.NAME.split(".")[-1].lower().replace("-", "_")
                        if modules.globals.fp_ui.get(fp_key, False):
                            temp_frame = fp.process_frame_v2(temp_frame)
                    else:
                        temp_frame = fp.process_frame_v2(temp_frame)

            current_time = time.time()
            frame_count += 1
            if current_time - prev_time >= fps_update_interval:
                fps = frame_count / (current_time - prev_time)
                frame_count = 0
                prev_time = current_time

            if modules.globals.show_fps:
                cv2.putText(
                    temp_frame, f"FPS: {fps:.1f}", (10, 30),
                    cv2.FONT_HERSHEY_SIMPLEX, 1, (0, 255, 0), 2,
                )

            try:
                self._pq.put_nowait(temp_frame)
            except queue.Full:
                try:
                    self._pq.get_nowait()
                except queue.Empty:
                    pass
                try:
                    self._pq.put_nowait(temp_frame)
                except queue.Full:
                    pass


class _VirtualCameraWorker(QThread):
    """Publishes processed frames through the UnityCapture device."""

    def __init__(
        self,
        frame_queue: queue.Queue,
        stop_event: threading.Event,
        width: int,
        height: int,
        fps: float,
    ):
        super().__init__()
        self._queue = frame_queue
        self._stop = stop_event
        self._width = width
        self._height = height
        self._fps = max(1, round(fps))

    def run(self) -> None:
        if pyvirtualcam is None:
            update_status("Virtual camera unavailable: install pyvirtualcam.")
            return
        try:
            with pyvirtualcam.Camera(
                width=self._width,
                height=self._height,
                fps=self._fps,
                fmt=pyvirtualcam.PixelFormat.RGB,
                backend="unitycapture",
            ) as camera:
                update_status(f"Virtual camera active: {camera.device}")
                while not self._stop.is_set():
                    try:
                        bgr_frame = self._queue.get(timeout=0.1)
                    except queue.Empty:
                        continue
                    if bgr_frame.shape[1] != self._width or bgr_frame.shape[0] != self._height:
                        bgr_frame = cv2.resize(
                            bgr_frame,
                            (self._width, self._height),
                            interpolation=cv2.INTER_LINEAR,
                        )
                    camera.send(cv2.cvtColor(bgr_frame, cv2.COLOR_BGR2RGB))
                    camera.sleep_until_next_frame()
        except Exception as exc:
            update_status(f"Virtual camera unavailable: {exc}")

class WebcamPreviewWindow(QWidget):
    def __init__(self, camera_index: int):
        super().__init__()
        self.setWindowTitle("Live Preview")
        self.resize(PREVIEW_DEFAULT_WIDTH, PREVIEW_DEFAULT_HEIGHT)
        layout = QVBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)
        self._image_label = QLabel()
        self._image_label.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self._image_label.setSizePolicy(QSizePolicy.Policy.Expanding, QSizePolicy.Policy.Expanding)
        layout.addWidget(self._image_label, 1)

        self._cap = VideoCapturer(camera_index)
        if not self._cap.start(PREVIEW_DEFAULT_WIDTH, PREVIEW_DEFAULT_HEIGHT, 60):
            update_status("Failed to start camera")
            QTimer.singleShot(0, self.close)
            return

        camera_fps = self._cap.actual_fps
        print(
            f"[webcam] Camera running at {self._cap.actual_width}x"
            f"{self._cap.actual_height}@{camera_fps:.0f}fps"
        )

        self._capture_queue: queue.Queue = queue.Queue(maxsize=2)
        self._processed_queue: queue.Queue = queue.Queue(maxsize=2)
        self._stop_event = threading.Event()
        self._virtual_camera_queue: queue.Queue = queue.Queue(maxsize=1)
        self._virtual_camera_stop = threading.Event()
        self._virtual_camera_worker: Optional[_VirtualCameraWorker] = None

        self._capture_worker = _CaptureWorker(
            self._cap, self._capture_queue, self._stop_event
        )
        self._processing_worker = _ProcessingWorker(
            self._capture_queue, self._processed_queue, self._stop_event, camera_fps
        )
        self._capture_worker.start()
        self._processing_worker.start()

        # Start the virtual device only when enabled in the main window.
        if modules.globals.virtual_camera_enabled:
            self._start_virtual_camera(camera_fps)

        # Poll at ~2x camera fps so we never block but also don't burn CPU.
        poll_ms = max(1, min(16, int(500 / max(camera_fps, 1))))
        self._timer = QTimer(self)
        self._timer.timeout.connect(self._tick)
        self._timer.start(poll_ms)

    def _start_virtual_camera(self, camera_fps: float) -> None:
        if self._virtual_camera_worker is not None and self._virtual_camera_worker.isRunning():
            return
        self._virtual_camera_stop.clear()
        self._virtual_camera_worker = _VirtualCameraWorker(
            self._virtual_camera_queue,
            self._virtual_camera_stop,
            self._cap.actual_width,
            self._cap.actual_height,
            camera_fps,
        )
        self._virtual_camera_worker.start()

    def _stop_virtual_camera(self) -> None:
        worker = self._virtual_camera_worker
        if worker is None:
            return
        self._virtual_camera_stop.set()
        try:
            worker.wait(2000)
        except Exception:
            pass
        self._virtual_camera_worker = None

    def set_virtual_camera_enabled(self, enabled: bool) -> None:
        if enabled:
            self._start_virtual_camera(self._cap.actual_fps)
        else:
            self._stop_virtual_camera()

    def _tick(self) -> None:
        if self._stop_event.is_set():
            self.close()
            return
        try:
            bgr_frame = self._processed_queue.get_nowait()
        except queue.Empty:
            return
        if self._virtual_camera_worker is not None and self._virtual_camera_worker.isRunning():
            try:
                self._virtual_camera_queue.put_nowait(bgr_frame)
            except queue.Full:
                try:
                    self._virtual_camera_queue.get_nowait()
                except queue.Empty:
                    pass
                try:
                    self._virtual_camera_queue.put_nowait(bgr_frame)
                except queue.Full:
                    pass
        display_frame = fit_image_to_size(bgr_frame, self.width(), self.height())
        self._image_label.setPixmap(_bgr_to_qpixmap(display_frame))

    def closeEvent(self, event) -> None:
        self._stop_event.set()
        self._stop_virtual_camera()
        try:
            self._timer.stop()
        except Exception:
            pass
        for worker in (self._capture_worker, self._processing_worker):
            try:
                worker.wait(2000)
            except Exception:
                pass
        try:
            self._cap.release()
        except Exception:
            pass
        global _WEBCAM_PREVIEW
        if _WEBCAM_PREVIEW is self:
            _WEBCAM_PREVIEW = None
        event.accept()



def _open_webcam_preview(camera_index: int) -> None:
    global _WEBCAM_PREVIEW
    if _WEBCAM_PREVIEW is not None:
        _WEBCAM_PREVIEW.close()
    _WEBCAM_PREVIEW = WebcamPreviewWindow(camera_index)
    _WEBCAM_PREVIEW.show()


# ─── mapper dialogs (image/video + live) ────────────────────────────────


def _make_thumb(cv2_img: np.ndarray) -> QPixmap:
    rgb = gpu_cvt_color(cv2_img, cv2.COLOR_BGR2RGB)
    image = Image.fromarray(rgb).resize(
        (MAPPER_PREVIEW_SIZE, MAPPER_PREVIEW_SIZE), Image.LANCZOS
    )
    return _pil_to_qpixmap(image)


class MapperDialog(QDialog):
    """Source × Target mapper for image / video processing."""

    def __init__(self, start_cb: Callable, mapping: list):
        super().__init__(_MAIN)
        self._start_cb = start_cb
        self._map = mapping
        self.setWindowTitle(_("Source x Target Mapper"))
        self.resize(POPUP_WIDTH, POPUP_HEIGHT)
        layout = QVBoxLayout(self)

        self._scroll = QScrollArea()
        self._scroll.setWidgetResizable(True)
        layout.addWidget(self._scroll, 1)

        self._status = QLabel("")
        self._status.setAlignment(Qt.AlignmentFlag.AlignCenter)
        layout.addWidget(self._status)

        btn_submit = QPushButton(_("Submit"))
        btn_submit.clicked.connect(self._on_submit)
        layout.addWidget(btn_submit, alignment=Qt.AlignmentFlag.AlignCenter)

        self._rebuild()

    def set_status(self, text: str) -> None:
        self._status.setText(_(text))

    def _rebuild(self) -> None:
        body = QWidget()
        grid = QGridLayout(body)
        grid.setHorizontalSpacing(10)
        grid.setVerticalSpacing(10)
        for item in self._map:
            row = item["id"]
            btn = QPushButton(_("Select source image"))
            btn.setFixedWidth(200)
            btn.clicked.connect(lambda _c, n=row: self._select_source(n))
            grid.addWidget(btn, row, 0)

            src_label = QLabel(f"S-{row}")
            src_label.setFixedSize(MAPPER_PREVIEW_SIZE, MAPPER_PREVIEW_SIZE)
            src_label.setAlignment(Qt.AlignmentFlag.AlignCenter)
            src_label.setStyleSheet("border: 1px dashed #555;")
            grid.addWidget(src_label, row, 1)
            if "source" in item:
                src_label.setPixmap(_make_thumb(item["source"]["cv2"]))
                src_label.setText("")

            x_label = QLabel("×")
            x_label.setAlignment(Qt.AlignmentFlag.AlignCenter)
            grid.addWidget(x_label, row, 2)

            tgt_label = QLabel(f"T-{row}")
            tgt_label.setFixedSize(MAPPER_PREVIEW_SIZE, MAPPER_PREVIEW_SIZE)
            tgt_label.setAlignment(Qt.AlignmentFlag.AlignCenter)
            tgt_label.setStyleSheet("border: 1px solid #555;")
            grid.addWidget(tgt_label, row, 3)
            if "target" in item:
                tgt_label.setPixmap(_make_thumb(item["target"]["cv2"]))
                tgt_label.setText("")

        grid.setRowStretch(grid.rowCount(), 1)
        self._scroll.setWidget(body)

    def _select_source(self, row: int) -> None:
        path, _f = QFileDialog.getOpenFileName(
            self, _("select an source image"),
            _RECENT_SOURCE_DIR or "",
            _IMAGE_FILE_FILTER,
        )
        if not path:
            return
        cv2_img = imread_unicode(path)
        face = get_one_face(cv2_img)
        if face is None:
            self.set_status("Face could not be detected in last upload!")
            return
        x_min, y_min, x_max, y_max = face["bbox"]
        self._map[row]["source"] = {
            "cv2": cv2_img[int(y_min):int(y_max), int(x_min):int(x_max)],
            "face": face,
        }
        self._rebuild()

    def _on_submit(self) -> None:
        if has_valid_map():
            self.accept()
            _MAIN._select_output_and_start()
        else:
            self.set_status("Atleast 1 source with target is required!")


class LiveMapperDialog(QDialog):
    """Source × Target mapper for live webcam mode."""

    def __init__(self, camera_index: int, mapping: list):
        super().__init__(_MAIN)
        self._camera_index = camera_index
        self._map = mapping
        self.setWindowTitle(_("Source x Target Mapper"))
        self.resize(POPUP_LIVE_WIDTH, POPUP_LIVE_HEIGHT)
        layout = QVBoxLayout(self)

        self._scroll = QScrollArea()
        self._scroll.setWidgetResizable(True)
        layout.addWidget(self._scroll, 1)

        self._status = QLabel("")
        self._status.setAlignment(Qt.AlignmentFlag.AlignCenter)
        layout.addWidget(self._status)

        btn_row = QHBoxLayout()
        for text, slot in (
            (_("Add"), self._on_add),
            (_("Clear"), self._on_clear),
            (_("Submit"), self._on_submit),
        ):
            b = QPushButton(text)
            b.clicked.connect(slot)
            btn_row.addWidget(b)
        layout.addLayout(btn_row)

        self._rebuild()

    def set_status(self, text: str) -> None:
        self._status.setText(_(text))

    def _rebuild(self) -> None:
        body = QWidget()
        grid = QGridLayout(body)
        grid.setHorizontalSpacing(10)
        grid.setVerticalSpacing(10)
        for item in self._map:
            row = item["id"]
            btn_s = QPushButton(_("Select source image"))
            btn_s.setFixedWidth(200)
            btn_s.clicked.connect(lambda _c, n=row: self._select_face(n, "source"))
            grid.addWidget(btn_s, row, 0)

            src_label = QLabel(f"S-{row}")
            src_label.setFixedSize(MAPPER_PREVIEW_SIZE, MAPPER_PREVIEW_SIZE)
            src_label.setAlignment(Qt.AlignmentFlag.AlignCenter)
            src_label.setStyleSheet("border: 1px dashed #555;")
            grid.addWidget(src_label, row, 1)
            if "source" in item:
                src_label.setPixmap(_make_thumb(item["source"]["cv2"]))
                src_label.setText("")

            x_label = QLabel("×")
            x_label.setAlignment(Qt.AlignmentFlag.AlignCenter)
            grid.addWidget(x_label, row, 2)

            btn_t = QPushButton(_("Select target image"))
            btn_t.setFixedWidth(200)
            btn_t.clicked.connect(lambda _c, n=row: self._select_face(n, "target"))
            grid.addWidget(btn_t, row, 3)

            tgt_label = QLabel(f"T-{row}")
            tgt_label.setFixedSize(MAPPER_PREVIEW_SIZE, MAPPER_PREVIEW_SIZE)
            tgt_label.setAlignment(Qt.AlignmentFlag.AlignCenter)
            tgt_label.setStyleSheet("border: 1px dashed #555;")
            grid.addWidget(tgt_label, row, 4)
            if "target" in item:
                tgt_label.setPixmap(_make_thumb(item["target"]["cv2"]))
                tgt_label.setText("")

        grid.setRowStretch(grid.rowCount(), 1)
        self._scroll.setWidget(body)

    def _select_face(self, row: int, kind: str) -> None:
        path, _f = QFileDialog.getOpenFileName(
            self, _("select an source image"),
            _RECENT_SOURCE_DIR or "",
            _IMAGE_FILE_FILTER,
        )
        if not path:
            return
        cv2_img = imread_unicode(path)
        face = get_one_face(cv2_img)
        if face is None:
            self.set_status("Face could not be detected in last upload!")
            return
        x_min, y_min, x_max, y_max = face["bbox"]
        self._map[row][kind] = {
            "cv2": cv2_img[int(y_min):int(y_max), int(x_min):int(x_max)],
            "face": face,
        }
        self._rebuild()

    def _on_add(self) -> None:
        add_blank_map()
        self._rebuild()
        self.set_status("Please provide mapping!")

    def _on_clear(self) -> None:
        for item in self._map:
            item.pop("source", None)
            item.pop("target", None)
        self._rebuild()
        self.set_status("All mappings cleared!")

    def _on_submit(self) -> None:
        if has_valid_map():
            simplify_maps()
            self.set_status("Mappings successfully submitted!")
            self.accept()
            _open_webcam_preview(self._camera_index)
        else:
            self.set_status("At least 1 source with target is required!")


def _open_mapper_dialog(start_cb: Callable, mapping: list) -> None:
    global _MAPPER
    close_mapper_window()
    _MAPPER = MapperDialog(start_cb, mapping)
    _MAPPER.show()


def _open_live_mapper_dialog(camera_index: int, mapping: list) -> None:
    global _LIVE_MAPPER
    close_mapper_window()
    _LIVE_MAPPER = LiveMapperDialog(camera_index, mapping)
    _LIVE_MAPPER.show()


def close_mapper_window() -> None:
    global _MAPPER, _LIVE_MAPPER
    if _MAPPER is not None:
        _MAPPER.close()
        _MAPPER = None
    if _LIVE_MAPPER is not None:
        _LIVE_MAPPER.close()
        _LIVE_MAPPER = None


# ─── entry point ─────────────────────────────────────────────────────────


class _Window:
    """Thin wrapper exposing .mainloop() for core.py compatibility."""

    def __init__(self, app: QApplication, main_window: MainWindow):
        self._app = app
        self._main = main_window

    def mainloop(self) -> None:
        self._main.show()
        self._app.exec()


def init(
    start: Callable[[], None], destroy: Callable[[], None], lang: str
) -> _Window:
    global _APP, _MAIN, _PREVIEW, _LANG, _BRIDGE

    _LANG = LanguageManager(lang)
    if QApplication.instance() is None:
        _APP = QApplication(sys.argv)
    else:
        _APP = QApplication.instance()
    _APP.setStyleSheet(QSS)

    _BRIDGE = _UIBridge()
    _MAIN = MainWindow(start, destroy)
    _PREVIEW = PreviewWindow()

    # Route status updates onto the UI thread regardless of caller.
    _BRIDGE.statusChanged.connect(_MAIN.set_status)

    return _Window(_APP, _MAIN)
