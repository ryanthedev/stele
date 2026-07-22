#!/usr/bin/env python3
"""Extract spec examples into JSON test corpora.

Replicates `spec_tests.py --dump-tests` (which imports a `cmark` module at
top level and so cannot run standalone). Regenerates:

  commonmark-0.31.2.json  (652 examples from commonmark-spec.txt)
  gfm-extensions.json     (extension sections from gfm-spec.txt)

Run from this directory: python3 extract_tests.py
"""

import json
import re
from pathlib import Path

HERE = Path(__file__).resolve().parent


def get_tests(specfile: Path):
    line_number = 0
    start_line = 0
    example_number = 0
    markdown_lines: list[str] = []
    html_lines: list[str] = []
    state = 0  # 0 regular text, 1 markdown example, 2 html output
    headertext = ""
    tests = []
    header_re = re.compile("#+ ")
    with open(specfile, "r", encoding="utf-8", newline="\n") as specf:
        for line in specf:
            line_number += 1
            stripped = line.strip()
            if stripped.startswith("`" * 32 + " example"):
                state = 1
            elif stripped == "`" * 32:
                state = 0
                example_number += 1
                tests.append(
                    {
                        "markdown": "".join(markdown_lines).replace("→", "\t"),
                        "html": "".join(html_lines).replace("→", "\t"),
                        "example": example_number,
                        "start_line": start_line,
                        "end_line": line_number,
                        "section": headertext,
                    }
                )
                start_line = 0
                markdown_lines = []
                html_lines = []
            elif stripped == ".":
                state = 2
            elif state == 1:
                if start_line == 0:
                    start_line = line_number - 1
                markdown_lines.append(line)
            elif state == 2:
                html_lines.append(line)
            elif state == 0 and re.match(header_re, line):
                headertext = header_re.sub("", line).strip()
    return tests


def main() -> None:
    cm = get_tests(HERE / "commonmark-spec.txt")
    json.dump(cm, open(HERE / "commonmark-0.31.2.json", "w"), indent=1)
    print(f"commonmark: {len(cm)} examples")

    gfm = get_tests(HERE / "gfm-spec.txt")
    keep = {
        "Tables (extension)",
        "Task list items (extension)",
        "Strikethrough (extension)",
        "Autolinks (extension)",
    }
    sel = [t for t in gfm if t["section"] in keep]
    json.dump(sel, open(HERE / "gfm-extensions.json", "w"), indent=1)
    print(f"gfm extensions: {len(sel)} examples")


if __name__ == "__main__":
    main()
