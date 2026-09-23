"""插件配置（NoneBot 标准 Config，字段均可省略）。"""
from __future__ import annotations

from pydantic import BaseModel, Field


class Config(BaseModel):
    guche_admin_users: list[int] = Field(default_factory=list)
    guche_fuzzy: bool = False
    guche_pos_end: bool = False
    guche_cooldown_seconds: float = 5.0
    guche_enable_cmd: str = "/开启固车拾取"
    guche_disable_cmd: str = "/关闭固车拾取"
    guche_help_cmd: str = "/固车拾取 使用方法"
    guche_mode_cmd: str = "/固车模式"
