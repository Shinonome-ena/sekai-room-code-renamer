"""固车核心逻辑（与 guche-core 行为对齐）。无 I/O、无 NoneBot 依赖。"""
from __future__ import annotations

import re

HEAD_FIVE_DIGIT_RE = re.compile(r"^(\d{5})\s+(.+)$")
TAIL_FIVE_DIGIT_RE = re.compile(r"^(.*\s)?(\d{5})$")
GROUP_NAME_MAX_LEN = 60


def match_message(text: str, fuzzy: bool) -> str | None:
    """fuzzy=False 严格（整串 5 位）；True 宽松（开头 5 位，第 6 位不能是数字）。"""
    if not fuzzy:
        return text if re.fullmatch(r"\d{5}", text) else None
    trimmed = text.strip()
    m = re.match(r"(\d{5})", trimmed)
    if not m:
        return None
    if trimmed[m.end():][:1].isdigit():
        return None
    return m.group(1)


def compute_new_name(old_name: str, new_code: str, end: bool) -> tuple[str, bool]:
    """end=False 车牌在前，True 在后。返回 (新群名, 是否截断过)。"""
    if not end:
        m = HEAD_FIVE_DIGIT_RE.match(old_name)
        if m:
            base_name = m.group(2) or ""
        elif len(old_name) == 5 and old_name.isdigit():
            base_name = ""
        else:
            base_name = old_name
    else:
        m = TAIL_FIVE_DIGIT_RE.match(old_name)
        base_name = (m.group(1) or "").rstrip() if m else old_name

    def join(base: str) -> str:
        if not base:
            return new_code
        return f"{new_code} {base}" if not end else f"{base} {new_code}"

    new_name = join(base_name)
    if len(new_name) <= GROUP_NAME_MAX_LEN:
        return new_name, False

    overflow = len(new_name) - GROUP_NAME_MAX_LEN
    base_len = len(base_name)
    if base_len > overflow:
        trimmed_base = (
            base_name[: base_len - overflow] if not end else base_name[overflow:]
        )
    else:
        trimmed_base = ""
    return join(trimmed_base), True


def is_superuser(user_id: int, admin_users: list[int]) -> bool:
    """空列表 = 所有人可执行管理指令。"""
    return (not admin_users) or (user_id in admin_users)
