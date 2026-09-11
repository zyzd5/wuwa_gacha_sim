#!/usr/bin/env python3
"""鸣潮唤取记录抓取 + 分析（只用 Python 标准库，无第三方依赖）。

用途：把你自己账号的抽卡记录拉下来做聚合统计，回答两个问题：

1. 4★ 内容里**武器**到底占多少比例（官方从未公示，模拟器里目前是猜的 0.25）。
2. 实际的 5★ 出金间隔分布，能否验证模拟器的软保底曲线。

# 隐私

抽卡链接里的 record_id 等同于读取你抽卡记录的凭证。本脚本：

- **只把请求发给库洛官方接口**（gmserver-api.aki-game2.com / .net），不发往任何其他地方；
- **不打印、不落盘** URL 或任何 ID（player_id / record_id / svr_id / resources_id）；
- 除非你显式加 `--dump`，否则不保存原始记录；`--dump` 出来的文件含账号相关字段，
  请自行保管好，别提交进 git。

# 用法

    python3 tools/gacha_history.py "<抽卡链接>"
    python3 tools/gacha_history.py --dump /tmp/ww.json "<抽卡链接>"   # 顺便存原始记录
    python3 tools/gacha_history.py --from-dump /tmp/ww.json           # 离线重算

链接从游戏里「唤取记录」页面取（PC 端在 Client.log 里搜 aki-gm-resources.aki-game.com）。
**链接只有约 1 小时有效期**，过期后接口会静默返回空数组（不报错），脚本会提示。
"""

from __future__ import annotations

import argparse
import json
import math
import sys
import urllib.error
import urllib.request
from collections import Counter, defaultdict
from urllib.parse import parse_qs, urlparse

# 请求里 cardPoolType 的枚举范围。URL 里的 gacha_type 不能直接拿来用，
# 必须逐个问，接口才知道你要哪个池子。
POOL_TYPES = range(1, 14)

# 国内 / 国际服接口
ENDPOINTS = {
    "cn": "https://gmserver-api.aki-game2.com/gacha/record/query",
    "oversea": "https://gmserver-api.aki-game2.net/gacha/record/query",
}

TIMEOUT = 30


# ─────────────────────────── 抓取 ───────────────────────────


def parse_link(url: str) -> dict[str, str]:
    """从抽卡链接里取出请求参数。返回值**不会**被打印。"""
    parsed = urlparse(url.replace("#", ""))
    host = parsed.hostname or ""
    if host == "aki-gm-resources.aki-game.com":
        region = "cn"
    elif host == "aki-gm-resources-oversea.aki-game.net":
        region = "oversea"
    else:
        raise SystemExit(
            f"无法识别的链接域名（{host!r}）。\n"
            "国服应为 aki-gm-resources.aki-game.com，国际服应为 aki-gm-resources-oversea.aki-game.net。"
        )

    query = {k: v[0] for k, v in parse_qs(parsed.query).items() if v}
    missing = [k for k in ("resources_id", "record_id", "player_id", "svr_id") if k not in query]
    if missing:
        raise SystemExit(f"链接里缺少参数：{', '.join(missing)}。请确认复制的是完整的唤取记录 URL。")

    return {
        "region": region,
        "cardPoolId": query["resources_id"],
        "recordId": query["record_id"],
        "playerId": query["player_id"],
        "serverId": query["svr_id"],
        "languageCode": query.get("lang", "zh-Hans"),
    }


def query_pool(params: dict[str, str], pool_type: int) -> list[dict]:
    """向官方接口要某个池子的全部记录。一次请求即返回该池完整历史，无需分页。"""
    body = dict(params)
    body.pop("region", None)
    body["cardPoolType"] = pool_type

    request = urllib.request.Request(
        ENDPOINTS[params["region"]],
        data=json.dumps(body).encode("utf-8"),
        headers={"Content-Type": "application/json", "User-Agent": "Mozilla/5.0"},
        method="POST",
    )
    with urllib.request.urlopen(request, timeout=TIMEOUT) as response:
        payload = json.loads(response.read().decode("utf-8"))

    if payload.get("code") != 0:
        raise SystemExit(f"接口返回错误：code={payload.get('code')} message={payload.get('message')!r}")
    return payload.get("data") or []


def fetch_all(params: dict[str, str]) -> dict[int, list[dict]]:
    pools: dict[int, list[dict]] = {}
    for pool_type in POOL_TYPES:
        try:
            records = query_pool(params, pool_type)
        except urllib.error.URLError as error:
            raise SystemExit(f"网络请求失败：{error}") from error
        if records:
            pools[pool_type] = records
        print(f"  卡池类型 {pool_type:>2}：{len(records):>5} 条记录", file=sys.stderr)
    return pools


# ─────────────────────────── 归类 ───────────────────────────


