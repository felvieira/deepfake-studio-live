<p align="center">
  <img src="media/brand/icon-128.png" alt="" width="96" height="96">
</p>

<h1 align="center">Deep-Live-Cam — Virtual Camera Edition</h1>

<p align="center">
  Real-time face swap that publishes straight to a virtual camera — no OBS required.
</p>

<p align="center">
  <b>English</b> · <a href="README.pt-BR.md">Português (Brasil)</a>
</p>

<p align="center">
  <img src="media/brand/hero.webp" alt="Deepfake Studio Live" width="860">
</p>

---

## About this fork

This is an independent fork of [hacksider/Deep-Live-Cam](https://github.com/hacksider/Deep-Live-Cam).
All the face-swapping work — the pipeline, the models, the performance tuning — comes from
that project and the people credited below. This fork does not claim any of it.

What this fork adds:

| Change | Why it matters |
|---|---|
| **Built-in virtual camera** | Upstream tells you to screen-capture the preview window with OBS. Here the processed frames go straight to a virtual camera device that Zoom, Discord, Teams and Meet see as a normal webcam. |
| **Rebuilt interface** | File and Live are now two clear modes instead of one crowded panel. Advanced options are collapsed by default, so the common path stays short. |
| **Installers for Windows and macOS** | A small installer that downloads everything — its own Python, the dependencies, ffmpeg, the AI models and the virtual camera — with real progress and resume. No terminal, no `pip`. |
| **Bundled UnityCapture driver** | The driver ships in `third_party/`, so there is nothing extra to hunt down. |

Everything else behaves as upstream. If you want the original project, go there — it is the
reference implementation and it is actively maintained.

---

## Disclaimer

This is deepfake software. Read this part.

**Consent is not optional.** If you use a real person's face, get their permission first.
This is not a formality — in many places using someone's likeness without consent is illegal,
and "it was just a joke" is not a defence.

**Label your output.** When you share something made with this tool, say so. People are
entitled to know whether what they are watching is real.

**The NSFW filter stays.** The project ships a check that blocks inappropriate material.
Do not remove it. If you remove it, you are on your own — legally and otherwise.

**Non-commercial only.** The face-analysis models (InsightFace `buffalo_l`) are licensed for
[non-commercial research use only](https://github.com/deepinsight/insightface?tab=readme-ov-file#license).
That restriction comes from the model authors, not from this project's licence, and it applies
no matter what you do with the code.

You are responsible for what you make with this. Not the authors of this fork, and not upstream.

---

## Installation

**Windows 10/11 (64-bit)**

Download [DeepfakeStudioLive-Setup.exe](https://github.com/felvieira/deepfake-studio-live/releases/latest)
and run it. When it finishes, open the app from the **Deepfake Studio Live** shortcut on the
Desktop or Start menu.

**macOS 13 or later, Apple Silicon (M1 and newer)**

1. Download [DeepfakeStudioLive-macOS.dmg](https://github.com/felvieira/deepfake-studio-live/releases/latest),
   open it and drag the installer to **Applications**.
2. Open it. The app is not signed by an Apple developer account yet, so the first time macOS
   blocks it: go to **System Settings → Privacy & Security**, scroll down and click
   **Open Anyway**. (Or, in Terminal: `xattr -dr com.apple.quarantine "/Applications/Deepfake Studio Live Installer.app"`.)
3. If the Xcode Command Line Tools are missing, macOS shows its own install window during
   the setup — confirm it and the installer continues on its own.
4. When it finishes, open **Deepfake Studio Live** from Applications, Launchpad or Spotlight.
   macOS asks for camera access the first time you start live mode.

**What the installer does (both systems)**

It downloads its own Python, the dependencies, ffmpeg, the ~1.3 GB of AI models and sets up
the virtual camera. Nothing is installed system-wide except the virtual camera driver on
Windows (the only step that asks for Administrator permission). It installs to
`%LOCALAPPDATA%\DeepLiveCam` on Windows and `~/Library/Application Support/DeepLiveCam` on
macOS, needs about 10 GB free and an internet connection during setup.

Every step resumes, so if a download breaks halfway, running it again picks up where it
stopped. When it finishes it tells you whether hardware acceleration is active.

**From source**

Windows (needs Python 3.11–3.13 and the Visual C++ Build Tools):

```bash
install.bat
venv\Scripts\python.exe run.py
```

macOS / Linux (needs Python 3.11–3.13; on the Mac, also the Xcode Command Line Tools):

```bash
./install.sh
./run.sh
```

ffmpeg comes with the dependencies (`imageio-ffmpeg`); a system ffmpeg on the PATH is used
instead when present. The models download on first launch.

---

## Using the virtual camera

1. Open the app and pick **Live camera** mode
2. Choose a source face image
3. Turn on **Send video to a virtual camera**
4. Click **Start live**
5. In Zoom, Discord, Teams or Meet, select the camera:
   - Windows: **Unity Video Capture** (installed by the installer)
   - macOS: **OBS Virtual Camera** — install [OBS Studio](https://obsproject.com) (free) and
     start its virtual camera once inside OBS; macOS asks you to approve it in System Settings.

Restart the calling app if the camera does not appear — most of them enumerate devices once
at startup.

To remove the Windows driver later: `uninstall_virtual_camera.bat`.

---

## GPU acceleration

- **Windows + NVIDIA:** the installer adds the CUDA libraries automatically.
- **Mac with Apple Silicon:** CoreML (GPU and Neural Engine) is used automatically; video
  encoding uses VideoToolbox.
- DirectML and OpenVINO work as upstream documents them — see the
  [upstream GPU guide](https://github.com/hacksider/Deep-Live-Cam#gpu-acceleration).

The badge in the top-right corner shows the GPU that was found and whether it is really in
use. If it says the GPU was found but is **not** in use, the accelerator failed to load and
the app is running on the CPU — it works, just much slower. **Free GPU memory** (under the
badge) unloads the models when you are done; stopping live mode does that automatically.

---

## Licence

**AGPL-3.0**, inherited from upstream. In short: you may use, modify and redistribute this,
but anything you distribute must also be AGPL and must come with source. If you run a
modified version as a network service, you owe your users the source too.

The full text is in [LICENSE](LICENSE). The InsightFace non-commercial restriction noted in
the disclaimer applies on top of it.

---

## Credits

This fork exists because of work done elsewhere. The credit belongs to:

- [hacksider](https://github.com/hacksider) and the [Deep-Live-Cam contributors](https://github.com/hacksider/Deep-Live-Cam/graphs/contributors) — the project this forks
- [s0md3v](https://github.com/s0md3v/roop) — the original roop codebase this all descends from
- [deepinsight](https://github.com/deepinsight) — [InsightFace](https://github.com/deepinsight/insightface), the face analysis models
- [schellingb](https://github.com/schellingb/UnityCapture) — UnityCapture, the virtual camera driver
- [ffmpeg](https://ffmpeg.org/) — video operations
- [Henry](https://github.com/henryruhs) — major upstream contributor

Named in the upstream README for their contributions there:
[havok2-htwo](https://github.com/havok2-htwo),
[GosuDRM](https://github.com/GosuDRM),
[pereiraroland26](https://github.com/pereiraroland26),
[vic4key](https://github.com/vic4key),
[kier007](https://github.com/kier007),
[qitianai](https://github.com/qitianai),
[laurigates](https://github.com/laurigates),
[maxwbuckley](https://github.com/maxwbuckley).
