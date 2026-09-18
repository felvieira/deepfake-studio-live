<h1 align="center">Deep-Live-Cam — Virtual Camera Edition</h1>

<p align="center">
  Real-time face swap that publishes straight to a virtual camera — no OBS required.
</p>

<p align="center">
  <b>English</b> · <a href="README.pt-BR.md">Português (Brasil)</a>
</p>

<p align="center">
  <img src="media/demo.gif" alt="Demo GIF" width="800">
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
| **One-step install** | `install.bat` creates the environment, installs dependencies and registers the camera driver. |
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

**Windows**

```bash
install.bat
```

This creates the virtual environment, installs the Python dependencies and registers the
UnityCapture driver. Windows will ask for Administrator permission at the driver step —
that step needs it; the rest does not.

Requires Python 3.11–3.13 and `ffmpeg` on your PATH.

The first launch downloads roughly 1 GB of models from
[Hugging Face](https://huggingface.co/hacksider/deep-live-cam). The window will look frozen
while that happens — progress only shows in the terminal. Downloads resume if interrupted.

**Linux / macOS**

Follow the [upstream installation guide](https://github.com/hacksider/Deep-Live-Cam#installation-manual).
The virtual camera in this fork is Windows-only; everything else works.

---

## Using the virtual camera

1. Run `python run.py`
2. Pick **Live camera** mode
3. Choose a source face image
4. Turn on **Send video to a virtual camera**
5. Click **Start live**
6. In Zoom, Discord, Teams or Meet, select **Unity Video Capture** as your camera

Restart the calling app if the camera does not appear — most of them enumerate devices once
at startup.

To remove the driver later: `uninstall_virtual_camera.bat`.

---

## GPU acceleration

CUDA, DirectML, OpenVINO and CoreML all work as upstream documents them. See the
[upstream GPU guide](https://github.com/hacksider/Deep-Live-Cam#gpu-acceleration) for the
provider-specific install commands.

The header shows which accelerator is active. If it says **CPU mode** on a machine with a
supported GPU, the provider failed to load — usually a CUDA/cuDNN version mismatch. The app
still runs, just slowly.

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
