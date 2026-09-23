"""世界计划固车助手 · NoneBot2 插件（OneBot V11）。"""
from __future__ import annotations

import asyncio
import json
import logging
import time

from nonebot import get_plugin_config, on_message, require
from nonebot.adapters.onebot.v11 import Bot, GroupMessageEvent
from nonebot.plugin import PluginMetadata

from .config import Config
from .core import compute_new_name, is_superuser, match_message

require("nonebot_plugin_localstore")
import nonebot_plugin_localstore as store  # noqa: E402

__plugin_meta__ = PluginMetadata(
    name="世界计划固车助手",
    description="《Project SEKAI》固车群自动拾取车牌改名，方便固车群友快速上车",
    usage=(
        "在群内发送 /开启固车拾取 启用；之后发送 5 位数字即可自动写入群名。\n"
        "发送 /固车拾取 使用方法 查看完整指令。"
    ),
    type="application",
    homepage="https://github.com/Shinonome-ena/sekai-room-code-renamer",
    config=Config,
    supported_adapters={"~onebot.v11"},
)

_config = get_plugin_config(Config)
_DATA_FILE = store.get_plugin_data_file("groups.json")
_groups: dict[int, dict] = {}
_cooldown: dict[int, float] = {}

_HELP = """\
世界计划固车助手使用帮助
━━━━━━━━━━━━━━━━

/开启固车拾取    - 启用本群自动改名功能
/关闭固车拾取    - 关闭本群自动改名功能
/固车模式        - 查看/设置匹配模式
发送5位数字      - 自动将数字添加到群名
/固车拾取 使用方法 - 显示本帮助

模式设置示例：
/固车模式 前缀    - 车牌放在群名前面
/固车模式 后缀    - 车牌放在群名后面
/固车模式 严格    - 只识别纯5位数字
/固车模式 宽松    - 消息开头提取5位数字

注意: 开启/关闭和模式设置需要管理员权限，改名功能所有人可用"""


def _load_groups() -> None:
    global _groups
    if not _DATA_FILE.exists():
        return
    try:
        raw = json.loads(_DATA_FILE.read_text("utf-8"))
        _groups = {int(k): dict(v) for k, v in raw.items()}
    except Exception:
        logging.error("sekai-guche: groups.json 解析失败，忽略")


def _save_groups() -> None:
    data = {str(k): v for k, v in _groups.items()}
    _DATA_FILE.write_text(json.dumps(data, ensure_ascii=False, indent=2), "utf-8")


async def _persist() -> None:
    await asyncio.to_thread(_save_groups)


_load_groups()


def _fuzzy_for(gid: int) -> bool:
    m = (_groups.get(gid) or {}).get("mode")
    return _config.guche_fuzzy if m is None else bool(m)


def _pos_end_for(gid: int) -> bool:
    p = (_groups.get(gid) or {}).get("pos")
    return _config.guche_pos_end if p is None else bool(p)


def _set_field(gid: int, pos: bool | None = None, mode: bool | None = None) -> list[str]:
    """只改指定字段；未传的不动。返回变化说明。"""
    e = _groups.get(gid)
    if e is None:
        return []
    changed = []
    if pos is not None and e.get("pos") != pos:
        e["pos"] = pos
        changed.append("后缀" if pos else "前缀")
    if mode is not None and e.get("mode") != mode:
        e["mode"] = mode
        changed.append("宽松" if mode else "严格")
    return changed


async def _reply(bot: Bot, gid: int, mid: int, text: str) -> None:
    await bot.send_group_msg(
        group_id=gid,
        message=f"[CQ:reply,id={mid}]{text}",
    )


async def _dispatch(bot: Bot, event: GroupMessageEvent) -> None:
    gid = int(event.group_id)
    uid = int(event.user_id)
    mid = int(event.message_id)
    text = event.message.extract_plain_text()
    trimmed = text.strip()
    admins = _config.guche_admin_users

    if trimmed == _config.guche_enable_cmd.strip():
        if not is_superuser(uid, admins):
            await _reply(bot, gid, mid, "权限不足")
            return
        if gid in _groups:
            await _reply(bot, gid, mid, "本群已启用固车助手功能")
            return
        _groups[gid] = {}
        await _persist()
        await _reply(bot, gid, mid, "已启动本群的自动拾取车牌功能")
        return

    if trimmed == _config.guche_disable_cmd.strip():
        if not is_superuser(uid, admins):
            await _reply(bot, gid, mid, "权限不足")
            return
        if gid not in _groups:
            return  # 本来就没开，静默
        del _groups[gid]
        await _persist()
        await _reply(bot, gid, mid, "固车助手功能已关闭，下一次冲榜见~")
        return

    if trimmed == _config.guche_help_cmd.strip():
        await _reply(bot, gid, mid, _HELP)
        return

    mode_cmd = _config.guche_mode_cmd.strip()
    if mode_cmd and trimmed.startswith(mode_cmd):
        if not is_superuser(uid, admins):
            await _reply(bot, gid, mid, "权限不足")
            return
        if gid not in _groups:
            return  # 未启用不创建
        args = trimmed[len(mode_cmd) :].strip()
        if not args:
            pos_end = _pos_end_for(gid)
            fuzzy = _fuzzy_for(gid)
            await _reply(
                bot,
                gid,
                mid,
                "固车模式设置\n━━━━━━━━━━━━━━━━\n\n"
                f"{mode_cmd} 前缀    - 车牌放在群名前面\n"
                f"{mode_cmd} 后缀    - 车牌放在群名后面\n"
                f"{mode_cmd} 严格    - 只识别纯5位数字\n"
                f"{mode_cmd} 宽松    - 消息开头提取5位数字\n"
                f"{mode_cmd} 前缀 严格  - 同时设置两个\n\n"
                f"当前模式：{'后缀' if pos_end else '前缀'} + {'宽松' if fuzzy else '严格'}",
            )
            return
        pos = mode = None
        for part in args.split():
            if part == "前缀":
                pos = False
            elif part == "后缀":
                pos = True
            elif part == "严格":
                mode = False
            elif part == "宽松":
                mode = True
            else:
                await _reply(bot, gid, mid, f"未知参数: {part}")
                return
        changed = _set_field(gid, pos=pos, mode=mode)
        if not changed:
            await _reply(bot, gid, mid, "设置未变化")
            return
        await _persist()
        await _reply(bot, gid, mid, "已切换为: " + " + ".join(changed))
        return

    if gid not in _groups:
        return

    now = time.time()
    if now - _cooldown.get(gid, 0.0) < _config.guche_cooldown_seconds:
        return

    code = match_message(text, _fuzzy_for(gid))
    if not code:
        return

    _cooldown[gid] = now

    try:
        info = await bot.get_group_info(group_id=gid)
        old_name = str(info.get("group_name", ""))
        new_name, truncated = compute_new_name(old_name, code, _pos_end_for(gid))
        await bot.set_group_name(group_id=gid, group_name=new_name)
        note = "（群名过长已截断）" if truncated else ""
        await _reply(bot, gid, mid, f"已将群聊名称从'{old_name}'改为'{new_name}'{note}")
    except Exception as e:
        logging.error("sekai-guche 异常: 群%s %s", gid, e)


sekai_guche = on_message(
    rule=lambda event: isinstance(event, GroupMessageEvent),
    priority=10,
    block=False,
)


@sekai_guche.handle()
async def _handle(bot: Bot, event: GroupMessageEvent):
    await _dispatch(bot, event)
