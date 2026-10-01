"""Compile the living manuscript, keeping generated artifacts outside Git."""
import argparse
import os
from pathlib import Path
import shutil
import subprocess


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-directory", type=Path, required=True)
    args = parser.parse_args()
    source = Path(__file__).resolve().parent
    output = args.output_directory.resolve()
    if output.is_relative_to(source.parent):
        raise ValueError("keep paper build artifacts outside the repository")
    for executable in ("pdflatex", "bibtex"):
        if shutil.which(executable) is None:
            raise RuntimeError(f"install {executable} to compile the manuscript")
    output.mkdir(parents=True, exist_ok=True)
    latex = ["pdflatex", "-no-shell-escape", "-interaction=nonstopmode", "-halt-on-error",
             f"-output-directory={output}", "main.tex"]
    environment = dict(os.environ)
    environment["BIBINPUTS"] = str(source) + os.pathsep + environment.get("BIBINPUTS", "")
    commands = [(latex, source), (["bibtex", "main"], output), (latex, source), (latex, source)]
    for i, (command, cwd) in enumerate(commands):
        with (output / f"build-{i}.log").open("w") as log:
            subprocess.run(command, cwd=cwd, env=environment, stdout=log,
                           stderr=subprocess.STDOUT, check=True)
    final_log = (output / "main.log").read_text()
    if "undefined" in final_log or "Rerun to get cross-references right" in final_log:
        raise RuntimeError("manuscript still has unresolved references; inspect main.log")
    print(output / "main.pdf")


if __name__ == "__main__":
    main()