def is_weapon(record: dict) -> bool:
    kind = str(record.get("resourceType", ""))
    return "武器" in kind or "weapon" in kind.lower()


def quality(record: dict) -> int:
    try:
        return int(record.get("qualityLevel", 0))
    except (TypeError, ValueError):
        return 0


def pool_label(records: list[dict]) -> str:
    """接口会在记录里带上卡池名（各版本字段名可能不同），能拿到就拿来用。"""
    for key in ("cardPoolType", "cardPoolName", "card_pool_type"):
        value = records[0].get(key)
        if isinstance(value, str) and value:
            return value
    return "?"


# ─────────────────────────── 统计 ───────────────────────────


def wilson(successes: int, total: int, z: float = 1.96) -> tuple[float, float]:
    """Wilson 95% 置信区间，比正态近似在小样本下稳。"""
    if total == 0:
        return (0.0, 1.0)
    p = successes / total
    denominator = 1 + z * z / total
    centre = (p + z * z / (2 * total)) / denominator
    half = z * math.sqrt(p * (1 - p) / total + z * z / (4 * total * total)) / denominator
    return (max(0.0, centre - half), min(1.0, centre + half))


def describe_pool(pool_type: int, records: list[dict]) -> dict:
    """所有池子通用的概览。"""
    order = sorted(records, key=lambda r: str(r.get("time", "")))
    four = [r for r in order if quality(r) == 4]
    five = [r for r in order if quality(r) == 5]
    four_weapon = sum(1 for r in four if is_weapon(r))
    return {
        "pool_type": pool_type,
        "label": pool_label(records),
        "total": len(order),
        "three": sum(1 for r in order if quality(r) == 3),
        "four": len(four),
        "four_weapon": four_weapon,
        "four_character": len(four) - four_weapon,
        "five": len(five),
        "five_weapon": sum(1 for r in five if is_weapon(r)),
        "five_character": sum(1 for r in five if not is_weapon(r)),
        "five_names": Counter(str(r.get("name", "?")) for r in five),
        "first_time": order[0].get("time") if order else None,
        "last_time": order[-1].get("time") if order else None,
        "order": order,
    }


def five_star_gaps(order: list[dict]) -> list[int]:
    """相邻两个 5★ 之间隔了多少抽。

    只统计**两端都在记录范围内**的间隔。第一个间隔的左端是未知的（6 个月窗口从中间截断），
    所以从第 1 个 5★ 之后开始算。
    """
    indices = [i for i, r in enumerate(order) if quality(r) == 5]
    return [b - a for a, b in zip(indices, indices[1:])]


# ─────────────────────────── 报告 ───────────────────────────


def print_pool_table(summaries: list[dict]) -> None:
    print()
    print("=" * 78)
    print("各卡池概览")
    print("=" * 78)
    print(f"{'类型':>4}  {'卡池名':<22} {'总数':>6} {'4★':>5} {'4★角色':>7} {'4★武器':>7} {'5★':>4}")
    print("-" * 78)
    for s in summaries:
        print(
            f"{s['pool_type']:>4}  {s['label']:<22} {s['total']:>6} {s['four']:>5} "
            f"{s['four_character']:>7} {s['four_weapon']:>7} {s['five']:>4}"
        )
        print(f"      {s['first_time']} → {s['last_time']}")


def pick_limited_pool(summaries: list[dict]) -> dict | None:
    """猜哪个是「角色活动唤取」：5★ 基本都是角色、且 5★ 名字种类最多的那个池子。

    角色常驻池的 5★ 只有固定的几个常驻角色，武器池的 5★ 是武器，
    所以「5★ 全是角色 且 名字最杂」基本只可能是限定角色池。
    """
    candidates = [s for s in summaries if s["five"] > 0 and s["five_weapon"] == 0 and s["four"] > 0]
    if not candidates:
        return None
    return max(candidates, key=lambda s: len(s["five_names"]))


def report_weapon_share(summary: dict) -> None:
    total = summary["four"]
    weapons = summary["four_weapon"]
    low, high = wilson(weapons, total)

    print()
    print("=" * 78)
    print(f"① 4★ 角色 / 武器占比  ——  来自卡池「{summary['label']}」")
    print("=" * 78)
    print(f"  样本：{total} 个 4★   角色 {summary['four_character']}   武器 {weapons}")
    if total:
        print(f"  武器占比：{weapons / total * 100:.2f}%   95% 置信区间 [{low * 100:.2f}%, {high * 100:.2f}%]")
    print()
    if total < 30:
        print("  ⚠️ 样本太少（<30），置信区间很宽，只能当作参考。")
    elif total < 100:
        print("  ⚠️ 样本偏少（<100），能排除极端值，但还定不死。")
    else:
        print("  ✅ 样本量足够给一个像样的估计。")

    if total:
        half_width = (high - low) / 2 * 100
        print()
        print(f"  按这个估计，每个 4★ 平均给 "
              f"{(1 - weapons / total) * 8 + (weapons / total) * 3:.3f} 个大珊瑚")
        print(f"  （当前模拟器用的是 0.25，对应 6.75；区间半宽 ±{half_width:.1f} 个百分点）")


