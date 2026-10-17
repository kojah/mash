#!/usr/bin/env bash
# Source from canonical runners so fixtures and fleet commands use one runtime.
export UV_PROJECT_ENVIRONMENT=${UV_PROJECT_ENVIRONMENT:-/home/james/scratch/mash-artifacts/ansible-venv}
mash_controller_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
uv sync --project "$mash_controller_root/deployment" --locked --quiet
