#!/usr/bin/env python3
import json
import re
import sys
from urllib.parse import quote


HEX_COLOR_RE = re.compile(r"^#([0-9a-fA-F]{3}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8})$")


def normalize_color(raw: str) -> tuple[str, int, int, int, int | None] | None:
    match = HEX_COLOR_RE.fullmatch(raw.strip())
    if not match:
        return None

    value = match.group(1).lower()
    if len(value) == 3:
        value = "".join(ch * 2 for ch in value)

    rgb = value[:6]
    alpha = value[6:8] if len(value) == 8 else None
    red = int(rgb[0:2], 16)
    green = int(rgb[2:4], 16)
    blue = int(rgb[4:6], 16)
    alpha_value = int(alpha, 16) if alpha else None
    normalized = f"#{rgb.upper()}{alpha.upper() if alpha else ''}"
    return normalized, red, green, blue, alpha_value


def color_icon(red: int, green: int, blue: int, alpha: int | None) -> str:
    opacity = "" if alpha is None else f' fill-opacity="{alpha / 255:.3f}"'
    fill = f"#{red:02X}{green:02X}{blue:02X}"
    background = ""
    if alpha is not None:
        background = """
  <rect x="4" y="4" width="56" height="56" fill="#f6f5f4"/>
  <path d="M4 60 60 4v56z" fill="#deddda"/>"""

    svg = f"""<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64" viewBox="0 0 64 64">
  <defs>
    <clipPath id="swatch">
      <rect x="4" y="4" width="56" height="56" rx="12"/>
    </clipPath>
  </defs>
  <g clip-path="url(#swatch)">{background}
    <rect x="4" y="4" width="56" height="56" fill="{fill}"{opacity}/>
  </g>
  <rect x="4" y="4" width="56" height="56" rx="12" fill="none" stroke="#000000" stroke-opacity="0.22" stroke-width="2"/>
</svg>
"""
    return "data:image/svg+xml," + quote(svg, safe="/:=;,+")


def luminance(red: int, green: int, blue: int) -> float:
    def channel(value: int) -> float:
        value = value / 255
        if value <= 0.03928:
            return value / 12.92
        return ((value + 0.055) / 1.055) ** 2.4

    return 0.2126 * channel(red) + 0.7152 * channel(green) + 0.0722 * channel(blue)


def main() -> int:
    query = sys.argv[1] if len(sys.argv) > 1 else ""
    parsed = normalize_color(query)
    if parsed is None:
        return 0

    color, red, green, blue, alpha = parsed
    icon = color_icon(red, green, blue, alpha)
    alpha_text = "" if alpha is None else f", alpha {alpha}"
    tone = "light" if luminance(red, green, blue) >= 0.5 else "dark"
    result = {
        "title": color,
        "subtitle": f"RGB {red}, {green}, {blue}{alpha_text} - {tone} color",
        "icon": icon,
        "pinned": True,
        "action": {"type": "copy-text", "text": color},
    }
    print(json.dumps(result, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