def report_gaps(summary: dict) -> None:
    gaps = five_star_gaps(summary["order"])
    print()
    print("=" * 78)
    print(f"② 5★ 出金间隔（未出金抽数 + 1）——  来自卡池「{summary['label']}」")
    print("=" * 78)
    if not gaps:
        print("  记录里不足 2 个 5★，算不出间隔。")
        return

    print(f"  样本：{len(gaps)} 个完整间隔（首个间隔因窗口截断被排除）")
    print(f"  间隔明细：{sorted(gaps)}")
    print()
    print(f"  平均间隔：{sum(gaps) / len(gaps):.2f} 抽      模拟器理论值 53.63")
    early = sum(1 for g in gaps if g <= 65)
    print(f"  软保底前出金（≤65 抽）：{early}/{len(gaps)} = {early / len(gaps) * 100:.1f}%   "
          f"模型期望 40.67%")
    late = sum(1 for g in gaps if g >= 78)
    print(f"  逼近硬保底（≥78 抽）：{late}/{len(gaps)} = {late / len(gaps) * 100:.1f}%")
    print()
    low, high = wilson(sum(1 for g in gaps if g <= 65), len(gaps))
    print(f"  「≤65 抽出金」比例 95% 置信区间：[{low * 100:.1f}%, {high * 100:.1f}%]")
    print("  若区间把 40.7% 排除在外，说明软保底曲线需要调整。")


def report_five_star_split(summary: dict) -> None:
    total = summary["five"]
    if not total:
        return
    standard_looking = ["凌阳", "安可", "卡卡罗", "鉴心", "维里奈"]
    standard = sum(count for name, count in summary["five_names"].items() if name in standard_looking)
    print()
    print("=" * 78)
    print("③ 5★ 限定 / 常驻 拆分（按常驻角色名单粗略识别）")
    print("=" * 78)
    print(f"  5★ 共 {total}：疑似常驻 {standard}，其余 {total - standard}")
    for name, count in summary["five_names"].most_common():
        print(f"    {name:<16} × {count}")


def main() -> int:
    parser = argparse.ArgumentParser(description="抓取并分析鸣潮唤取记录")
    parser.add_argument("link", nargs="?", help="游戏内「唤取记录」页面的完整 URL")
    parser.add_argument("--dump", metavar="PATH", help="把原始记录存成 JSON（含账号字段，注意保管）")
    parser.add_argument("--from-dump", metavar="PATH", help="从之前 --dump 出来的文件离线重算")
    args = parser.parse_args()

    if args.from_dump:
        with open(args.from_dump, encoding="utf-8") as handle:
            raw = json.load(handle)
        pools = {int(k): v for k, v in raw.items()}
    else:
        if not args.link:
            print("请把抽卡链接作为参数传进来，或用 --from-dump 读本地文件。", file=sys.stderr)
            return 2
        params = parse_link(args.link)
        print("正在向库洛官方接口拉取记录……", file=sys.stderr)
        pools = fetch_all(params)
        if not any(pools.values()):
            print(
                "\n所有卡池都返回空数据。最可能的原因是**链接已过期**"
                "（有效期约 1 小时）：请重新打开游戏内的唤取记录页面，再取一次链接。",
                file=sys.stderr,
            )
            return 1
        if args.dump:
            with open(args.dump, "w", encoding="utf-8") as handle:
                json.dump(pools, handle, ensure_ascii=False)
            print(f"\n原始记录已写入 {args.dump}（含账号字段，别提交进 git）", file=sys.stderr)

    summaries = [describe_pool(t, r) for t, r in sorted(pools.items())]
    summaries = [s for s in summaries if s["total"] > 0]
    if not summaries:
        print("没有解析到任何记录。", file=sys.stderr)
        return 1

    print_pool_table(summaries)

    limited = pick_limited_pool(summaries)
    if limited is None:
        print()
        print("⚠️ 没有找到符合「5★ 全为角色且 4★ 有记录」的卡池，无法估算 4★ 武器占比。")
        print("   请把上面的「各卡池概览」整段发给我，我来人工判断。")
        return 0

    report_weapon_share(limited)
    report_gaps(limited)
    report_five_star_split(limited)

    print()
    print("=" * 78)
    print("把以上全部内容发给我即可（不含任何账号凭证）")
    print("=" * 78)
    return 0


if __name__ == "__main__":
    sys.exit(main())
