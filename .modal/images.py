"""The sandbox image, built once and published as `goble-agent`.

Modal's guide asks for named images when sandboxes are involved: a sandbox must
never block on an image build. This is the only place that builds the image.

    modal run .modal/images.py

The control plane (`.modal/app.py`) only refers to it with `Image.from_name`.
"""

from __future__ import annotations

import pathlib

import modal

APP_NAME = "sandboxes"
IMAGE_NAME = "goble-agent"
HERE = pathlib.Path(__file__).resolve().parent

app = modal.App(APP_NAME)


def agent_image() -> modal.Image:
    return modal.Image.from_dockerfile(HERE / "Dockerfile", context_dir=str(HERE))


@app.local_entrypoint()
def build():
    with modal.enable_output():
        agent_image().build(app).publish(IMAGE_NAME)
    print(f"published {IMAGE_NAME}")
